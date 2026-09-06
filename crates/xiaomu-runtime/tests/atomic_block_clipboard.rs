//! P4.6 atomic block clipboard regressions.
//!
//! A collapsed atomic node selection projects into a whole-block clipboard
//! fragment (kind + attrs, no interior), survives the versioned metadata
//! wire, pastes as a sibling block after the focused block, and mixed
//! inline/atomic contexts still fail closed.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::clipboard::{ClipboardNodeContent, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError,
};

fn offset_of(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    let text: String = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    TextBuffer::from_string(text).offset_at(byte).unwrap()
}

/// `Document > [p("前"), HorizontalRule, p("后")]`.
fn fixture() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("前", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::new([].into_iter().collect()).unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let last = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("后", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, rule, last]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        rule,
        last,
    )
}

fn session_at(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentSession {
    let inline = DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_of(document, node, byte),
        0,
        CursorAffinity::Before,
    ));
    DocumentSession::new(document.clone(), DocumentSelection::collapsed(inline)).unwrap()
}

#[test]
fn atomic_selection_projects_to_a_whole_block_fragment() {
    let (document, first, rule, _) = fixture();
    let mut session = session_at(&document, first, 0);
    session.set_atomic_selection(rule).unwrap();

    let slice = session.clipboard_slice().unwrap().expect("atomic fragment");
    assert_eq!(slice.roots().len(), 1);
    let root = &slice.roots()[0];
    assert!(matches!(root.kind(), NodeKind::HorizontalRule));
    assert!(matches!(root.content(), ClipboardNodeContent::Atomic));
    assert_eq!(slice.plain_text(), "");

    // The wire round trip preserves the whole block.
    let metadata = encode_metadata(&slice).unwrap();
    let decoded = decode_metadata("", &metadata).expect("v4 metadata decodes");
    assert_eq!(decoded.roots().len(), 1);
    assert!(matches!(
        decoded.roots()[0].content(),
        ClipboardNodeContent::Atomic
    ));
    assert!(matches!(
        decoded.roots()[0].kind(),
        NodeKind::HorizontalRule
    ));
}

#[test]
fn pasting_an_atomic_fragment_inserts_a_sibling_block() {
    let (document, first, rule, _) = fixture();
    let mut source = session_at(&document, first, 0);
    source.set_atomic_selection(rule).unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    // Paste into a fresh document at the caret inside the first paragraph:
    // the rule inserts as the next sibling, the caret stays put.
    let (fresh, first, _, _) = fixture();
    let mut target = session_at(&fresh, first, 3);
    let outcome = target
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    assert_eq!(
        outcome,
        xiaomu_runtime::session::SessionOutcome::DocumentChanged
    );

    let children = target
        .document()
        .node(target.document().root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(
        children.len(),
        4,
        "rule inserts right after the focused block"
    );
    assert!(matches!(
        target.document().node(children[1]).unwrap().kind(),
        NodeKind::HorizontalRule
    ));

    // The caret stays inline inside the focused paragraph.
    match target.selection().focus() {
        DocumentPosition::Inline(point) => {
            assert_eq!(point.node_id(), first);
            assert_eq!(point.text_offset().as_usize(), 3);
        }
        other => panic!("caret must stay inline after atomic paste: {other:?}"),
    }
}

#[test]
fn pasting_onto_an_atomic_selection_fails_closed() {
    let (document, first, rule, _) = fixture();
    let mut source = session_at(&document, first, 0);
    source.set_atomic_selection(rule).unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();

    let mut target = source;
    // Target still has the node selection active: no replace-selection
    // contract exists yet, so the paste is rejected instead of guessing.
    assert_eq!(
        target.apply_intent(&EditIntent::PasteSlice { slice }),
        Err(SessionError::ClipboardAtomicUnsupported),
    );
    // The document is unchanged.
    assert!(target.document().node(rule).is_some());
}
