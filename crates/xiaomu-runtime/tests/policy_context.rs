//! Effective typing marks and explicit inheritance decisions at the host seam.

use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, IntentDisposition, PolicyError, SessionContext,
    SessionError, SessionPolicy,
};

struct CheckMarks;

impl SessionPolicy for CheckMarks {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        match intent {
            EditIntent::ToggleMark { mark: Mark::Bold } => {
                Ok(IntentDisposition::StoredMarks(Some(MarkSet::empty())))
            }
            EditIntent::ToggleMark { mark: Mark::Italic } => {
                Ok(IntentDisposition::StoredMarks(None))
            }
            EditIntent::InsertText { .. } => {
                let expected = match context.stored_marks() {
                    Some(explicit) => explicit.clone(),
                    None => MarkSet::new([Mark::Bold]).unwrap(),
                };
                assert_eq!(context.effective_typing_marks(), Some(expected));
                Ok(IntentDisposition::Continue)
            }
            EditIntent::Delete => {
                assert_eq!(context.effective_typing_marks(), None);
                Ok(IntentDisposition::StoredMarks(Some(MarkSet::empty())))
            }
            _ => Ok(IntentDisposition::Continue),
        }
    }
}

fn fixture() -> (XiaomuDocument, xiaomu_core::document::NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = InlineContent::new([
        TextRun::new("a", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
        TextRun::new("b", MarkSet::new([Mark::Italic]).unwrap()).unwrap(),
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

#[test]
fn context_uses_runtime_left_run_inheritance_at_zero_and_a_run_boundary() {
    for raw in [0, 1] {
        let (doc, node) = fixture();
        let at = doc
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap();
        let selection =
            DocumentSelection::collapsed(TextPoint::new(node, at, CursorAffinity::Before));
        let mut session =
            DocumentSession::new_with_policy(doc, selection, Box::new(CheckMarks)).unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "X".into() })
            .unwrap();
        let inline = session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert_eq!(
            inline.runs()[0].text().as_str(),
            if raw == 0 { "Xa" } else { "aX" }
        );
    }
}

#[test]
fn explicit_empty_marks_and_reset_to_inheritance_remain_distinct() {
    let (doc, node) = fixture();
    let selection = DocumentSelection::collapsed(TextPoint::at_start_of(node));
    let mut session =
        DocumentSession::new_with_policy(doc, selection, Box::new(CheckMarks)).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&MarkSet::empty()));
    session
        .apply_intent(&EditIntent::InsertText { text: "X".into() })
        .unwrap();
    let runs = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert_eq!(runs[0].text().as_str(), "X");
    assert_eq!(runs[0].marks(), &MarkSet::empty());
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
        .unwrap();
    assert_eq!(session.stored_marks(), None);
    session
        .apply_intent(&EditIntent::InsertText { text: "Y".into() })
        .unwrap();
    assert_eq!(
        session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .text()
            .as_str(),
        "Ya"
    );
}

#[test]
fn explicit_marks_decision_rejects_ranges_without_changing_any_state() {
    let (doc, node) = fixture();
    let inline = doc.node(node).unwrap().content().as_inline().unwrap();
    let selection = DocumentSelection::new(
        TextPoint::new(node, TextOffset::ZERO, CursorAffinity::Before),
        TextPoint::new(node, inline.offset_at(2).unwrap(), CursorAffinity::Before),
    );
    let mut session =
        DocumentSession::new_with_policy(doc.clone(), selection, Box::new(CheckMarks)).unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::Delete),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.document().store(), doc.store());
    assert_eq!(session.document().revision(), doc.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.history_depths(), (0, 0));
}
