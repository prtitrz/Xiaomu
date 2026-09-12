//! Document-order navigation helpers shared by the GPUI frontend.
//!
//! This module owns layout-independent traversal only: collecting inline
//! blocks, Unicode-scalar horizontal stepping and validating raw byte targets.
//! Visual Up/Down and Home/End live in `visual_navigation`, where the current
//! wrapped GPUI layout is available.

use xiaomu_core::document::{InlineContent, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::session::{CellRange, DocumentPosition};

/// One inline-bearing block in document order.
#[derive(Clone, Debug)]
pub(crate) struct TextBlock {
    /// The inline-bearing node itself.
    pub node: NodeId,
    /// Its canonical inline content.
    pub inline: InlineContent,
}

impl TextBlock {
    /// The canonical concatenated text of the block.
    pub(crate) fn text(&self) -> String {
        self.inline
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect()
    }
}

/// Collects every inline-bearing block, depth-first from the root.
///
/// This is the render order of the multi-block view and the traversal order
/// of cross-block navigation.
#[must_use]
pub(crate) fn text_blocks(document: &XiaomuDocument) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    collect_inline(document, document.root(), &mut blocks);
    blocks
}

fn collect_inline(document: &XiaomuDocument, id: NodeId, blocks: &mut Vec<TextBlock>) {
    let Some(node) = document.node(id) else {
        return;
    };
    match node.content() {
        NodeContent::Inline(inline) => blocks.push(TextBlock {
            node: id,
            inline: inline.clone(),
        }),
        NodeContent::Children(children) => {
            for child in children {
                collect_inline(document, *child, blocks);
            }
        }
        NodeContent::Atomic | _ => {}
    }
}

/// Index of `node` in `blocks`, if it is an inline-bearing block.
#[must_use]
pub(crate) fn block_index(blocks: &[TextBlock], node: NodeId) -> Option<usize> {
    blocks.iter().position(|block| block.node == node)
}

/// The innermost table cell containing `node`, if any.
#[must_use]
pub(crate) fn table_cell_ancestor(document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(id) = current {
        if matches!(document.node(id)?.kind(), NodeKind::TableCell) {
            return Some(id);
        }
        current = document.parent_of(id);
    }
    None
}

/// Whether the focused position lives inside `target`'s subtree.
///
/// Gap focuses are addressed by their parent container, so the container
/// chain decides containment for them too.
#[must_use]
pub(crate) fn selection_is_within(
    document: &XiaomuDocument,
    focus: DocumentPosition,
    target: NodeId,
) -> bool {
    let start = match focus {
        DocumentPosition::Inline(point) => point.node_id(),
        DocumentPosition::Atomic(node) => node,
        DocumentPosition::Gap(gap) => gap.parent(),
    };
    let mut current = Some(start);
    while let Some(id) = current {
        if id == target {
            return true;
        }
        current = document.parent_of(id);
    }
    false
}

/// The cell ids of an active cell range's rectangle, row-major, or `None`
/// when the endpoints do not resolve inside one table.
#[must_use]
pub(crate) fn cell_range_rect(document: &XiaomuDocument, range: CellRange) -> Option<Vec<NodeId>> {
    let locate = |cell: NodeId| -> Option<(NodeId, usize, usize)> {
        let row = document.parent_of(cell)?;
        let table = document.parent_of(row)?;
        let children = |id: NodeId| {
            document
                .node(id)
                .and_then(|node| node.content().as_children().map(<[NodeId]>::to_vec))
        };
        let row_index = children(table)?
            .iter()
            .position(|candidate| *candidate == row)?;
        let col_index = children(row)?
            .iter()
            .position(|candidate| *candidate == cell)?;
        Some((table, row_index, col_index))
    };
    let (table, anchor_row, anchor_col) = locate(range.anchor())?;
    let (_, focus_row, focus_col) = locate(range.focus())?;
    let (row_min, row_max) = (anchor_row.min(focus_row), anchor_row.max(focus_row));
    let (col_min, col_max) = (anchor_col.min(focus_col), anchor_col.max(focus_col));

    let children = |id: NodeId| {
        document
            .node(id)
            .and_then(|node| node.content().as_children().map(<[NodeId]>::to_vec))
    };
    let table_rows = children(table)?;
    let mut rect = Vec::new();
    for row_index in row_min..=row_max {
        let row_cells = children(*table_rows.get(row_index)?)?;
        for col_index in col_min..=col_max {
            rect.push(*row_cells.get(col_index)?);
        }
    }
    Some(rect)
}

/// One navigation unit in document order: an editable text block or an
/// atomic block addressed as a whole node (P4.6).
#[derive(Clone, Debug)]
pub(crate) enum NavUnit {
    Text(TextBlock),
    Atomic(NodeId),
}

/// Collects the navigation units of a document, depth-first from the root.
///
/// This extends [`text_blocks`] with atomic blocks, which participate in
/// horizontal traversal as whole-node selections.
pub(crate) fn nav_units(document: &XiaomuDocument) -> Vec<NavUnit> {
    let mut units = Vec::new();
    collect_units(document, document.root(), &mut units);
    units
}

fn collect_units(document: &XiaomuDocument, id: NodeId, units: &mut Vec<NavUnit>) {
    let Some(node) = document.node(id) else {
        return;
    };
    match node.content() {
        NodeContent::Inline(inline) => units.push(NavUnit::Text(TextBlock {
            node: id,
            inline: inline.clone(),
        })),
        NodeContent::Atomic => units.push(NavUnit::Atomic(id)),
        NodeContent::Children(children) => {
            for child in children {
                collect_units(document, *child, units);
            }
        }
        _ => {}
    }
}

/// Index of `node` in `units`, whether it is a text block or an atomic one.
pub(crate) fn unit_index(units: &[NavUnit], node: NodeId) -> Option<usize> {
    units.iter().position(|unit| match unit {
        NavUnit::Text(block) => block.node == node,
        NavUnit::Atomic(id) => *id == node,
    })
}

/// Where one horizontal navigation step lands: inside a text block at a raw
/// byte offset, or on a whole atomic block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HorizontalTarget {
    InText(usize, usize),
    OnAtomic(usize),
}

/// One horizontal navigation step over the unit sequence.
///
/// Left at a block start wraps to the previous unit's end (a text block's
/// last byte or an atomic block selection); Right at a block end wraps to
/// the next unit. Returns `None` at the document edges. Soft-wrap affinity
/// is handled by the visual navigation controller before this logical step
/// runs.
#[must_use]
pub(crate) fn step_horizontal(
    units: &[NavUnit],
    unit: usize,
    offset: usize,
    forward: bool,
) -> Option<HorizontalTarget> {
    let text_of = |index: usize| match &units[index] {
        NavUnit::Text(block) => block.text(),
        NavUnit::Atomic(_) => String::new(),
    };

    if forward {
        if let Some(next) = next_boundary(&text_of(unit), offset) {
            return Some(HorizontalTarget::InText(unit, next));
        }
        let following = unit + 1;
        match units.get(following)? {
            NavUnit::Text(_) => Some(HorizontalTarget::InText(following, 0)),
            NavUnit::Atomic(_) => Some(HorizontalTarget::OnAtomic(following)),
        }
    } else {
        if let Some(previous) = previous_boundary(&text_of(unit), offset) {
            return Some(HorizontalTarget::InText(unit, previous));
        }
        let prior = unit.checked_sub(1)?;
        match &units[prior] {
            NavUnit::Text(block) => Some(HorizontalTarget::InText(prior, text_of_last_byte(block))),
            NavUnit::Atomic(_) => Some(HorizontalTarget::OnAtomic(prior)),
        }
    }
}

/// Last byte offset of one text block's canonical text.
fn text_of_last_byte(block: &TextBlock) -> usize {
    block.text().len()
}

/// Previous Unicode scalar boundary in `text`, or `None` at the start.
pub(crate) fn previous_boundary(text: &str, offset: usize) -> Option<usize> {
    text[..offset]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

/// Next Unicode scalar boundary in `text`, or `None` at the end.
pub(crate) fn next_boundary(text: &str, offset: usize) -> Option<usize> {
    text[offset..].chars().next().map(|c| offset + c.len_utf8())
}

/// Start/end of one logical block as `(block, raw byte)` targets.
///
/// This is the layout-unavailable fallback used before a block has painted;
/// the normal P3 path resolves Home/End against the current visual row.
#[must_use]
pub(crate) fn line_edge(
    blocks: &[TextBlock],
    block: usize,
    to_end: bool,
) -> Option<(usize, usize)> {
    blocks.get(block).map(|b| match to_end {
        true => (block, b.text().len()),
        false => (block, 0),
    })
}

/// Converts a raw byte index into a validated [`TextOffset`].
#[must_use]
pub(crate) fn validated_offset(block: &TextBlock, raw: usize) -> Option<TextOffset> {
    block.inline.offset_at(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{MarkSet, TextRun};
    use xiaomu_core::document::{NodeAttrs, NodeStoreBuilder};
    use xiaomu_core::selection::InlinePoint;

    /// Document > [p("one"), p("二👍三"), quote > p("deep")].
    fn sample_document() -> XiaomuDocument {
        fn paragraph(text: &str, builder: &mut NodeStoreBuilder) -> NodeId {
            builder
                .insert(
                    xiaomu_core::document::NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        }

        let mut builder = NodeStoreBuilder::new();
        let one = paragraph("one", &mut builder);
        let unicode = paragraph("二👍三", &mut builder);
        let deep = paragraph("deep", &mut builder);
        let quote = builder
            .insert(
                xiaomu_core::document::NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([deep]),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([one, unicode, quote]),
            )
            .unwrap();
        XiaomuDocument::new(root, builder.finish()).unwrap()
    }

    #[test]
    fn text_blocks_walk_the_tree_in_document_order() {
        let document = sample_document();
        let blocks = text_blocks(&document);

        assert_eq!(blocks.len(), 3);
        let texts: Vec<String> = blocks.iter().map(TextBlock::text).collect();
        assert_eq!(texts, ["one", "二👍三", "deep"]);
    }

    #[test]
    fn block_index_finds_nested_blocks() {
        let document = sample_document();
        let blocks = text_blocks(&document);
        let nested: Vec<NodeId> = {
            let mut ids = Vec::new();
            collect_inline_ids(&document, document.root(), &mut ids);
            ids
        };
        assert_eq!(nested.len(), 3);
        assert!(block_index(&blocks, nested[2]).is_some());
    }

    #[test]
    fn table_cell_ancestor_walks_to_the_innermost_cell() {
        fn paragraph(text: &str, builder: &mut NodeStoreBuilder) -> NodeId {
            builder
                .insert(
                    xiaomu_core::document::NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        }

        let mut builder = NodeStoreBuilder::new();
        let plain = paragraph("plain", &mut builder);
        let in_cell = paragraph("cell", &mut builder);
        let cell = builder
            .insert(
                xiaomu_core::document::NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([in_cell]),
            )
            .unwrap();
        let row = builder
            .insert(
                xiaomu_core::document::NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([cell]),
            )
            .unwrap();
        let table = builder
            .insert(
                xiaomu_core::document::NodeKind::Table,
                NodeAttrs::empty(),
                NodeContent::children([row]),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([plain, table]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(table_cell_ancestor(&document, in_cell), Some(cell));
        assert_eq!(table_cell_ancestor(&document, cell), Some(cell));
        assert_eq!(table_cell_ancestor(&document, table), None);
        assert_eq!(table_cell_ancestor(&document, plain), None);

        // The caret inside the cell paragraph reports containment for the
        // table; the caret outside does not.
        let focus = DocumentPosition::Inline(InlinePoint::at_start_of(in_cell));
        assert!(selection_is_within(&document, focus, table));
        assert!(selection_is_within(&document, focus, cell));
        let outside = DocumentPosition::Inline(InlinePoint::at_start_of(plain));
        assert!(!selection_is_within(&document, outside, table));
    }

    fn collect_inline_ids(document: &XiaomuDocument, id: NodeId, out: &mut Vec<NodeId>) {
        let Some(node) = document.node(id) else {
            return;
        };
        match node.content() {
            NodeContent::Inline(_) => out.push(id),
            NodeContent::Children(children) => {
                for child in children {
                    collect_inline_ids(document, *child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn horizontal_steps_cross_block_boundaries_by_scalar() {
        let document = sample_document();
        let units = nav_units(&document);

        // Right from the end of block 0 wraps to the start of block 1.
        assert_eq!(
            step_horizontal(&units, 0, 3, true),
            Some(HorizontalTarget::InText(1, 0))
        );
        // Right over "二" (3 bytes) lands on the emoji boundary.
        assert_eq!(
            step_horizontal(&units, 1, 0, true),
            Some(HorizontalTarget::InText(1, 3))
        );
        assert_eq!(
            step_horizontal(&units, 1, 3, true),
            Some(HorizontalTarget::InText(1, 7))
        );
        // Left at a block start wraps to the previous block's end.
        assert_eq!(
            step_horizontal(&units, 1, 0, false),
            Some(HorizontalTarget::InText(0, 3))
        );

        // Document edges return None.
        assert_eq!(step_horizontal(&units, 0, 0, false), None);
        assert_eq!(step_horizontal(&units, 2, 4, true), None);
    }

    #[test]
    fn hard_newline_has_distinct_before_and_after_caret_boundaries() {
        let mut builder = NodeStoreBuilder::new();
        let code = builder
            .insert(
                xiaomu_core::document::NodeKind::CodeBlock,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("a\nb", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([code]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let blocks = text_blocks(&document);
        let units = nav_units(&document);

        assert_eq!(blocks[0].text(), "a\nb");
        assert!(validated_offset(&blocks[0], 1).is_some());
        assert!(validated_offset(&blocks[0], 2).is_some());
        assert_eq!(
            step_horizontal(&units, 0, 1, true),
            Some(HorizontalTarget::InText(0, 2))
        );
        assert_eq!(
            step_horizontal(&units, 0, 2, false),
            Some(HorizontalTarget::InText(0, 1))
        );
    }

    #[test]
    fn line_edges_reach_both_ends_as_layout_fallback() {
        let document = sample_document();
        let blocks = text_blocks(&document);

        assert_eq!(line_edge(&blocks, 1, false), Some((1, 0)));
        // "二👍三" spans ten UTF-8 bytes.
        assert_eq!(line_edge(&blocks, 1, true), Some((1, 10)));
        assert_eq!(line_edge(&blocks, 9, true), None);
    }

    #[test]
    fn empty_blocks_participate_in_horizontal_wrap_around() {
        let mut builder = NodeStoreBuilder::new();
        let empty = builder
            .insert(
                xiaomu_core::document::NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::empty()),
            )
            .unwrap();
        let full = builder
            .insert(
                xiaomu_core::document::NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("ab", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([empty, full]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let units = nav_units(&document);

        assert_eq!(
            step_horizontal(&units, 0, 0, true),
            Some(HorizontalTarget::InText(1, 0))
        );
        assert_eq!(
            step_horizontal(&units, 1, 2, false),
            Some(HorizontalTarget::InText(1, 1))
        );
        let _ = empty;
    }

    #[test]
    fn validated_offset_rejects_mid_scalar_targets() {
        let document = sample_document();
        let blocks = text_blocks(&document);

        assert!(validated_offset(&blocks[1], 0).is_some());
        // Inside the emoji scalar.
        assert!(validated_offset(&blocks[1], 4).is_none());
    }
}
