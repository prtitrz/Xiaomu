//! Original source selection remains separate from an atomic intent target.
use xiaomu_core::{
    document::{InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::{InlinePoint, NodeGap},
};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition, PolicyError,
    SessionContext, SessionOutcome, SessionPolicy,
};

struct SourceRules;
impl SessionPolicy for SourceRules {
    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        assert_eq!(context.original_selection(), context.selection());
        Ok(None)
    }
    fn clipboard_export_spec(
        &self,
        context: SessionContext<'_>,
        _: xiaomu_runtime::clipboard::ClipboardExportPurpose,
    ) -> Result<Option<xiaomu_runtime::clipboard::ClipboardExportSpec>, PolicyError> {
        assert_eq!(context.original_selection(), context.selection());
        Ok(None)
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let children = context
            .document()
            .node(context.document().root())
            .unwrap()
            .content()
            .as_children()
            .unwrap();
        let original = context
            .original_selection()
            .as_same_node_inline()
            .unwrap()
            .1
            .node_id();
        let target = context
            .selection()
            .as_same_node_inline()
            .unwrap()
            .1
            .node_id();
        match intent {
            EditIntent::InsertText { text } if text == "target" => {
                assert_eq!(original, children[0]);
                assert_eq!(target, children[1]);
            }
            EditIntent::InsertText { text } if text == "refuse" || text == "no-op" => {
                assert_eq!(original, children[1]);
                assert_eq!(target, children[0]);
                return if text == "refuse" {
                    Err(PolicyError::new("refused"))
                } else {
                    Ok(IntentDisposition::NoChange)
                };
            }
            _ => assert_eq!(context.original_selection(), context.selection()),
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn original_selection_is_fresh_after_success_failure_undo_redo_and_normal_intents() {
    let mut builder = NodeStoreBuilder::new();
    let mut paragraph = || {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(InlineContent::empty()),
            )
            .unwrap()
    };
    let first = paragraph();
    let second = paragraph();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let original = DocumentSelection::collapsed(InlinePoint::at_start_of(first));
    let target = DocumentSelection::collapsed(InlinePoint::at_start_of(second));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), original, Box::new(SourceRules))
            .unwrap();
    session
        .apply_intent_with_selection(
            target,
            &EditIntent::InsertText {
                text: "target".into(),
            },
        )
        .unwrap();
    assert!(session.prepare_cut().unwrap().is_none());
    assert!(session.clipboard_slice().unwrap().is_none());
    let after = session.selection();
    let after_doc = session.document().clone();
    for text in ["refuse", "no-op"] {
        let result = session
            .apply_intent_with_selection(original, &EditIntent::InsertText { text: text.into() });
        if text == "refuse" {
            assert!(result.is_err());
        } else {
            assert_eq!(result, Ok(SessionOutcome::NoChange));
        }
        assert_eq!(session.document().store(), after_doc.store());
        assert_eq!(session.document().revision(), after_doc.revision());
        assert_eq!(session.selection(), after);
        assert_eq!(session.history_depths(), (1, 0));
    }
    // Invalid target never reaches policy: it would panic on the gap above.
    assert!(
        session
            .apply_intent_with_selection(
                DocumentSelection::collapsed(NodeGap::new(root, 99)),
                &EditIntent::Delete
            )
            .is_err()
    );
    assert_eq!(session.selection(), after);
    assert!(session.prepare_cut().unwrap().is_none());
    assert!(session.clipboard_slice().unwrap().is_none());
    session
        .apply_intent(&EditIntent::InsertText {
            text: String::new(),
        })
        .unwrap();
    session.undo().unwrap();
    assert_eq!(session.selection(), original);
    assert_eq!(session.document().store(), document.store());
    session
        .apply_intent(&EditIntent::InsertText {
            text: String::new(),
        })
        .unwrap();
    session.redo().unwrap();
    assert_eq!(session.selection(), after);
    assert!(session.prepare_cut().unwrap().is_none());
    assert!(session.clipboard_slice().unwrap().is_none());
    session
        .apply_intent(&EditIntent::InsertText {
            text: String::new(),
        })
        .unwrap();
}
