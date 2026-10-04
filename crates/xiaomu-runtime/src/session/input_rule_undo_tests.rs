//! Private budget, ownership and staged-publication checks.

use super::*;
use crate::session::{EditIntent, structure::StagedPlan};
use std::collections::BTreeMap;
use xiaomu_core::document::{AtomKind, InlineContent, LinkMark, NodeId, NodeStoreBuilder, TextRun};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};

fn fixture() -> (DocumentSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    (DocumentSession::new(document, selection).unwrap(), node)
}

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn replace(node: NodeId, start: usize, end: usize, replacement: &str) -> TransactionStep {
    TransactionStep::ReplaceText {
        node,
        range: TextRange::new(offset(start), offset(end)).unwrap(),
        replacement: replacement.into(),
    }
}

fn install_token(session: &mut DocumentSession) {
    let selection = session.selection();
    let spec = InputRuleUndoSpec::new(transaction(), selection).unwrap();
    session
        .commit(
            EditPlan::new(transaction(), SelectionUpdate::Exact { selection }, None)
                .with_input_rule_undo(spec),
        )
        .unwrap();
    assert!(session.input_rule_undo_available());
}

#[test]
fn budget_accounts_steps_deep_payloads_and_origin_metadata() {
    let (session, node) = fixture();
    let selection = session.selection();
    let mut boundary = transaction();
    for _ in 0..MAX_STEPS {
        boundary.push_step(TransactionStep::RemoveNode { node });
    }
    assert!(InputRuleUndoSpec::new(boundary.clone(), selection).is_ok());
    boundary.push_step(TransactionStep::RemoveNode { node });
    assert!(matches!(
        InputRuleUndoSpec::new(boundary, selection),
        Err(SessionError::InputRuleUndoBudgetExceeded)
    ));

    let huge = "x".repeat(MAX_BYTES);
    let large_attrs = NodeAttrs::new(BTreeMap::from([(
        "payload".into(),
        AttrValue::List(vec![AttrValue::Object(BTreeMap::from([(
            "nested".into(),
            AttrValue::String(huge.clone()),
        )]))]),
    )]))
    .unwrap();
    let large_marks = MarkSet::new([Mark::Link(LinkMark::new(huge.clone(), None))]).unwrap();
    let large_inline =
        InlineContent::new([TextRun::new("x", large_marks.clone()).unwrap()]).unwrap();
    let mut origin = Transaction::new(TransactionOrigin::Extension(huge.clone()));
    origin.set_metadata("key", "small").unwrap();
    let mut metadata = transaction();
    metadata.set_metadata("key", huge.clone()).unwrap();
    for tx in [
        transaction().with_step(replace(node, 0, 0, &huge)),
        transaction().with_step(TransactionStep::SetNodeAttrs {
            node,
            attrs: large_attrs,
        }),
        transaction().with_step(TransactionStep::SetInlineAtomMarks {
            atom: node,
            marks: large_marks.clone(),
        }),
        transaction().with_step(TransactionStep::InsertNode {
            parent: session.document().root(),
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Inline(large_inline),
        }),
        transaction().with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::at_start_of(node),
            kind: AtomKind::new("example").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(huge.clone()).unwrap(),
        }),
        transaction().with_step(TransactionStep::SetNodeKind {
            node,
            kind: NodeKind::custom(huge).unwrap(),
        }),
        origin,
        metadata,
    ] {
        assert!(matches!(
            InputRuleUndoSpec::new(tx, selection),
            Err(SessionError::InputRuleUndoBudgetExceeded)
        ));
    }
    assert!(matches!(
        InputRuleUndoSpec::new(transaction(), selection)
            .unwrap()
            .with_stored_marks(Some(large_marks)),
        Err(SessionError::InputRuleUndoBudgetExceeded)
    ));
}

#[test]
fn budget_rejects_recursive_attrs_and_subtree_node_payloads() {
    let (session, node) = fixture();
    let mut value = AttrValue::Null;
    for _ in 0..MAX_ATTR_DEPTH + 2 {
        value = AttrValue::List(vec![value]);
    }
    let attrs = NodeAttrs::new(BTreeMap::from([("nested".into(), value)])).unwrap();
    assert!(
        InputRuleUndoSpec::new(
            transaction().with_step(TransactionStep::SetNodeAttrs { node, attrs }),
            session.selection()
        )
        .is_err()
    );
    let nodes = vec![session.document().node(node).unwrap().clone(); MAX_NODES + 1];
    let tx = transaction().with_step(TransactionStep::RestoreSubtree {
        parent: session.document().root(),
        index: 0,
        root: node,
        nodes,
    });
    assert!(matches!(
        InputRuleUndoSpec::new(tx, session.selection()),
        Err(SessionError::InputRuleUndoBudgetExceeded)
    ));
}

#[test]
fn constructor_does_not_retain_large_spare_string_capacity() {
    let (session, node) = fixture();
    let mut replacement = String::with_capacity(MAX_BYTES * 4);
    replacement.push('x');
    let spec = InputRuleUndoSpec::new(
        transaction().with_step(TransactionStep::ReplaceText {
            node,
            range: TextRange::new(TextOffset::ZERO, TextOffset::ZERO).unwrap(),
            replacement,
        }),
        session.selection(),
    )
    .unwrap();
    match &spec.transaction.steps()[0] {
        TransactionStep::ReplaceText { replacement, .. } => assert_eq!(replacement.capacity(), 1),
        _ => panic!("replacement expected"),
    }
}

#[test]
fn token_binds_actual_commit_revision_not_private_planner_revision() {
    let (mut session, node) = fixture();
    let first = transaction().with_step(replace(node, 0, 0, "a"));
    let second = transaction().with_step(replace(node, 1, 1, "b"));
    let planner = second
        .apply(&first.apply(session.document()).unwrap())
        .unwrap();
    assert_eq!(planner.revision().as_u64(), 2);
    let selection = DocumentSelection::collapsed(InlinePoint::new(
        node,
        offset(2),
        0,
        xiaomu_core::selection::CursorAffinity::After,
    ));
    let reverse = transaction().with_step(replace(node, 0, 2, ""));
    let spec = InputRuleUndoSpec::new(reverse, session.selection()).unwrap();
    let combined = transaction()
        .with_step(replace(node, 0, 0, "a"))
        .with_step(replace(node, 1, 1, "b"));
    session
        .commit(
            EditPlan::new(combined, SelectionUpdate::Exact { selection }, None)
                .with_input_rule_undo(spec),
        )
        .unwrap();
    assert_eq!(session.document().revision().as_u64(), 1);
    assert!(session.input_rule_undo_available());
    session
        .with_transient_rollback(DocumentSession::undo_input_rule)
        .unwrap();
    assert!(!session.input_rule_undo_available());
    assert_eq!(
        session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .len_bytes(),
        0
    );
}

#[test]
fn staged_failure_keeps_same_rc_token_success_drops_it() {
    let (mut session, node) = fixture();
    install_token(&mut session);
    let token = Rc::downgrade(session.input_rule_undo.as_ref().unwrap());
    let before = session.document().revision();
    let failed = StagedPlan::new(SelectionUpdate::PreserveSelection)
        .stage(move |_| Ok(transaction().with_step(replace(node, 0, 0, "x"))))
        .stage(|_| Err(SessionError::UnsupportedEdit));
    assert_eq!(
        session.with_transient_rollback(|session| session.commit_staged(failed)),
        Err(SessionError::UnsupportedEdit)
    );
    assert_eq!(session.document().revision(), before);
    assert!(session.input_rule_undo_available());
    assert!(Rc::ptr_eq(
        session.input_rule_undo.as_ref().unwrap(),
        &token.upgrade().unwrap()
    ));
    assert_eq!(
        token.strong_count(),
        1,
        "rollback retains no extra snapshot"
    );
    let valid = StagedPlan::new(SelectionUpdate::PreserveSelection).stage(|_| Ok(transaction()));
    session
        .with_transient_rollback(|session| session.commit_staged(valid))
        .unwrap();
    assert!(!session.input_rule_undo_available());
    assert!(token.upgrade().is_none());
}

#[test]
fn successful_new_rule_replaces_old_token_and_empty_history_queries_preserve_it() {
    let (mut session, _) = fixture();
    install_token(&mut session);
    let old = Rc::downgrade(session.input_rule_undo.as_ref().unwrap());
    install_token(&mut session);
    assert!(old.upgrade().is_none());
    assert_eq!(session.redo(), Ok(SessionOutcome::NoChange));
    assert!(session.input_rule_undo_available());
    // Empty-Undo cannot occur naturally while a committing rule entry remains,
    // but exercising the internal boundary guards against blanket clearing.
    session.history = super::super::HistoryStack::new();
    assert_eq!(session.undo(), Ok(SessionOutcome::NoChange));
    assert!(session.input_rule_undo_available());
}

#[test]
fn horizontal_rule_intent_has_no_default_canonical_behavior() {
    let (mut session, _) = fixture();
    let document = session.document().clone();
    assert_eq!(
        session.apply_intent(&EditIntent::InsertHorizontalRule),
        Err(SessionError::UnsupportedEdit)
    );
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.history_depths(), (0, 0));
}
