//! P3.5 canonical LF / HardBreak / CodeBlock regressions.

use xiaomu_core::document::{
    InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent, SessionOutcome};

fn document_with(kind: NodeKind, text: &str) -> (XiaomuDocument, NodeId) {
    let content = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    let mut builder = NodeStoreBuilder::new();
    let block = builder
        .insert(kind, NodeAttrs::empty(), NodeContent::Inline(content))
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), block)
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn session_at(document: &XiaomuDocument, node: NodeId, raw: usize) -> DocumentSession {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(point(document, node, raw)),
    )
    .unwrap()
}

fn text(session: &DocumentSession, node: NodeId) -> String {
    session
        .document()
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

fn mark_at(session: &DocumentSession, node: NodeId, raw: usize, kind: MarkKind) -> bool {
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let end = cursor + run.len_bytes();
        if raw >= cursor && raw < end {
            return run.marks().contains(kind);
        }
        cursor = end;
    }
    false
}

fn insert(session: &mut DocumentSession, value: &str) {
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText {
                text: value.to_owned(),
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
}

#[test]
fn hard_break_is_one_lf_inside_the_same_paragraph_and_breaks_typing_history() {
    let (document, paragraph) = document_with(NodeKind::Paragraph, "ab");
    let root = document.root();
    let mut session = session_at(&document, paragraph, 1);

    insert(&mut session, "X");
    assert_eq!(session.history_depths(), (1, 0));

    assert_eq!(
        session
            .apply_intent(&EditIntent::insert_line_break())
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(text(&session, paragraph), "aX\nb");
    assert_eq!(
        session
            .text_selection()
            .unwrap()
            .focus()
            .offset()
            .as_usize(),
        3
    );
    assert_eq!(session.history_depths(), (2, 0));

    insert(&mut session, "Y");
    assert_eq!(text(&session, paragraph), "aX\nYb");
    assert_eq!(session.history_depths(), (3, 0));

    let children = session
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.as_ref(), &[paragraph]);

    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "aX\nb");
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "aXb");
    session.undo().unwrap();
    assert_eq!(text(&session, paragraph), "ab");

    session.redo().unwrap();
    session.redo().unwrap();
    session.redo().unwrap();
    assert_eq!(text(&session, paragraph), "aX\nYb");
}

#[test]
fn code_block_newline_keeps_stable_block_identity_and_backspace_deletes_one_lf() {
    let (document, code) = document_with(NodeKind::CodeBlock, "abcd");
    let root = document.root();
    let mut session = session_at(&document, code, 2);

    session
        .apply_intent(&EditIntent::insert_line_break())
        .unwrap();
    assert_eq!(text(&session, code), "ab\ncd");
    assert_eq!(
        session.document().node(code).unwrap().kind(),
        &NodeKind::CodeBlock
    );
    let children = session
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.as_ref(), &[code]);

    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(text(&session, code), "abcd");
    assert_eq!(session.history_depths(), (2, 0));

    session.undo().unwrap();
    assert_eq!(text(&session, code), "ab\ncd");
    session.undo().unwrap();
    assert_eq!(text(&session, code), "abcd");
}

#[test]
fn line_break_uses_and_preserves_stored_marks_for_following_typing() {
    let (document, paragraph) = document_with(NodeKind::Paragraph, "");
    let mut session = session_at(&document, paragraph, 0);

    assert_eq!(
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap(),
        SessionOutcome::NoChange
    );
    session
        .apply_intent(&EditIntent::insert_line_break())
        .unwrap();
    insert(&mut session, "x");

    assert_eq!(text(&session, paragraph), "\nx");
    assert!(mark_at(&session, paragraph, 0, MarkKind::Bold));
    assert!(mark_at(&session, paragraph, 1, MarkKind::Bold));
    assert!(
        session
            .stored_marks()
            .expect("line break keeps pending marks")
            .contains(MarkKind::Bold)
    );
}

#[test]
fn hard_break_survives_single_block_clipboard_projection() {
    let (document, paragraph) = document_with(NodeKind::Paragraph, "a\nb");
    let mut session = session_at(&document, paragraph, 0);

    session
        .apply_intent(&EditIntent::SetSelection {
            anchor: point(session.document(), paragraph, 0),
            focus: point(session.document(), paragraph, 3),
        })
        .unwrap();

    let slice = session
        .clipboard_slice()
        .unwrap()
        .expect("non-collapsed selection projects");
    assert_eq!(slice.plain_text(), "a\nb");
    assert_eq!(slice.blocks().len(), 1);
}
