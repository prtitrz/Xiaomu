//! Per-editor admission of measured tables; canonical data is never repaired.
//!
//! Building children and accepting input are separate gates. A checked plan
//! permits the first build, while editing requires a successful measurement
//! for that exact table shape and presentation. Every gate checks the current
//! snapshot, including all enclosing tables, rather than trusting a mounted
//! child or a document-wide revision that ordinary typing would invalidate.
//! A bounded current-snapshot cache shares exact structural keys across input
//! queries; snapshot invalidation never replaces structural admission checks.

use std::{cell::RefCell, rc::Rc};

use gpui::Hsla;
use xiaomu_core::document::{AttrValue, NodeAttrs, NodeId, NodeKind, TableGrid, XiaomuDocument};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

use crate::table_layout::{TableLayoutError, TableLayoutOptions, TableLayoutPlan};

#[path = "table_capability/cache.rs"]
mod cache;
use cache::CapabilityCache;

#[cfg(test)]
#[path = "table_capability/cache_tests.rs"]
mod cache_tests;

/// One shared state object retained by the document and every input handler.
pub(crate) type SharedTableCapability = Rc<RefCell<TableCapability>>;

/// Exact admission inputs, deliberately excluding revision and inline content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TableCapabilityKey {
    root: NodeId,
    grid: TableGrid,
    presentation: Vec<(NodeId, NodeKind, NodeAttrs)>,
}

/// Opt-in measurement successes belonging to one editor instance only.
#[derive(Debug, Default)]
pub(crate) struct TableCapability {
    enabled: bool,
    cache: RefCell<CapabilityCache>,
}

impl TableCapability {
    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Mutate the shared object in place so retained handlers see revocation.
    /// Even a repeated setting requires a fresh successful measurement.
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        *self.cache.get_mut() = CapabilityCache::default();
    }

    /// Validate geometry and the exact presentation this renderer implements.
    pub(crate) fn key(
        &self,
        document: &XiaomuDocument,
        table: NodeId,
    ) -> Result<Rc<TableCapabilityKey>, TableLayoutError> {
        self.cache.borrow_mut().key(document, table)
    }

    /// Geometry is sufficient to materialize children before first measurement.
    pub(crate) fn can_build(&self, document: &XiaomuDocument, table: NodeId) -> bool {
        if self.enabled {
            self.key(document, table).is_ok()
        } else {
            legacy_permits(document, table)
        }
    }

    /// Freshly validate the snapshot before trusting a measured success.
    pub(crate) fn permits(&self, document: &XiaomuDocument, table: NodeId) -> bool {
        if !self.enabled {
            return legacy_permits(document, table);
        }
        self.cache.borrow_mut().permits(document, table)
    }

    pub(crate) fn permits_document(&self, document: &XiaomuDocument) -> bool {
        if self.enabled {
            // Even deleting the final table must prune its measured success;
            // an immediate Undo then needs a fresh measurement in this view.
            self.cache.borrow_mut().prepare(document);
        }
        document
            .store()
            .iter()
            .filter(|node| node.kind() == &NodeKind::Table)
            .all(|table| self.enabled && self.permits(document, table.id()))
    }

    /// A failed measurement revokes the previous success, even for the same key.
    pub(crate) fn record(&mut self, table: NodeId, key: Rc<TableCapabilityKey>, success: bool) {
        if success && self.enabled && key.grid.table() == table {
            self.cache.get_mut().record(table, key);
        } else {
            self.revoke(table);
        }
    }

    pub(crate) fn revoke(&mut self, table: NodeId) {
        self.cache.get_mut().revoke(table);
    }

    /// The snapshot index avoids a full-tree Core parent search per paragraph.
    pub(crate) fn parent_of(&self, document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
        self.cache.borrow_mut().parent_of(document, node)
    }

    fn selection_is_valid(&self, document: &XiaomuDocument, selection: DocumentSelection) -> bool {
        self.cache
            .borrow_mut()
            .selection_is_valid(document, selection)
    }

    #[cfg(test)]
    fn cache_counts(&self) -> (usize, usize, usize) {
        self.cache.borrow().counts()
    }

    #[cfg(test)]
    fn cache_sizes(&self) -> (usize, usize, usize) {
        self.cache.borrow().sizes()
    }

    /// Includes `node` itself and keeps walking past successful inner tables.
    pub(crate) fn hidden_ancestor(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
    ) -> Option<NodeId> {
        self.hidden_ancestor_matching(document, node, false)
    }

    /// The build traversal must not depend on a prior measurement success.
    pub(crate) fn hidden_ancestor_for_build(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
    ) -> Option<NodeId> {
        self.hidden_ancestor_matching(document, node, true)
    }

    fn hidden_ancestor_matching(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
        building: bool,
    ) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(id) = current {
            if document.node(id)?.kind() == &NodeKind::Table
                && !(if building {
                    self.can_build(document, id)
                } else {
                    self.permits(document, id)
                })
            {
                return Some(id);
            }
            current = if self.enabled {
                self.parent_of(document, id)
            } else {
                document.parent_of(id)
            };
        }
        None
    }
}

fn build_key(
    document: &XiaomuDocument,
    table: NodeId,
) -> Result<TableCapabilityKey, TableLayoutError> {
    let grid = document.table_grid(table)?;
    let mut presentation = Vec::with_capacity(1 + grid.rows() + grid.origins().len());
    for id in std::iter::once(table)
        .chain((0..grid.rows()).filter_map(|row| grid.row_id(row)))
        .chain(grid.origins().map(|placement| placement.cell()))
    {
        let node = document.node(id).ok_or(xiaomu_core::Error::UnknownNode)?;
        match node.kind() {
            NodeKind::Table | NodeKind::TableRow if node.attrs().is_empty() => {}
            NodeKind::TableCell | NodeKind::TableHeader => {
                validate_cell_presentation(node.attrs())?;
            }
            _ => return Err(TableLayoutError::InvalidDimension),
        }
        presentation.push((id, node.kind().clone(), node.attrs().clone()));
    }
    // Preserve exact raw attrs; the renderer rejects unsupported width hints.
    // This work runs once per table/snapshot, never once per native query.
    let plan = TableLayoutPlan::from_document(document, table, TableLayoutOptions::default())?;
    plan.layout(0.0, &vec![0.0; plan.cells().len()])?;
    Ok(TableCapabilityKey {
        root: document.root(),
        grid,
        presentation,
    })
}

fn legacy_permits(document: &XiaomuDocument, table: NodeId) -> bool {
    document
        .table_grid(table)
        .is_ok_and(|grid| !grid.has_spans())
}

fn validate_cell_presentation(attrs: &NodeAttrs) -> Result<(), TableLayoutError> {
    for (key, value) in attrs.iter() {
        match key {
            "colspan" | "rowspan" | "colwidth" | "backgroundColor" => {}
            "align" if matches!(value, AttrValue::Null) => {}
            "align" if matches!(value, AttrValue::String(value) if value == "left") => {}
            _ => return Err(TableLayoutError::InvalidDimension),
        }
    }
    cell_background(attrs)?;
    Ok(())
}

/// Missing/null use the host default; only fully parsed CSS Color 3 is admitted.
/// Unsupported colors remain in the document and refuse writable rendering.
pub(crate) fn cell_background(attrs: &NodeAttrs) -> Result<Option<Hsla>, TableLayoutError> {
    match attrs.get("backgroundColor") {
        None | Some(AttrValue::Null) => Ok(None),
        Some(AttrValue::String(value)) => crate::block_view::css_color(value)
            .map(Some)
            .ok_or(TableLayoutError::InvalidDimension),
        Some(_) => Err(TableLayoutError::InvalidDimension),
    }
}

/// Check both parked text endpoints and the effective CellRange endpoints.
pub(crate) fn selection_has_hidden_table_endpoint(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    capability: &TableCapability,
) -> bool {
    selection_has_hidden_table_endpoint_matching(document, selection, capability, false)
}

/// Proxies must first be built even while their table awaits measurement.
pub(crate) fn selection_has_hidden_table_endpoint_for_build(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    capability: &TableCapability,
) -> bool {
    selection_has_hidden_table_endpoint_matching(document, selection, capability, true)
}

fn selection_has_hidden_table_endpoint_matching(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    capability: &TableCapability,
    building: bool,
) -> bool {
    if capability.enabled() && !capability.selection_is_valid(document, selection) {
        return true;
    }
    let hidden = |node| {
        if building {
            capability.hidden_ancestor_for_build(document, node)
        } else {
            capability.hidden_ancestor(document, node)
        }
        .is_some()
    };
    if selection
        .active_cell_range()
        .is_some_and(|range| [range.anchor(), range.focus()].into_iter().any(hidden))
    {
        return true;
    }
    [selection.anchor(), selection.focus()]
        .into_iter()
        .any(|position| {
            let node = match position {
                DocumentPosition::Inline(point) => point.node_id(),
                DocumentPosition::Atomic(node) => node,
                DocumentPosition::Gap(gap) => gap.parent(),
            };
            hidden(node)
        })
}

/// Late native callbacks must check their own current identity and ancestry,
/// even when the current selection has already moved to a visible paragraph.
pub(crate) fn handler_is_hidden_by_table(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    node: NodeId,
    range_input: bool,
    capability: &TableCapability,
) -> bool {
    if (capability.enabled() && document.node(node).is_none())
        || selection_has_hidden_table_endpoint(document, selection, capability)
    {
        return true;
    }
    // Only this exact, currently selected whole-table proxy may edit a table
    // placeholder. Its enclosing tables still need their own admission.
    if range_input
        && selection.as_node_selection() == Some(node)
        && document
            .node(node)
            .is_some_and(|node| node.kind() == &NodeKind::Table)
    {
        let parent = if capability.enabled() {
            capability.parent_of(document, node)
        } else {
            document.parent_of(node)
        };
        return parent.is_some_and(|parent| capability.hidden_ancestor(document, parent).is_some());
    }
    capability.hidden_ancestor(document, node).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{
        handler_is_hidden_by_table as hidden,
        selection_has_hidden_table_endpoint as selection_hidden,
        selection_has_hidden_table_endpoint_for_build as build_hidden,
    };
    use xiaomu_core::document::{InlineContent, NodeContent, NodeStoreBuilder};
    use xiaomu_core::selection::NodeGap;
    use xiaomu_core::text::TextRange;
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

    struct Fixture {
        document: XiaomuDocument,
        table: NodeId,
        row: NodeId,
        cell: NodeId,
        text: NodeId,
        outside: NodeId,
        outer: NodeId,
    }

    fn attrs(key: &str, value: AttrValue) -> NodeAttrs {
        NodeAttrs::new([(key.to_owned(), value)].into()).unwrap()
    }

    fn fixture(span: i64) -> Fixture {
        let mut b = NodeStoreBuilder::new();
        let mut paragraph = || {
            b.insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::empty()),
            )
            .unwrap()
        };
        let text = paragraph();
        let outside = paragraph();
        let cell = b
            .insert(
                NodeKind::TableHeader,
                attrs("colspan", AttrValue::Integer(span)),
                NodeContent::children([text]),
            )
            .unwrap();
        let mut container = |kind, children: &[NodeId]| {
            b.insert(
                kind,
                NodeAttrs::empty(),
                NodeContent::children(children.iter().copied()),
            )
            .unwrap()
        };
        let row = container(NodeKind::TableRow, &[cell]);
        let table = container(NodeKind::Table, &[row]);
        let outer_cell = container(NodeKind::TableCell, &[table]);
        let outer_row = container(NodeKind::TableRow, &[outer_cell]);
        let outer = container(NodeKind::Table, &[outer_row]);
        let root = container(NodeKind::Document, &[outside, outer]);
        Fixture {
            document: XiaomuDocument::new(root, b.finish()).unwrap(),
            table,
            row,
            cell,
            text,
            outside,
            outer,
        }
    }

    fn apply(document: &XiaomuDocument, step: TransactionStep) -> XiaomuDocument {
        Transaction::new(TransactionOrigin::System)
            .with_step(step)
            .apply(document)
            .unwrap()
    }

    fn measure(capability: &mut TableCapability, document: &XiaomuDocument, table: NodeId) {
        capability.record(table, capability.key(document, table).unwrap(), true);
    }

    #[test]
    fn default_legacy_and_opt_in_require_distinct_build_and_input_gates() {
        let f = fixture(2);
        let mut cap = TableCapability::default();
        assert!(!cap.enabled());
        assert!(!cap.can_build(&f.document, f.table));
        assert!(cap.permits(&f.document, f.outer));
        assert_eq!(cap.hidden_ancestor(&f.document, f.text), Some(f.table));
        cap.set_enabled(true);
        assert!(cap.can_build(&f.document, f.table));
        assert!(!cap.permits(&f.document, f.table));
        assert_eq!(cap.hidden_ancestor_for_build(&f.document, f.text), None);
        measure(&mut cap, &f.document, f.table);
        assert_eq!(cap.hidden_ancestor(&f.document, f.text), Some(f.outer));
        measure(&mut cap, &f.document, f.outer);
        assert_eq!(cap.hidden_ancestor(&f.document, f.text), None);
        cap.record(f.outer, cap.key(&f.document, f.outer).unwrap(), false);
        assert_eq!(cap.hidden_ancestor(&f.document, f.text), Some(f.outer));
    }

    #[test]
    fn success_survives_typing_but_not_kind_or_attribute_changes() {
        let f = fixture(2);
        let mut cap = TableCapability::default();
        cap.set_enabled(true);
        measure(&mut cap, &f.document, f.table);
        let zero = InlineContent::empty().offset_at(0).unwrap();
        let typed = apply(
            &f.document,
            TransactionStep::ReplaceText {
                node: f.text,
                range: TextRange::new(zero, zero).unwrap(),
                replacement: "中文".into(),
            },
        );
        assert!(cap.permits(&typed, f.table));
        let body = apply(
            &typed,
            TransactionStep::SetNodeKind {
                node: f.cell,
                kind: NodeKind::TableCell,
            },
        );
        assert!(!cap.permits(&body, f.table));
        let colored = apply(
            &typed,
            TransactionStep::SetNodeAttrs {
                node: f.cell,
                attrs: NodeAttrs::new(
                    [
                        ("colspan".into(), AttrValue::Integer(2)),
                        ("backgroundColor".into(), AttrValue::String("red".into())),
                    ]
                    .into(),
                )
                .unwrap(),
            },
        );
        assert!(cap.can_build(&colored, f.table));
        assert!(!cap.permits(&colored, f.table));
    }

    #[test]
    fn shared_revocation_reaches_retained_handlers_without_affecting_another_editor() {
        let f = fixture(2);
        let shared = Rc::new(RefCell::new(TableCapability::default()));
        let retained = shared.clone();
        shared.borrow_mut().set_enabled(true);
        measure(&mut shared.borrow_mut(), &f.document, f.table);
        assert!(retained.borrow().permits(&f.document, f.table));
        let other = TableCapability::default();
        assert!(!other.permits(&f.document, f.table));
        shared.borrow_mut().set_enabled(false);
        assert!(!retained.borrow().enabled());
        shared.borrow_mut().set_enabled(true);
        assert!(!retained.borrow().permits(&f.document, f.table));
    }

    #[test]
    fn unsupported_presentation_preserves_raw_data_and_refuses_build() {
        let f = fixture(1);
        let mut cap = TableCapability::default();
        cap.set_enabled(true);
        for (node, key, value) in [
            (f.cell, "align", AttrValue::String("right".into())),
            (f.cell, "align", AttrValue::String("center".into())),
            (f.cell, "extension", AttrValue::Null),
            (f.row, "style", AttrValue::String("height: 9px".into())),
            (f.table, "class", AttrValue::String("wide".into())),
            (
                f.cell,
                "colwidth",
                AttrValue::List(vec![AttrValue::Integer(1_000_001)]),
            ),
        ] {
            let raw = attrs(key, value);
            let step = TransactionStep::SetNodeAttrs {
                node,
                attrs: raw.clone(),
            };
            let changed = apply(&f.document, step);
            assert!(!cap.can_build(&changed, f.table));
            assert_eq!(changed.node(node).unwrap().attrs(), &raw);
        }
        for value in [AttrValue::Null, AttrValue::String("left".into())] {
            let changed = apply(
                &f.document,
                TransactionStep::SetNodeAttrs {
                    node: f.cell,
                    attrs: attrs("align", value),
                },
            );
            assert!(cap.can_build(&changed, f.table));
        }
        let background = |value| cell_background(&attrs("backgroundColor", value));
        assert!(cell_background(&NodeAttrs::empty()).unwrap().is_none());
        assert!(background(AttrValue::Null).unwrap().is_none());
        for color in ["red", "#123456", "rgba(10,20,30,0.5)", "transparent"] {
            assert!(
                background(AttrValue::String(color.into()))
                    .unwrap()
                    .is_some()
            );
        }
        for color in ["currentColor", "red trailing", "var(--color)", ""] {
            assert!(background(AttrValue::String(color.into())).is_err());
        }
    }

    #[test]
    fn stale_handlers_cell_ranges_and_whole_table_proxies_check_current_ancestors() {
        let f = fixture(2);
        let outside = DocumentSelection::node(&f.document, f.outside).unwrap();
        let park = NodeGap::new(f.document.root(), 0).into();
        let range = DocumentSelection::cell_range(f.cell, f.cell, park);
        let mut cap = TableCapability::default();
        cap.set_enabled(true);
        assert!(selection_hidden(&f.document, range, &cap));
        assert!(!build_hidden(&f.document, range, &cap));
        assert!(hidden(&f.document, outside, f.text, false, &cap));
        let outer_selected = DocumentSelection::node(&f.document, f.outer).unwrap();
        assert!(!hidden(&f.document, outer_selected, f.outer, true, &cap));
        assert!(hidden(&f.document, outside, f.outer, true, &cap));
        assert!(hidden(&f.document, outer_selected, f.text, false, &cap));
        let inner_selected = DocumentSelection::node(&f.document, f.table).unwrap();
        assert!(hidden(&f.document, inner_selected, f.table, true, &cap));
        measure(&mut cap, &f.document, f.outer);
        assert!(!hidden(&f.document, inner_selected, f.table, true, &cap));
        let removed = apply(&f.document, TransactionStep::RemoveNode { node: f.outer });
        let outside = DocumentSelection::node(&removed, f.outside).unwrap();
        assert!(hidden(&removed, outside, f.text, false, &cap));
    }
}
