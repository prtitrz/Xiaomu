//! Host plans may publish explicit typing marks with their final caret.

use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::TextPoint;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionError, SessionPolicy,
};

struct KeepMarks {
    reject_split: bool,
}

impl SessionPolicy for KeepMarks {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let plan = match intent {
            EditIntent::SplitBlock => {
                let focus = context.selection().as_single_node().unwrap().focus();
                EditPlan::new(
                    Transaction::new(TransactionOrigin::UserInput).with_step(
                        TransactionStep::SplitNode {
                            node: focus.node_id(),
                            at: focus.offset(),
                        },
                    ),
                    SelectionUpdate::CaretAtSplitTail,
                    None,
                )
                .with_stored_marks(context.effective_typing_marks())
            }
            EditIntent::Delete => EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput),
                SelectionUpdate::MapExisting,
                None,
            )
            .with_stored_marks(Some(MarkSet::empty())),
            _ => return Ok(IntentDisposition::Continue),
        };
        Ok(IntentDisposition::Apply(plan))
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.reject_split
            && document
                .node(document.root())
                .unwrap()
                .content()
                .as_children()
                .unwrap()
                .len()
                > 1
        {
            return Err(PolicyError::new("split rejected"));
        }
        Ok(())
    }
}

fn session(reject_split: bool, range: bool) -> DocumentSession {
    let mut builder = NodeStoreBuilder::new();
    let inline =
        InlineContent::new([TextRun::new("a", MarkSet::new([Mark::Bold]).unwrap()).unwrap()])
            .unwrap();
    let end = inline.offset_at(1).unwrap();
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
    let focus = TextPoint::new(node, end, xiaomu_core::selection::CursorAffinity::Before);
    let selection = if range {
        DocumentSelection::new(TextPoint::at_start_of(node), focus)
    } else {
        DocumentSelection::collapsed(focus)
    };
    DocumentSession::new_with_policy(
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        selection,
        Box::new(KeepMarks { reject_split }),
    )
    .unwrap()
}

#[test]
fn planned_split_carries_marks_to_empty_tail_and_typing_then_undoes_once() {
    let mut session = session(false, false);
    let original = session.document().clone();
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    assert_eq!(
        session.stored_marks(),
        Some(&MarkSet::new([Mark::Bold]).unwrap())
    );
    assert_eq!(session.history_depths(), (1, 0));
    session
        .apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    let node = session
        .selection()
        .as_single_node()
        .unwrap()
        .focus()
        .node_id();
    assert_eq!(
        session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks(),
        &MarkSet::new([Mark::Bold]).unwrap()
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
}

#[test]
fn after_marks_with_noncollapsed_selection_reject_before_publication() {
    let mut session = session(false, true);
    let original = session.document().clone();
    let selection = session.selection();
    assert_eq!(
        session.apply_intent(&EditIntent::Delete),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.document().store(), original.store());
    assert_eq!(session.document().revision(), original.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn rejected_candidate_never_installs_planned_marks_or_breaks_typing_group() {
    let mut session = session(true, false);
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let original = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    assert!(matches!(
        session.apply_intent(&EditIntent::SplitBlock),
        Err(SessionError::Policy(_))
    ));
    assert_eq!(session.document().store(), original.store());
    assert_eq!(session.document().revision(), original.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}
