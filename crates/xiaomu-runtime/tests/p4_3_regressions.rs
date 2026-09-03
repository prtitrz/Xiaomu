//! Regressions found while auditing the P4.3 mixed-inline implementation.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn offset(raw: usize) -> TextOffset {
    const SCRATCH: &str = "000000000000000000000000000000000000000000000000";
    TextBuffer::from_string(SCRATCH.to_owned())
        .offset_at(raw)
        .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn document_with_blocks(texts: &[&str]) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let blocks: Vec<_> = texts
        .iter()
        .map(|text| paragraph(&mut builder, text))
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(blocks.iter().copied()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), blocks)
}

fn point(node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, CursorAffinity::Before)
}

fn position(node: NodeId, raw: usize, ordinal: usize) -> DocumentPosition {
    DocumentPosition::Inline(point(node, raw, ordinal))
}

fn insert_atom(
    document: XiaomuDocument,
    node: NodeId,
    raw: usize,
    ordinal: usize,
    fallback: &str,
) -> (XiaomuDocument, NodeId) {
    let before: Vec<_> = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .collect();
    let next = Transaction::new(TransactionOrigin::Extension("regression".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at: point(node, raw, ordinal),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(&document)
        .unwrap();
    let atom = next
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .find(|atom| !before.contains(atom))
        .unwrap();
    (next, atom)
}

fn caret(session: &DocumentSession) -> InlinePoint {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => point,
        DocumentPosition::Gap(_) => panic!("expected inline caret"),
    }
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

#[test]
fn home_and_end_cross_outer_atom_gaps_at_same_text_offset() {
    let (document, blocks) = document_with_blocks(&["ab"]);
    let node = blocks[0];
    let (document, _) = insert_atom(document, node, 0, 0, "@lead");
    let (document, _) = insert_atom(document, node, 2, 0, "@tail");

    let mut home = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(position(node, 0, 1)),
    )
    .unwrap();
    assert_eq!(
        home.apply_intent(&EditIntent::MoveCaret {
            caret_move: CaretMove::ToStart,
            extend_selection: false,
        })
        .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(
        (
            caret(&home).text_offset().as_usize(),
            caret(&home).atom_index()
        ),
        (0, 0)
    );

    let mut end =
        DocumentSession::new(document, DocumentSelection::collapsed(position(node, 2, 0))).unwrap();
    assert_eq!(
        end.apply_intent(&EditIntent::MoveCaret {
            caret_move: CaretMove::ToEnd,
            extend_selection: false,
        })
        .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(
        (
            caret(&end).text_offset().as_usize(),
            caret(&end).atom_index()
        ),
        (2, 1)
    );
}

#[test]
fn paste_trailing_atom_uses_post_edit_unicode_boundary() {
    let (source, blocks) = document_with_blocks(&["中"]);
    let source_node = blocks[0];
    let (source, _) = insert_atom(source, source_node, 3, 0, "@尾");
    let source_session = DocumentSession::new(
        source,
        DocumentSelection::new(position(source_node, 0, 0), position(source_node, 3, 1)),
    )
    .unwrap();
    let slice = source_session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "中@尾");

    let (target, blocks) = document_with_blocks(&["x"]);
    let target_node = blocks[0];
    let mut target_session = DocumentSession::new(
        target,
        DocumentSelection::collapsed(position(target_node, 1, 0)),
    )
    .unwrap();
    target_session
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();

    let inline = target_session
        .document()
        .node(target_node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(text(target_session.document(), target_node), "x中");
    assert_eq!(inline.atoms().len(), 1);
    assert_eq!(inline.atoms()[0].text_offset().as_usize(), 4);
}

#[test]
fn cross_block_copy_keeps_trailing_head_atoms_when_text_part_is_empty() {
    let (document, blocks) = document_with_blocks(&["a", "b"]);
    let first = blocks[0];
    let second = blocks[1];
    let (document, _) = insert_atom(document, first, 1, 0, "@A");
    let session = DocumentSession::new(
        document,
        DocumentSelection::new(position(first, 1, 0), position(second, 1, 0)),
    )
    .unwrap();

    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "@A\nb");
    assert_eq!(slice.blocks()[0].inline().atoms().len(), 1);
}

#[test]
fn cross_block_delete_moves_unselected_tail_atoms_and_undo_restores_store() {
    let (document, blocks) = document_with_blocks(&["ab", "cd"]);
    let first = blocks[0];
    let second = blocks[1];

    let (document, head_keep) = insert_atom(document, first, 1, 0, "@head-keep");
    let (document, head_selected) = insert_atom(document, first, 2, 0, "@head-selected");
    let (document, tail_selected) = insert_atom(document, second, 0, 0, "@tail-selected");
    let (document, tail_keep) = insert_atom(document, second, 1, 0, "@tail-keep");
    let original_store = document.store().clone();

    let selection = DocumentSelection::new(position(first, 1, 1), position(second, 1, 0));
    let mut session = DocumentSession::new(document, selection).unwrap();
    session.apply_intent(&EditIntent::Backspace).unwrap();

    assert_eq!(text(session.document(), first), "ad");
    assert!(session.document().node(second).is_none());
    assert!(session.document().node(head_selected).is_none());
    assert!(session.document().node(tail_selected).is_none());
    assert_eq!(session.document().parent_of(head_keep), Some(first));
    assert_eq!(session.document().parent_of(tail_keep), Some(first));

    let inline = session
        .document()
        .node(first)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let seam_atoms: Vec<_> = inline
        .atoms()
        .iter()
        .filter(|placement| placement.text_offset().as_usize() == 1)
        .map(|placement| placement.atom())
        .collect();
    assert_eq!(seam_atoms, [head_keep, tail_keep]);
    assert_eq!(
        (
            caret(&session).text_offset().as_usize(),
            caret(&session).atom_index()
        ),
        (1, 1)
    );

    session.undo().unwrap();
    assert_eq!(session.document().store(), &original_store);
    assert_eq!(session.selection(), selection);

    session.redo().unwrap();
    assert_eq!(text(session.document(), first), "ad");
    assert_eq!(session.document().parent_of(tail_keep), Some(first));
}
