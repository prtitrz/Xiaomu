//! Opt-in rule restoration is generic, atomic, bounded, and ephemeral.
//!
//! These policies deliberately use plain paragraphs rather than implementing
//! a Markdown rule. Host matching and history-group parity are separate seams.

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    InputRuleUndoSpec, IntentDisposition, PolicyError, SelectionUpdate, SessionContext,
    SessionError, SessionOutcome, SessionPolicy,
};

fn transaction() -> Transaction {
    Transaction::new(TransactionOrigin::UserInput)
}

// Deliberately independent of the candidate, permitting invalid final offsets.
fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn point(node: NodeId, raw: usize, ordinal: usize, affinity: CursorAffinity) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, affinity)
}

fn caret(node: NodeId, raw: usize) -> DocumentSelection {
    DocumentSelection::collapsed(point(node, raw, 0, CursorAffinity::Before))
}

fn replace(node: NodeId, start: usize, end: usize, replacement: &str) -> Transaction {
    transaction().with_step(TransactionStep::ReplaceText {
        node,
        range: TextRange::new(offset(start), offset(end)).unwrap(),
        replacement: replacement.into(),
    })
}

fn exact(transaction: Transaction, selection: DocumentSelection) -> EditPlan {
    EditPlan::new(transaction, SelectionUpdate::Exact { selection }, None)
}

fn fixture(value: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if value.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(value, MarkSet::empty()).unwrap()]).unwrap()
    };
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

fn restoration_plan(node: NodeId, before: &str, restored: &str) -> EditPlan {
    exact(replace(node, 0, before.len(), "R"), caret(node, 1)).with_input_rule_undo(
        InputRuleUndoSpec::new(replace(node, 0, 1, restored), caret(node, restored.len())).unwrap(),
    )
}

struct Rules {
    plan: EditPlan,
    alternative: Option<EditPlan>,
    route_backspace: bool,
    force_undo: bool,
    rejected_revision: Option<(u64, String)>,
}

impl Rules {
    fn new(plan: EditPlan) -> Self {
        Self {
            plan,
            alternative: None,
            route_backspace: true,
            force_undo: false,
            rejected_revision: None,
        }
    }
}

impl SessionPolicy for Rules {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        match intent {
            EditIntent::InsertText { text } if text == "rule" => {
                Ok(IntentDisposition::Apply(self.plan.clone()))
            }
            EditIntent::InsertText { text } if text == "alternative" => {
                Ok(IntentDisposition::Apply(self.alternative.clone().unwrap()))
            }
            EditIntent::InsertText { text } if text == "reject" => {
                Err(PolicyError::new("prepare rejected"))
            }
            EditIntent::InsertText { text } if text == "noop" => Ok(IntentDisposition::NoChange),
            EditIntent::InsertText { text } if text == "changed target" => {
                assert!(!context.input_rule_undo_available());
                Ok(IntentDisposition::NoChange)
            }
            EditIntent::Backspace
                if self.force_undo
                    || self.route_backspace && context.input_rule_undo_available() =>
            {
                Ok(IntentDisposition::UndoInputRule)
            }
            _ => Ok(IntentDisposition::Continue),
        }
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        let contents: String = document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .map(|run| run.text().as_str())
            .collect();
        if contents.contains('!')
            || self
                .rejected_revision
                .as_ref()
                .is_some_and(|(revision, value)| {
                    document.revision().as_u64() == *revision && contents == *value
                })
        {
            return Err(PolicyError::new("candidate rejected"));
        }
        Ok(())
    }
}

fn setup() -> (DocumentSession, NodeId) {
    let (document, node) = fixture("[x]");
    let rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
    let session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    (session, node)
}

fn insert(session: &mut DocumentSession, value: &str) -> Result<SessionOutcome, SessionError> {
    session.apply_intent(&EditIntent::InsertText { text: value.into() })
}

fn activate(session: &mut DocumentSession) {
    assert_eq!(insert(session, "rule"), Ok(SessionOutcome::DocumentChanged));
    assert!(session.input_rule_undo_available());
}

type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

fn listen(session: &mut DocumentSession) -> Counts {
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    counts
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    available: bool,
    counts: (usize, usize),
}

impl Snapshot {
    fn capture(session: &DocumentSession, counts: &Counts) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            available: session.input_rule_undo_available(),
            counts: counts.get(),
        }
    }

    fn assert_unchanged(&self, session: &DocumentSession, counts: &Counts) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(session.input_rule_undo_available(), self.available);
        assert_eq!(counts.get(), self.counts);
    }
}

#[test]
fn immediate_backspace_restores_actual_trigger_and_consumes_one_token() {
    for marker in ["[x]", "[]", "[ ]", ">", "中🙂"] {
        let (document, node) = fixture(marker);
        let restored = format!("{marker} ");
        let plan = restoration_plan(node, marker, &restored);
        let mut session = DocumentSession::new_with_policy(
            document,
            caret(node, marker.len()),
            Box::new(Rules::new(plan)),
        )
        .unwrap();
        let counts = listen(&mut session);
        activate(&mut session);
        assert_eq!(
            session.apply_intent(&EditIntent::Backspace),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_eq!(text(session.document(), node), restored);
        assert_eq!(session.selection(), caret(node, restored.len()));
        assert!(!session.input_rule_undo_available());
        assert_eq!(counts.get(), (2, 0));
        // Consumed metadata cannot reinterpret the next ordinary Backspace.
        session.apply_intent(&EditIntent::Backspace).unwrap();
        assert_eq!(text(session.document(), node), marker);
    }
}

#[test]
fn restoration_is_a_forward_isolated_edit_and_redo_never_republishes_metadata() {
    let (mut session, node) = setup();
    activate(&mut session);
    let converted = session.document().clone();
    session.apply_intent(&EditIntent::Backspace).unwrap();
    // Deliberately records the scoped, known extra Undo step, not PM grouping parity.
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), converted.store());
    assert!(!session.input_rule_undo_available());
    session.undo().unwrap();
    assert_eq!(text(session.document(), node), "[x]");
    assert_eq!(session.selection(), caret(node, 3));
    session.redo().unwrap();
    assert_eq!(session.document().store(), converted.store());
    assert!(!session.input_rule_undo_available());
    session.redo().unwrap();
    assert_eq!(text(session.document(), node), "[x] ");
    assert!(!session.input_rule_undo_available());
}

#[test]
fn ordinary_undo_restores_original_selection_and_omits_trigger() {
    let original = "[]picked中🙂";
    let (document, node) = fixture(original);
    let before = DocumentSelection::new(
        point(node, 2, 0, CursorAffinity::After),
        point(node, original.len(), 0, CursorAffinity::Before),
    );
    let mut session = DocumentSession::new_with_policy(
        document,
        before,
        Box::new(Rules::new(restoration_plan(node, original, "[] "))),
    )
    .unwrap();
    activate(&mut session);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "[] ");
    assert_eq!(session.selection(), caret(node, 3));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(session.document(), node), original);
    assert_eq!(session.selection(), before);
}

#[test]
fn no_policy_or_no_metadata_preserves_generic_backspace_behavior() {
    let (document, node) = fixture("中🙂");
    let mut plain = DocumentSession::new(document, caret(node, 7)).unwrap();
    assert!(!plain.input_rule_undo_available());
    plain.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(plain.document(), node), "中");

    let (document, node) = fixture("[x]");
    let plan = exact(replace(node, 0, 3, "R"), caret(node, 1));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(Rules::new(plan)))
            .unwrap();
    insert(&mut session, "rule").unwrap();
    assert!(!session.input_rule_undo_available());
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "");
}

#[test]
fn metadata_alone_does_not_intercept_backspace_without_host_disposition() {
    let (document, node) = fixture("[x]");
    let mut rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
    rules.route_backspace = false;
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    activate(&mut session);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "");
    assert!(!session.input_rule_undo_available());
}

#[test]
fn forced_unavailable_restoration_fails_without_falling_through_to_deletion() {
    let (document, node) = fixture("[x]");
    let mut rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
    rules.force_undo = true;
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session.apply_intent(&EditIntent::Backspace),
        Err(SessionError::UnsupportedEdit)
    );
    before.assert_unchanged(&session, &counts);
    activate(&mut session);
    // Proposed target differs even though the old current caret still matches.
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session.apply_intent_with_selection(caret(node, 0), &EditIntent::Backspace),
        Err(SessionError::UnsupportedEdit)
    );
    before.assert_unchanged(&session, &counts);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "[x] ");
}

#[test]
fn explicit_identical_selection_clears_without_revision_history_or_listener_change() {
    for setter in 0..4 {
        let (mut session, node) = setup();
        let counts = listen(&mut session);
        activate(&mut session);
        let before = Snapshot::capture(&session, &counts);
        let point = point(node, 1, 0, CursorAffinity::Before);
        let outcome = match setter {
            0 => session.set_document_selection(session.selection()),
            1 => session.set_inline_selection(point, point),
            2 => session.apply_intent(&EditIntent::SetSelection {
                anchor: TextPoint::new(node, offset(1), CursorAffinity::Before),
                focus: TextPoint::new(node, offset(1), CursorAffinity::Before),
            }),
            _ => session.apply_intent(&EditIntent::PlaceCaret {
                offset: offset(1),
                extend_selection: false,
            }),
        };
        assert_eq!(outcome, Ok(SessionOutcome::NoChange));
        assert!(!session.input_rule_undo_available());
        assert_eq!(session.document().revision(), before.document.revision());
        assert_eq!(session.document().store(), before.document.store());
        assert_eq!(session.selection(), before.selection);
        assert_eq!(session.history_depths(), before.history);
        assert_eq!(counts.get(), before.counts);
    }
}

#[test]
fn moving_away_and_back_cannot_resurrect_token() {
    let (mut session, node) = setup();
    activate(&mut session);
    session.set_document_selection(caret(node, 0)).unwrap();
    session.set_document_selection(caret(node, 1)).unwrap();
    assert!(!session.input_rule_undo_available());
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "");
}

#[test]
fn proposed_selection_preflight_is_readonly_and_rejected_targets_preserve_token() {
    let (mut session, node) = setup();
    let counts = listen(&mut session);
    activate(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        session.apply_intent_with_selection(
            caret(node, 0),
            &EditIntent::InsertText {
                text: "changed target".into()
            }
        ),
        Ok(SessionOutcome::NoChange)
    );
    before.assert_unchanged(&session, &counts);
    assert!(
        session
            .apply_intent_with_selection(
                caret(node, 0),
                &EditIntent::InsertText {
                    text: "reject".into()
                }
            )
            .is_err()
    );
    before.assert_unchanged(&session, &counts);
    assert!(session.set_document_selection(caret(node, 99)).is_err());
    before.assert_unchanged(&session, &counts);
}

#[test]
fn successful_target_only_publication_clears_token() {
    let (mut session, node) = setup();
    activate(&mut session);
    assert_eq!(
        session.apply_intent_with_selection(
            caret(node, 0),
            &EditIntent::InsertText {
                text: String::new()
            }
        ),
        Ok(SessionOutcome::SelectionChanged)
    );
    assert!(!session.input_rule_undo_available());
}

#[test]
fn readonly_copy_queries_noops_and_stored_marks_preserve_token() {
    let (mut session, _) = setup();
    let counts = listen(&mut session);
    activate(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(session.clipboard_slice().unwrap(), None);
    assert_eq!(session.selected_text(), None);
    session.selection().validate(session.document()).unwrap();
    let _snapshot_for_host_serialization = session.document().clone();
    assert!(session.input_rule_undo_available());
    assert_eq!(insert(&mut session, "noop"), Ok(SessionOutcome::NoChange));
    assert_eq!(insert(&mut session, ""), Ok(SessionOutcome::NoChange));
    assert_eq!(
        session.apply_intent(&EditIntent::MoveCaret {
            caret_move: CaretMove::ToEnd,
            extend_selection: false,
        }),
        Ok(SessionOutcome::NoChange)
    );
    // There is no redo entry; this is an explicitly chosen native no-op boundary.
    assert_eq!(session.redo(), Ok(SessionOutcome::NoChange));
    before.assert_unchanged(&session, &counts);
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert!(session.input_rule_undo_available());
    assert_eq!(session.document().revision(), before.document.revision());
    assert_eq!(session.history_depths(), before.history);
    assert_eq!(counts.get(), before.counts);
}

#[test]
fn normal_raw_empty_and_staged_document_commits_clear_token() {
    for route in 0..3 {
        let (mut session, _) = setup();
        activate(&mut session);
        let revision = session.document().revision();
        let result = match route {
            0 => insert(&mut session, "z"),
            1 => session.apply(&transaction()),
            // List wrapping uses the generic multi-stage transaction pipeline.
            _ => session.apply_intent(&EditIntent::TurnInto {
                kind: NodeKind::BulletList,
            }),
        };
        assert_eq!(result, Ok(SessionOutcome::DocumentChanged));
        assert_ne!(session.document().revision(), revision);
        assert!(!session.input_rule_undo_available());
    }
}

#[test]
fn successful_undo_and_redo_clear_rule_availability() {
    let (mut session, node) = setup();
    activate(&mut session);
    session.undo().unwrap();
    assert_eq!(text(session.document(), node), "[x]");
    assert!(!session.input_rule_undo_available());
    session.redo().unwrap();
    assert_eq!(text(session.document(), node), "R");
    assert!(!session.input_rule_undo_available());
}

#[test]
fn a_second_successful_rule_replaces_rather_than_stacks_metadata() {
    let (document, node) = fixture("[x]");
    let mut rules = Rules::new(restoration_plan(node, "[x]", "first "));
    rules.alternative = Some(restoration_plan(node, "R", "second "));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    activate(&mut session);
    insert(&mut session, "alternative").unwrap();
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "second ");
    assert!(!session.input_rule_undo_available());
}

#[test]
fn forward_and_reverse_failures_preserve_prior_token_and_all_public_state() {
    for failure in 0..8 {
        let (document, node) = fixture("[x]");
        let mut forward = replace(node, 0, 1, "N");
        let mut after = caret(node, 1);
        let mut reverse = replace(node, 0, 1, "next ");
        let mut restored = caret(node, 5);
        match failure {
            0 => forward = replace(node, 0, 99, "N"),
            1 => after = caret(node, 99),
            2 => forward = replace(node, 0, 1, "!"),
            3 => reverse = replace(node, 0, 99, "next "),
            4 => restored = caret(node, 99),
            5 => reverse = replace(node, 0, 1, "!"),
            6 => {
                reverse = replace(node, 0, 1, "🙂");
                // A byte offset inside a UTF-8 scalar is not an exact caret.
                restored = caret(node, 1);
            }
            _ => {
                restored = DocumentSelection::new(
                    point(node, 0, 0, CursorAffinity::Before),
                    point(node, 5, 0, CursorAffinity::After),
                )
            }
        }
        if failure == 5 {
            restored = caret(node, 1);
        }
        let mut spec = InputRuleUndoSpec::new(reverse, restored).unwrap();
        if failure == 7 {
            spec = spec.with_stored_marks(None).unwrap();
        }
        let mut rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
        rules.alternative = Some(exact(forward, after).with_input_rule_undo(spec));
        let mut session =
            DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
        let counts = listen(&mut session);
        activate(&mut session);
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
            .unwrap();
        let before = Snapshot::capture(&session, &counts);
        let result = insert(&mut session, "alternative");
        match failure {
            0 | 3 => assert!(matches!(result, Err(SessionError::Core(_))), "{result:?}"),
            2 | 5 => assert!(matches!(result, Err(SessionError::Policy(_))), "{result:?}"),
            _ => assert_eq!(result, Err(SessionError::SelectionInvalid)),
        }
        before.assert_unchanged(&session, &counts);
        // Equality alone is insufficient: the old reversal must still execute.
        session.apply_intent(&EditIntent::Backspace).unwrap();
        assert_eq!(text(session.document(), node), "[x] ");
        assert!(!session.input_rule_undo_available());
    }
}

#[test]
fn rejected_raw_transaction_and_prepare_preserve_restorable_token() {
    let (mut session, node) = setup();
    let counts = listen(&mut session);
    activate(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert!(session.apply(&replace(node, 0, 99, "bad")).is_err());
    before.assert_unchanged(&session, &counts);
    assert!(insert(&mut session, "reject").is_err());
    before.assert_unchanged(&session, &counts);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "[x] ");
}

#[test]
fn rejected_history_undo_preserves_token_and_history_entry() {
    let (document, node) = fixture("[x]");
    let mut rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
    // This is a fixed pure predicate. Original revision zero is valid;
    // rule reversal at revision two contains the trigger and remains valid.
    rules.rejected_revision = Some((document.revision().as_u64() + 2, "[x]".into()));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    let counts = listen(&mut session);
    activate(&mut session);
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let before = Snapshot::capture(&session, &counts);
    assert!(matches!(session.undo(), Err(SessionError::Policy(_))));
    before.assert_unchanged(&session, &counts);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "[x] ");
    assert_eq!(session.history_depths(), (2, 0));
}

#[test]
fn rejected_history_redo_preserves_document_selection_marks_and_redo_entry() {
    let (document, node) = fixture("[x]");
    let mut rules = Rules::new(restoration_plan(node, "[x]", "[x] "));
    rules.rejected_revision = Some((document.revision().as_u64() + 3, "R".into()));
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(rules)).unwrap();
    let counts = listen(&mut session);
    activate(&mut session);
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let before = Snapshot::capture(&session, &counts);
    assert!(matches!(session.redo(), Err(SessionError::Policy(_))));
    before.assert_unchanged(&session, &counts);
    assert_eq!(session.history_depths(), (0, 1));
}

#[test]
fn restoration_retains_exact_utf8_atom_ordinals_marks_and_existing_identities() {
    let (mut document, node) = fixture("中🙂tail");
    document = transaction()
        .with_step(TransactionStep::AddMark {
            node,
            range: TextRange::new(offset(0), offset(3)).unwrap(),
            mark: Mark::Bold,
        })
        .with_step(TransactionStep::AddMark {
            node,
            range: TextRange::new(offset(3), offset(7)).unwrap(),
            mark: Mark::Italic,
        })
        .with_step(TransactionStep::InsertInlineAtom {
            at: point(node, 7, 0, CursorAffinity::Before),
            kind: AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::hard_break(),
        })
        .with_step(TransactionStep::InsertInlineAtom {
            at: point(node, 7, 1, CursorAffinity::Before),
            kind: AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::hard_break(),
        })
        .apply(&document)
        .unwrap();
    let atom = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()[0]
        .atom();
    document = transaction()
        .with_step(TransactionStep::SetInlineAtomMarks {
            atom,
            marks: MarkSet::new([Mark::Underline]).unwrap(),
        })
        .apply(&document)
        .unwrap();
    let before = DocumentSelection::collapsed(point(node, 7, 1, CursorAffinity::After));
    let forward = transaction().with_step(TransactionStep::ReplaceInlineText {
        at: point(node, 0, 0, CursorAffinity::Before),
        end: offset(7),
        replacement: String::new(),
    });
    let applied = forward.apply_with_changes(&document).unwrap();
    let spec = InputRuleUndoSpec::new(applied.inverse().clone(), before)
        .unwrap()
        .with_stored_marks(Some(MarkSet::new([Mark::Strike]).unwrap()))
        .unwrap();
    let after = DocumentSelection::collapsed(point(node, 0, 1, CursorAffinity::After));
    let plan = exact(forward, after).with_input_rule_undo(spec);
    let mut session =
        DocumentSession::new_with_policy(document.clone(), before, Box::new(Rules::new(plan)))
            .unwrap();
    activate(&mut session);
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64() + 1
    );
    assert_eq!(session.selection(), after);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before);
    assert_eq!(
        session.stored_marks(),
        Some(&MarkSet::new([Mark::Strike]).unwrap())
    );
    let restored = session.document().clone();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.document().store(), restored.store());
    assert!(!session.input_rule_undo_available());
}

#[test]
fn reverse_exact_selection_checks_atom_ordinals_before_publication() {
    let (document, node) = fixture("[x]");
    let spec = InputRuleUndoSpec::new(
        replace(node, 0, 1, "[x] "),
        DocumentSelection::collapsed(point(node, 4, 1, CursorAffinity::After)),
    )
    .unwrap();
    let plan = exact(replace(node, 0, 3, "R"), caret(node, 1)).with_input_rule_undo(spec);
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(Rules::new(plan)))
            .unwrap();
    let counts = listen(&mut session);
    let before = Snapshot::capture(&session, &counts);
    assert_eq!(
        insert(&mut session, "rule"),
        Err(SessionError::SelectionInvalid)
    );
    before.assert_unchanged(&session, &counts);
}

#[test]
fn reversal_exact_selection_preserves_backward_direction_and_affinity() {
    let (document, node) = fixture("中🙂");
    let restored = DocumentSelection::new(
        point(node, 7, 0, CursorAffinity::After),
        point(node, 0, 0, CursorAffinity::Before),
    );
    let spec = InputRuleUndoSpec::new(replace(node, 0, 1, "中🙂"), restored).unwrap();
    let plan = exact(replace(node, 0, 7, "R"), caret(node, 1)).with_input_rule_undo(spec);
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 7), Box::new(Rules::new(plan)))
            .unwrap();
    activate(&mut session);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(session.selection(), restored);
    assert_eq!(text(session.document(), node), "中🙂");
    assert!(!session.input_rule_undo_available());
}

#[test]
fn reversal_stored_marks_distinguish_inheritance_explicit_empty_and_marks() {
    for marks in [
        None,
        Some(MarkSet::empty()),
        Some(MarkSet::new([Mark::Italic]).unwrap()),
    ] {
        let (document, node) = fixture("[x]");
        let spec = InputRuleUndoSpec::new(replace(node, 0, 1, "[x] "), caret(node, 4))
            .unwrap()
            .with_stored_marks(marks.clone())
            .unwrap();
        let plan = exact(replace(node, 0, 3, "R"), caret(node, 1))
            .with_stored_marks(Some(MarkSet::new([Mark::Bold]).unwrap()))
            .with_input_rule_undo(spec);
        let mut session =
            DocumentSession::new_with_policy(document, caret(node, 3), Box::new(Rules::new(plan)))
                .unwrap();
        activate(&mut session);
        assert_eq!(
            session.stored_marks(),
            Some(&MarkSet::new([Mark::Bold]).unwrap())
        );
        session.apply_intent(&EditIntent::Backspace).unwrap();
        assert_eq!(session.stored_marks(), marks.as_ref());
        insert(&mut session, "z").unwrap();
        let inline = session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert_eq!(
            inline.runs().last().unwrap().marks(),
            marks.as_ref().unwrap_or(&MarkSet::empty())
        );
    }
}

#[test]
fn retained_selected_content_can_be_copied_and_serialized_without_invalidating_token() {
    let (document, node) = fixture("[x]");
    let selected = DocumentSelection::new(
        point(node, 0, 0, CursorAffinity::Before),
        point(node, 1, 0, CursorAffinity::After),
    );
    let spec = InputRuleUndoSpec::new(replace(node, 0, 1, "[x] "), caret(node, 4)).unwrap();
    let plan = exact(replace(node, 0, 3, "R"), selected).with_input_rule_undo(spec);
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 3), Box::new(Rules::new(plan)))
            .unwrap();
    let counts = listen(&mut session);
    activate(&mut session);
    let before = Snapshot::capture(&session, &counts);
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "R");
    let encoded = xiaomu_runtime::clipboard::encode_metadata(&slice).unwrap();
    let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    assert!(value.is_object());
    before.assert_unchanged(&session, &counts);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text(session.document(), node), "[x] ");
}

#[test]
fn spec_budget_limits_steps_and_deep_owned_payloads() {
    let (document, node) = fixture("x");
    let mut at_limit = transaction();
    for _ in 0..64 {
        at_limit.push_step(TransactionStep::SetNodeKind {
            node,
            kind: NodeKind::Paragraph,
        });
    }
    assert!(InputRuleUndoSpec::new(at_limit.clone(), caret(node, 0)).is_ok());
    at_limit.push_step(TransactionStep::SetNodeKind {
        node,
        kind: NodeKind::Paragraph,
    });
    assert!(matches!(
        InputRuleUndoSpec::new(at_limit, caret(node, 0)),
        Err(SessionError::InputRuleUndoBudgetExceeded)
    ));

    let oversized = "x".repeat(64 * 1024 + 1);
    let nested_attrs = NodeAttrs::new(BTreeMap::from([(
        "outer".into(),
        AttrValue::List(vec![AttrValue::Object(BTreeMap::from([(
            "inner".into(),
            AttrValue::String(oversized.clone()),
        )]))]),
    )]))
    .unwrap();
    let steps = [
        replace(node, 0, 0, &oversized),
        transaction().with_step(TransactionStep::SetNodeAttrs {
            node,
            attrs: nested_attrs,
        }),
        transaction().with_step(TransactionStep::AddMark {
            node,
            range: TextRange::new(offset(0), offset(1)).unwrap(),
            mark: Mark::Link(LinkMark::new(oversized.clone(), None)),
        }),
        transaction().with_step(TransactionStep::RestoreSubtree {
            parent: document.root(),
            index: 0,
            root: node,
            nodes: vec![document.node(node).unwrap().clone(); 257],
        }),
        Transaction::new(TransactionOrigin::Extension(oversized.clone())),
    ];
    for transaction in steps {
        assert!(matches!(
            InputRuleUndoSpec::new(transaction, caret(node, 0)),
            Err(SessionError::InputRuleUndoBudgetExceeded)
        ));
    }
    let marks = MarkSet::new([Mark::Link(LinkMark::new(oversized, None))]).unwrap();
    assert!(matches!(
        InputRuleUndoSpec::new(transaction(), caret(node, 0))
            .unwrap()
            .with_stored_marks(Some(marks)),
        Err(SessionError::InputRuleUndoBudgetExceeded)
    ));
}
