//! Cross-block caret navigation translation.
//!
//! Pure logic over Core types only: given the document's inline-bearing
//! blocks in document order and the current focus, compute the target
//! position for a navigation key. The GPUI layer converts the result into a
//! [`xiaomu_core::selection::TextPoint`] and a runtime
//! [`xiaomu_runtime::session::EditIntent::SetSelection`].
//!
//! Blocks are single visual lines in this slice (no soft wrapping yet), so
//! Up/Down move to the adjacent block and clamp the horizontal byte index;
//! x-preserving vertical movement needs shaped-line geometry and lands with
//! real layout in the harness gate.

use xiaomu_core::document::{InlineContent, NodeContent, NodeId, XiaomuDocument};
use xiaomu_core::text::TextOffset;

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

/// Where the focused block sits relative to list structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ListContext {
    /// The item has a previous sibling item (so it can indent).
    pub has_previous_item: bool,
    /// The item's list is itself inside another list item (so it can
    /// outdent).
    pub nested: bool,
}

/// Classifies `node`'s position when it is a block directly inside a list
/// item; `None` when the node is not in a list at all.
///
/// Tab / Shift-Tab translate on top of this: a block outside any list
/// turns into a bullet on Tab, and a top-level item Shift-Tab lifts back
/// out to a plain paragraph.
#[must_use]
pub(crate) fn list_context(document: &XiaomuDocument, node: NodeId) -> Option<ListContext> {
    let parent = document.parent_of(node)?;
    if document.node(parent)?.kind() != &xiaomu_core::document::NodeKind::ListItem {
        return None;
    }
    let list = document.parent_of(parent)?;
    if !matches!(
        document.node(list)?.kind(),
        xiaomu_core::document::NodeKind::BulletList | xiaomu_core::document::NodeKind::OrderedList
    ) {
        return None;
    }
    let list_parent = document.parent_of(list)?;
    let nested = document.node(list_parent)?.kind() == &xiaomu_core::document::NodeKind::ListItem;
    let siblings = document.node(list)?.content().as_children()?;
    let index = siblings.iter().position(|child| child == &parent)?;
    Some(ListContext {
        has_previous_item: index > 0,
        nested,
    })
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

/// One horizontal navigation step over the whole block sequence.
///
/// Left at a block start wraps to the previous block's end; Right at a
/// block end wraps to the next block's start. Returns `(block, raw byte)`
/// or `None` at the document edges.
#[must_use]
pub(crate) fn step_horizontal(
    blocks: &[TextBlock],
    block: usize,
    offset: usize,
    forward: bool,
) -> Option<(usize, usize)> {
    let text_of = |index: usize| blocks[index].text();

    if forward {
        if let Some(next) = next_boundary(&text_of(block), offset) {
            return Some((block, next));
        }
        let following = block + 1;
        (following < blocks.len()).then_some((following, 0))
    } else {
        if let Some(previous) = previous_boundary(&text_of(block), offset) {
            return Some((block, previous));
        }
        block
            .checked_sub(1)
            .map(|prior| (prior, text_of(prior).len()))
    }
}

/// Start/end of one block as `(block, raw byte)` targets.
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

/// One vertical navigation step.
///
/// With one visual line per block this moves to the adjacent block,
/// clamping the byte offset into its length and snapping onto a UTF-8
/// scalar boundary of the target; returns `None` at the first or last
/// block.
#[must_use]
pub(crate) fn step_vertical(
    blocks: &[TextBlock],
    block: usize,
    offset: usize,
    down: bool,
) -> Option<(usize, usize)> {
    let target = if down {
        block + 1
    } else {
        block.checked_sub(1)?
    };
    let text = blocks.get(target)?.text();
    Some((target, snap_to_scalar_boundary(&text, offset)))
}

/// Clamps `raw` into `text` and floors to the start of the containing
/// Unicode scalar when the candidate lands inside a multi-byte character.
fn snap_to_scalar_boundary(text: &str, raw: usize) -> usize {
    let clamped = raw.min(text.len());
    if text.is_char_boundary(clamped) {
        return clamped;
    }
    let mut index = clamped;
    while index > 0 {
        index -= 1;
        if text.is_char_boundary(index) {
            return index;
        }
    }
    0
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
        let blocks = text_blocks(&document);

        // Right from the end of block 0 wraps to the start of block 1.
        assert_eq!(step_horizontal(&blocks, 0, 3, true), Some((1, 0)));
        // Right over "二" (3 bytes) lands on the emoji boundary.
        assert_eq!(step_horizontal(&blocks, 1, 0, true), Some((1, 3)));
        assert_eq!(step_horizontal(&blocks, 1, 3, true), Some((1, 7)));
        // Left at a block start wraps to the previous block's end.
        assert_eq!(step_horizontal(&blocks, 1, 0, false), Some((0, 3)));

        // Document edges return None.
        assert_eq!(step_horizontal(&blocks, 0, 0, false), None);
        assert_eq!(step_horizontal(&blocks, 2, 4, true), None);
    }

    #[test]
    fn line_edges_reach_both_ends() {
        let document = sample_document();
        let blocks = text_blocks(&document);

        assert_eq!(line_edge(&blocks, 1, false), Some((1, 0)));
        // "二👍三" spans ten UTF-8 bytes.
        assert_eq!(line_edge(&blocks, 1, true), Some((1, 10)));
        assert_eq!(line_edge(&blocks, 9, true), None);
    }

    #[test]
    fn vertical_steps_clamp_into_adjacent_block_length() {
        let document = sample_document();
        let blocks = text_blocks(&document);

        // Offset 2 in "one" lands inside "二" (3-byte UTF-8) and floors to 0.
        assert_eq!(step_vertical(&blocks, 0, 2, true), Some((1, 0)));
        assert!(validated_offset(&blocks[1], 0).is_some());
        // Offset 6 in "二👍三" (10 bytes) clamps into "deep" (4 bytes).
        assert_eq!(step_vertical(&blocks, 1, 6, true), Some((2, 4)));
        // Up keeps the offset when it fits on a scalar boundary.
        assert_eq!(step_vertical(&blocks, 1, 1, false), Some((0, 1)));

        // First block cannot go up; last cannot go down.
        assert_eq!(step_vertical(&blocks, 0, 0, false), None);
        assert_eq!(step_vertical(&blocks, 2, 0, true), None);
    }

    #[test]
    fn vertical_steps_always_land_on_a_valid_scalar_boundary() {
        let document = sample_document();
        let blocks = text_blocks(&document);
        let target = &blocks[1];
        // "二👍三" scalar starts: 0, 3, 7, 10.
        for offset in 0..=blocks[0].text().len() + 2 {
            let Some((_, raw)) = step_vertical(&blocks, 0, offset, true) else {
                continue;
            };
            assert!(
                validated_offset(target, raw).is_some(),
                "offset {offset} snapped to illegal {raw}"
            );
        }
    }

    #[test]
    fn empty_blocks_participate_in_wrap_around() {
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
        let blocks = text_blocks(&document);

        // Left at the start of the empty block reaches the end of nothing —
        // it is already both edges; Right wraps into the next block.
        assert_eq!(step_horizontal(&blocks, 0, 0, true), Some((1, 0)));
        assert_eq!(step_horizontal(&blocks, 1, 2, false), Some((1, 1)));
        let _ = empty;
    }

    #[test]
    fn list_context_classifies_items_and_outsiders() {
        let document = sample_document();

        // "one" and the quote's paragraph are not in any list.
        let blocks = text_blocks(&document);
        assert_eq!(list_context(&document, blocks[0].node), None);
        assert_eq!(list_context(&document, blocks[2].node), None);

        // Build Document > ul > [li > p(a), li > p(b)] to cover both items.
        let mut builder = NodeStoreBuilder::new();
        let a = paragraph_for_test("a", &mut builder);
        let b = paragraph_for_test("b", &mut builder);
        let item_a = builder
            .insert(
                xiaomu_core::document::NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([a]),
            )
            .unwrap();
        let item_b = builder
            .insert(
                xiaomu_core::document::NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([b]),
            )
            .unwrap();
        let ul = builder
            .insert(
                xiaomu_core::document::NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([item_a, item_b]),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([ul]),
            )
            .unwrap();
        let doc2 = XiaomuDocument::new(root, builder.finish()).unwrap();

        assert_eq!(
            list_context(&doc2, a),
            Some(ListContext {
                has_previous_item: false,
                nested: false
            })
        );
        assert_eq!(
            list_context(&doc2, b),
            Some(ListContext {
                has_previous_item: true,
                nested: false
            })
        );
    }

    #[test]
    fn list_context_detects_nesting() {
        let mut builder = NodeStoreBuilder::new();
        let outer_p = paragraph_for_test("outer", &mut builder);
        let inner_p = paragraph_for_test("inner", &mut builder);
        let inner_item = builder
            .insert(
                xiaomu_core::document::NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([inner_p]),
            )
            .unwrap();
        let inner_list = builder
            .insert(
                xiaomu_core::document::NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([inner_item]),
            )
            .unwrap();
        let outer_item = builder
            .insert(
                xiaomu_core::document::NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([outer_p, inner_list]),
            )
            .unwrap();
        let outer_list = builder
            .insert(
                xiaomu_core::document::NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([outer_item]),
            )
            .unwrap();
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([outer_list]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        // The outer item is top-level; the inner item sits in a nested list
        // whose previous-sibling count within that inner list is zero.
        assert_eq!(
            list_context(&document, outer_p),
            Some(ListContext {
                has_previous_item: false,
                nested: false
            })
        );
        assert_eq!(
            list_context(&document, inner_p),
            Some(ListContext {
                has_previous_item: false,
                nested: true
            })
        );
    }

    fn paragraph_for_test(text: &str, builder: &mut NodeStoreBuilder) -> NodeId {
        builder
            .insert(
                xiaomu_core::document::NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
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
