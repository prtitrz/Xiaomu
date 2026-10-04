//! The frontend mark query shares default input inheritance without mutations.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, TextRun, TextStyleAttributes, TextStyleMark,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn mark(color: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_color(color.into()),
    ))
}
fn marks(color: &str) -> MarkSet {
    MarkSet::new([mark(color)]).unwrap()
}
fn fixture() -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = InlineContent::new([
        TextRun::new("a", marks("red")).unwrap(),
        TextRun::new("中", marks("blue")).unwrap(),
    ])
    .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("0123456789".into())
        .offset_at(raw)
        .unwrap()
}
fn point(node: NodeId, raw: usize) -> TextPoint {
    TextPoint::new(node, offset(raw), CursorAffinity::Before)
}

#[test]
fn range_start_not_focus_controls_query_and_composition_commit() {
    let (doc, node) = fixture();
    let selection = DocumentSelection::new(point(node, 0), point(node, 4));
    let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
    assert_eq!(
        session.effective_input_marks(node, offset(0)).unwrap(),
        marks("red")
    );
    assert_eq!(
        session.effective_input_marks(node, offset(1)).unwrap(),
        marks("red")
    );
    assert_eq!(
        session.effective_input_marks(node, offset(4)).unwrap(),
        marks("blue")
    );
    assert_eq!(session.document().revision(), doc.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(session.stored_marks(), None);
    session
        .apply_intent(&EditIntent::CommitComposition {
            range: TextRange::new(offset(0), offset(1)).unwrap(),
            text: "拼".into(),
        })
        .unwrap();
    let runs = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs[0].text().as_str(), "拼");
    assert_eq!(runs[0].marks(), &marks("red"));
    assert_eq!(runs[1].marks(), &marks("blue"));
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
}

#[test]
fn pending_marks_and_explicit_empty_override_surrounding_runs_without_revision() {
    let (doc, node) = fixture();
    let mut session =
        DocumentSession::new(doc.clone(), DocumentSelection::collapsed(point(node, 1))).unwrap();
    session
        .apply_intent(&EditIntent::SetMark {
            mark: mark("green"),
        })
        .unwrap();
    assert_eq!(
        session.effective_input_marks(node, offset(4)).unwrap(),
        marks("green")
    );
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::TextStyle,
        })
        .unwrap();
    assert_eq!(
        session.effective_input_marks(node, offset(1)).unwrap(),
        MarkSet::empty()
    );
    assert_eq!(session.document().revision(), doc.revision());
    assert_eq!(session.history_depths(), (0, 0));
    session
        .apply_intent(&EditIntent::CommitComposition {
            range: TextRange::new(offset(1), offset(1)).unwrap(),
            text: "x".into(),
        })
        .unwrap();
    let runs = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs[1].text().as_str(), "x");
    assert_eq!(runs[1].marks(), &MarkSet::empty());
}

#[test]
fn foreign_node_and_stale_or_mid_scalar_offsets_reject_without_state_change() {
    let (doc, node) = fixture();
    let session =
        DocumentSession::new(doc.clone(), DocumentSelection::collapsed(point(node, 4))).unwrap();
    for (query_node, at) in [
        (doc.root(), offset(0)),
        (node, offset(2)),
        (node, offset(5)),
    ] {
        assert!(session.effective_input_marks(query_node, at).is_err());
    }
    assert_eq!(session.document().store(), doc.store());
    assert_eq!(session.document().revision(), doc.revision());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(session.stored_marks(), None);
}

#[test]
fn atom_gap_ordinal_keeps_the_same_default_mark_inheritance() {
    let (doc, node) = fixture();
    let at = InlinePoint::new(node, offset(1), 0, CursorAffinity::Before);
    let doc = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at,
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@A").unwrap(),
        })
        .apply(&doc)
        .unwrap();
    let selection =
        DocumentSelection::collapsed(InlinePoint::new(node, offset(1), 1, CursorAffinity::Before));
    let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
    assert_eq!(
        session.effective_input_marks(node, offset(1)).unwrap(),
        marks("red")
    );
    session
        .apply_intent(&EditIntent::CommitComposition {
            range: TextRange::new(offset(1), offset(1)).unwrap(),
            text: "x".into(),
        })
        .unwrap();
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(inline.runs()[0].text().as_str(), "ax");
    assert_eq!(inline.runs()[0].marks(), &marks("red"));
    assert_eq!(inline.atoms().len(), 1);
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
}
