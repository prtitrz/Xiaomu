//! Projection from a document selection into a detached clipboard fragment.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, TextRun, XiaomuDocument,
};

use super::fragment::{
    ClipboardAtom, ClipboardInline, ClipboardNode, ClipboardNodeContent, ClipboardSlice,
    project_roots,
};
use crate::session::{DocumentPosition, DocumentSelection, SessionError};

/// Projects a validated document selection into a detached clipboard slice.
///
/// A single selected inline block stays an inline fragment even when it lives
/// under a list or quote. Once a selection spans multiple inline leaves, the
/// minimal selected container tree is retained so Xiaomu-native copy/paste can
/// preserve list/quote structure without dragging unrelated siblings along.
pub(crate) fn slice_selection(
    document: &XiaomuDocument,
    selection: DocumentSelection,
) -> Result<Option<ClipboardSlice>, SessionError> {
    selection.validate(document)?;

    let (head, tail) = selection.ordered(document)?;
    let (DocumentPosition::Inline(head), DocumentPosition::Inline(tail)) = (head, tail) else {
        return Err(SessionError::SelectionInvalid);
    };

    // Identical endpoints (node, boundary, and atom ordinal) select nothing;
    // affinity is visual bookkeeping and never selects canonical content.
    // Two gaps at one text boundary can still select the atoms between them.
    if head.node_id() == tail.node_id()
        && head.text_offset() == tail.text_offset()
        && head.atom_index() == tail.atom_index()
    {
        return Ok(None);
    }

    let mut source_blocks = Vec::new();
    collect_inline_blocks(document, document.root(), &mut source_blocks);

    let head_index = source_blocks
        .iter()
        .position(|block| block.node == head.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let tail_index = source_blocks
        .iter()
        .position(|block| block.node == tail.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    if head_index > tail_index {
        return Err(SessionError::SelectionInvalid);
    }

    if head_index == tail_index {
        let source = &source_blocks[head_index];
        let inline = slice_inline(
            document,
            &source.inline,
            head.text_offset().as_usize(),
            head.atom_index(),
            tail.text_offset().as_usize(),
            tail.atom_index(),
            false,
        )?;
        return Ok(Some(ClipboardSlice::from_roots(vec![ClipboardNode::new(
            source.kind.clone(),
            source.attrs.clone(),
            ClipboardNodeContent::Inline(inline),
        )])));
    }

    // Cross-block selections own every atom of the blocks they span; the
    // boundary blocks clip by ordinal exactly like the editing contract.
    let mut selected = BTreeMap::new();
    for (index, source) in source_blocks[head_index..=tail_index].iter().enumerate() {
        let absolute_index = head_index + index;
        let (start_raw, start_ordinal, end_raw, end_ordinal, include_end_atoms) =
            if absolute_index == head_index {
                (
                    head.text_offset().as_usize(),
                    head.atom_index(),
                    source.inline.len_bytes(),
                    0,
                    true,
                )
            } else if absolute_index == tail_index {
                (
                    0,
                    0,
                    tail.text_offset().as_usize(),
                    tail.atom_index(),
                    false,
                )
            } else {
                (0, 0, source.inline.len_bytes(), 0, true)
            };
        selected.insert(
            source.node,
            slice_inline(
                document,
                &source.inline,
                start_raw,
                start_ordinal,
                end_raw,
                end_ordinal,
                include_end_atoms,
            )?,
        );
    }

    let roots = project_roots(document, &selected);
    if roots.is_empty() {
        return Err(SessionError::SelectionInvalid);
    }
    Ok(Some(ClipboardSlice::from_roots(roots)))
}

struct SourceBlock {
    node: NodeId,
    kind: NodeKind,
    attrs: NodeAttrs,
    inline: InlineContent,
}

fn collect_inline_blocks(document: &XiaomuDocument, id: NodeId, out: &mut Vec<SourceBlock>) {
    let Some(node) = document.node(id) else {
        return;
    };
    match node.content() {
        NodeContent::Inline(inline) => out.push(SourceBlock {
            node: id,
            kind: node.kind().clone(),
            attrs: node.attrs().clone(),
            inline: inline.clone(),
        }),
        NodeContent::Children(children) => {
            for child in children {
                collect_inline_blocks(document, *child, out);
            }
        }
        NodeContent::Atomic | _ => {}
    }
}

fn slice_inline(
    document: &XiaomuDocument,
    inline: &InlineContent,
    start_raw: usize,
    start_ordinal: usize,
    end_raw: usize,
    end_ordinal: usize,
    include_end_atoms: bool,
) -> Result<ClipboardInline, SessionError> {
    inline.offset_at(start_raw).map_err(SessionError::Core)?;
    inline.offset_at(end_raw).map_err(SessionError::Core)?;
    if start_raw > end_raw {
        return Err(SessionError::SelectionInvalid);
    }

    let mut pieces = Vec::new();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;

        let overlap_start = start_raw.max(run_start);
        let overlap_end = end_raw.min(run_end);
        if overlap_start >= overlap_end {
            continue;
        }

        let text = &run.text().as_str()[overlap_start - run_start..overlap_end - run_start];
        pieces.push(TextRun::new(text, run.marks().clone()).map_err(SessionError::Core)?);
    }

    // Detach the atoms inside the span, re-anchored to the slice start. The
    // same-boundary rule mirrors the editing contract: atoms at or after the
    // start gap and atoms before the end gap belong to the selection.
    let text_view = InlineContent::new(pieces.iter().cloned()).map_err(SessionError::Core)?;
    let mut atoms = Vec::new();
    for placement in inline.atoms() {
        let offset = placement.text_offset().as_usize();
        let ordinal = same_boundary_ordinal(inline, placement.text_offset(), placement.atom());
        let inside = if start_raw == end_raw {
            offset == start_raw && ordinal >= start_ordinal && ordinal < end_ordinal
        } else {
            offset == start_raw && ordinal >= start_ordinal
                || offset > start_raw
                    && (offset < end_raw
                        || (offset == end_raw && (include_end_atoms || ordinal < end_ordinal)))
        };
        if !inside {
            continue;
        }
        let payload = document
            .node(placement.atom())
            .ok_or(SessionError::SelectionInvalid)?;
        let content = payload
            .content()
            .as_inline_atom()
            .ok_or(SessionError::SelectionInvalid)?;
        atoms.push(ClipboardAtom::new(
            text_view
                .offset_at(offset - start_raw)
                .map_err(SessionError::Core)?,
            atom_kind_of(payload)?,
            payload.attrs().clone(),
            content.clone(),
        ));
    }

    ClipboardInline::new(pieces, atoms).map_err(SessionError::Core)
}

fn atom_kind_of(
    node: &xiaomu_core::document::Node,
) -> Result<xiaomu_core::document::AtomKind, SessionError> {
    match node.kind() {
        NodeKind::InlineAtom(kind) => Ok(kind.clone()),
        _ => Err(SessionError::SelectionInvalid),
    }
}

fn same_boundary_ordinal(
    inline: &InlineContent,
    offset: xiaomu_core::text::TextOffset,
    atom: NodeId,
) -> usize {
    inline
        .atoms()
        .iter()
        .take_while(|placement| placement.text_offset() <= offset)
        .filter(|placement| placement.text_offset() == offset)
        .take_while(|placement| placement.atom() != atom)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::{Mark, MarkKind, MarkSet, NodeStoreBuilder};
    use xiaomu_core::selection::{CursorAffinity, TextPoint};

    struct Fixture {
        document: XiaomuDocument,
        first: NodeId,
        second: NodeId,
        list_first: NodeId,
        list_second: NodeId,
    }

    fn fixture() -> Fixture {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("ab", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                        TextRun::new("中", MarkSet::empty()).unwrap(),
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::Heading(xiaomu_core::document::HeadingLevel::new(2).unwrap()),
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("cd", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let list_first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("甲乙", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let first_item = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([list_first]),
            )
            .unwrap();
        let list_second = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("尾", MarkSet::new([Mark::Italic]).unwrap()).unwrap(),
                        TextRun::new("巴", MarkSet::empty()).unwrap(),
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let second_item = builder
            .insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([list_second]),
            )
            .unwrap();
        let list = builder
            .insert(
                NodeKind::BulletList,
                NodeAttrs::empty(),
                NodeContent::children([first_item, second_item]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first, second, list]),
            )
            .unwrap();
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            first,
            second,
            list_first,
            list_second,
        }
    }

    fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
        let inline = document.node(node).unwrap().content().as_inline().unwrap();
        TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
    }

    fn text(inline: &ClipboardInline) -> String {
        inline.text()
    }

    #[test]
    fn cross_block_slice_keeps_partial_runs_marks_and_plain_fallback() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.first, 1),
            point(&fixture.document, fixture.second, 1),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();

        assert_eq!(slice.plain_text(), "b中\nc");
        assert_eq!(slice.blocks().len(), 2);
        assert!(matches!(slice.blocks()[0].kind(), NodeKind::Paragraph));
        assert!(matches!(slice.blocks()[1].kind(), NodeKind::Heading(_)));
        let first = slice.blocks()[0].inline();
        assert_eq!(text(first), "b中");
        assert_eq!(first.runs().len(), 2);
        assert!(first.runs()[0].marks().contains(MarkKind::Bold));
        assert!(first.runs()[1].marks().is_empty());
    }

    #[test]
    fn single_leaf_inside_list_does_not_capture_list_ancestors() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.list_first, 0),
            point(&fixture.document, fixture.list_first, 3),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();

        assert_eq!(slice.plain_text(), "甲");
        assert_eq!(slice.roots().len(), 1);
        assert!(matches!(slice.roots()[0].kind(), NodeKind::Paragraph));
        assert!(slice.roots()[0].content().as_inline().is_some());
    }

    #[test]
    fn multi_leaf_list_selection_retains_minimal_list_tree() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.list_first, 3),
            point(&fixture.document, fixture.list_second, 3),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();

        assert_eq!(slice.plain_text(), "乙\n尾");
        assert_eq!(slice.roots().len(), 1);
        let list = &slice.roots()[0];
        assert!(matches!(list.kind(), NodeKind::BulletList));
        let items = list.content().as_children().unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|item| matches!(item.kind(), NodeKind::ListItem))
        );
        assert_eq!(
            text(
                items[0].content().as_children().unwrap()[0]
                    .content()
                    .as_inline()
                    .unwrap()
            ),
            "乙"
        );
        assert_eq!(
            text(
                items[1].content().as_children().unwrap()[0]
                    .content()
                    .as_inline()
                    .unwrap()
            ),
            "尾"
        );
    }

    #[test]
    fn selection_crossing_into_list_retains_separate_root_fragments() {
        let fixture = fixture();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.second, 1),
            point(&fixture.document, fixture.list_first, 3),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();

        assert_eq!(slice.roots().len(), 2);
        assert!(matches!(slice.roots()[0].kind(), NodeKind::Heading(_)));
        assert!(matches!(slice.roots()[1].kind(), NodeKind::BulletList));
        assert_eq!(slice.plain_text(), "d\n甲");
    }

    #[test]
    fn boundary_only_selection_still_carries_two_empty_leaves() {
        let fixture = fixture();
        let first_len = fixture
            .document
            .node(fixture.first)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .len_bytes();
        let selection = DocumentSelection::new(
            point(&fixture.document, fixture.first, first_len),
            point(&fixture.document, fixture.second, 0),
        );
        let slice = slice_selection(&fixture.document, selection)
            .unwrap()
            .unwrap();
        assert_eq!(slice.plain_text(), "\n");
        assert_eq!(slice.blocks().len(), 2);
        assert!(slice.blocks().iter().all(|block| block.inline().is_empty()));
    }

    #[test]
    fn same_logical_text_point_with_different_affinity_is_not_content() {
        let fixture = fixture();
        let point = point(&fixture.document, fixture.first, 1);
        let after = TextPoint::new(point.node_id(), point.offset(), CursorAffinity::After);
        let selection = DocumentSelection::new(point, after);
        assert_eq!(slice_selection(&fixture.document, selection).unwrap(), None);
    }
}
