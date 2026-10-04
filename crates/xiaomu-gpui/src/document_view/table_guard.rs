//! Temporary view boundary until logical span geometry has a renderer.
//!
//! Canonical traversal and Runtime inline editing stay unchanged. Only this
//! renderer's editable surfaces and navigation omit placeholder descendants.
use std::collections::HashMap;

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

use super::{
    DocumentView,
    navigation::{self, NavUnit, TextBlock},
};

pub(super) fn rendered_nav_units(document: &XiaomuDocument) -> Vec<NavUnit> {
    fn visible(document: &XiaomuDocument, node: NodeId, cache: &mut HashMap<NodeId, bool>) -> bool {
        if let Some(visible) = cache.get(&node) {
            return *visible;
        }
        let result = !navigation::table_needs_placeholder(document, node)
            && document
                .parent_of(node)
                .is_none_or(|parent| visible(document, parent, cache));
        cache.insert(node, result);
        result
    }
    // Keep canonical ordering, caching ancestry so each table grid is checked
    // once rather than rebuilding its occupancy for every descendant block.
    let mut visibility = HashMap::new();
    let mut units = navigation::nav_units(document);
    units.retain(|unit| {
        visible(
            document,
            match unit {
                NavUnit::Text(block) => block.node,
                NavUnit::Atomic(node) => *node,
            },
            &mut visibility,
        )
    });
    units
}

pub(super) fn rendered_text_blocks(document: &XiaomuDocument) -> Vec<TextBlock> {
    rendered_nav_units(document)
        .into_iter()
        .filter_map(|unit| match unit {
            NavUnit::Text(block) => Some(block),
            NavUnit::Atomic(_) => None,
        })
        .collect()
}

/// Shared only by this view and its native input handlers. Runtime remains
/// able to edit canonical text inside a legal spanning table without GPUI.
pub(crate) fn selection_has_hidden_table_endpoint(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> bool {
    if selection.active_cell_range().is_some_and(|range| {
        [range.anchor(), range.focus()]
            .into_iter()
            .any(|cell| navigation::spanning_table_ancestor(document, cell).is_some())
    }) {
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
            navigation::spanning_table_ancestor(document, node).is_some()
        })
}

impl DocumentView {
    /// A host-restored endpoint can still refer to canonical hidden content.
    /// Consume legacy gestures there without giving an unpainted editor focus.
    pub(super) fn selection_has_hidden_table_endpoint(&self) -> bool {
        let session = self.session.borrow();
        selection_has_hidden_table_endpoint(session.document(), session.selection())
    }
}
