//! The default planner must never flatten typed tasks or consume their state.

mod task_list_support;
use task_list_support::*;

use xiaomu_core::document::{
    AttrValue, Mark, NodeAttrs, NodeKind, NodeStoreBuilder, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::decode_metadata;
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

#[test]
fn default_paste_refuses_open_closed_and_single_leaf_task_wrappers_atomically() {
    let (document, first, tail) = fixture(Some(AttrValue::Bool(true)));
    let mut slices = vec![
        copy(&document, first, tail, false),
        copy(&document, first, tail, true),
    ];
    let mut one = roundtrip(&slices[0]);
    one["roots"].as_array_mut().unwrap().truncate(1);
    one["roots"][0]["content"]["value"]["children"][0]["content"]["value"]["children"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    slices.push(decode_metadata("task中🙂", &one.to_string()).unwrap());
    for slice in slices {
        let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(tail));
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "a".into() })
            .unwrap();
        let before = session.document().clone();
        let before_selection = session.selection();
        let before_marks = session.stored_marks().cloned();
        assert_eq!(
            session.apply_intent(&EditIntent::PasteSlice { slice }),
            Err(SessionError::UnsupportedEdit)
        );
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), before_selection);
        assert_eq!(session.stored_marks(), before_marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        session
            .apply_intent(&EditIntent::InsertText { text: "b".into() })
            .unwrap();
        assert_eq!(
            session.history_depths(),
            (1, 0),
            "failed task paste keeps typing coalescence"
        );
    }
}

#[test]
fn task_detection_precedes_table_and_cell_range_paste_dispatch() {
    let (document, first, tail) = fixture(None);
    let source = copy(&document, first, tail, true);
    let mut wire = roundtrip(&source);
    let task = wire["roots"][0].clone();
    wire["closed"] = serde_json::json!(false);
    wire["roots"] = serde_json::json!([{
        "kind":{"type":"table"},"attrs":{},
        "content":{"type":"table","value":{"rows":[[{
            "kind":{"type":"table_cell"},"attrs":{},
            "content":{"type":"children","value":{"children":[task]}}
        }]]}}
    }]);
    let table_slice = decode_metadata("task中🙂 code ", &wire.to_string()).unwrap();
    let mut builder = NodeStoreBuilder::new();
    let paragraph = text(&mut builder, NodeKind::Paragraph, "target");
    let cell = container(&mut builder, NodeKind::TableCell, vec![paragraph]);
    let row = container(&mut builder, NodeKind::TableRow, vec![cell]);
    let table = container(&mut builder, NodeKind::Table, vec![row]);
    let root = container(&mut builder, NodeKind::Document, vec![table]);
    let target = XiaomuDocument::new(root, builder.finish()).unwrap();
    for rectangular in [false, true] {
        let mut session = DocumentSession::new(
            target.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(paragraph)),
        )
        .unwrap();
        if rectangular {
            session.set_cell_range_selection(cell, cell).unwrap();
        }
        let selection = session.selection();
        assert_eq!(
            session.apply_intent(&EditIntent::PasteSlice {
                slice: table_slice.clone()
            }),
            Err(SessionError::UnsupportedEdit)
        );
        assert_eq!(session.document().store(), target.store());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), (0, 0));
    }
}

struct TaskPolicy;
impl SessionPolicy for TaskPolicy {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::PasteSlice { .. } = intent {
            // A sentinel plan demonstrates that an explicit policy owns this
            // intent before default rejection, without claiming a task fitter.
            let node = context
                .selection()
                .as_single_node()
                .unwrap()
                .focus()
                .node_id();
            return Ok(IntentDisposition::Apply(EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::SetNodeAttrs {
                        node,
                        attrs: NodeAttrs::new(
                            [("policy-handled".into(), AttrValue::Bool(true))].into(),
                        )
                        .unwrap(),
                    },
                ),
                SelectionUpdate::MapExisting,
                None,
            )));
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn an_explicit_policy_plan_can_intercept_task_paste_before_the_default_guard() {
    let (document, first, tail) = fixture(None);
    let source = copy(&document, first, tail, true);
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(tail));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(TaskPolicy))
            .unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::PasteSlice { slice: source }),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(
        session
            .document()
            .node(tail)
            .unwrap()
            .attrs()
            .get("policy-handled"),
        Some(&AttrValue::Bool(true))
    );
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
}
