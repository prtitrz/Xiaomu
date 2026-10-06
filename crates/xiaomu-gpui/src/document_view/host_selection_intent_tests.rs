//! Mounted atomic targeting, refusal and native focus/history integration.
use super::*;
use crate::{
    block_view::SharedSession,
    editor::{EditorHooks, EditorInstance},
    history_clock::HistoryClock,
};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{
        InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    },
    selection::{InlinePoint, NodeGap},
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::{
    DocumentChangeListener, EditPlan, HistoryTimestamp, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionPolicy,
};

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (edits, selections) = self.0.get();
        self.0.set((edits + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (edits, selections) = self.0.get();
        self.0.set((edits, selections + 1));
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Allow,
    ReadOnly,
    NoChange,
    InvalidAfter,
    RejectCandidate,
}
struct Policy {
    mode: Mode,
    original: DocumentSelection,
    target: NodeId,
    after: DocumentSelection,
}
impl SessionPolicy for Policy {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if !matches!(intent, EditIntent::Delete) {
            return Ok(IntentDisposition::Continue);
        }
        assert_eq!(context.original_selection(), self.original);
        if context.selection().as_node_selection() != Some(self.target) {
            return Ok(IntentDisposition::NoChange);
        }
        assert_eq!(
            context.stored_marks(),
            None,
            "target inherits its own marks"
        );
        match self.mode {
            Mode::ReadOnly => Err(PolicyError::new("read-only document")),
            Mode::NoChange => Ok(IntentDisposition::NoChange),
            _ => Ok(IntentDisposition::Apply(EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput)
                    .with_step(TransactionStep::RemoveNode { node: self.target }),
                SelectionUpdate::Exact {
                    selection: if matches!(self.mode, Mode::InvalidAfter) {
                        context.original_selection()
                    } else {
                        self.after
                    },
                },
                None,
            ))),
        }
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if matches!(self.mode, Mode::RejectCandidate) && document.node(self.target).is_none() {
            Err(PolicyError::new("candidate rejected"))
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct Clock(Cell<usize>);
impl HistoryClock for Clock {
    fn now(&self) -> HistoryTimestamp {
        self.0.set(self.0.get() + 1);
        HistoryTimestamp::from_millis(1000)
    }
}

struct Opened {
    handle: WindowHandle<DocumentView>,
    session: SharedSession,
    counts: Counts,
    clock: Rc<Clock>,
    document: XiaomuDocument,
    original: DocumentSelection,
    target: DocumentSelection,
    after: DocumentSelection,
    inside: NodeId,
    outside: NodeId,
    container: NodeId,
}
fn open(cx: &mut TestAppContext, mode: Mode, stamped: bool) -> Opened {
    let mut builder = NodeStoreBuilder::new();
    let mut paragraph = || {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("text", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    };
    let inside = paragraph();
    let outside = paragraph();
    let container = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([inside]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([container, outside]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let original = DocumentSelection::collapsed(InlinePoint::at_start_of(inside));
    let target = DocumentSelection::node(&document, container).unwrap();
    let after = DocumentSelection::collapsed(InlinePoint::at_start_of(outside));
    let counts = Rc::new(Cell::new((0, 0)));
    let clock = Rc::new(Clock::default());
    let editor = EditorInstance::new_with_policy_and_history_clock(
        document.clone(),
        original,
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..Default::default()
        },
        Box::new(Policy {
            mode,
            original,
            target: container,
            after,
        }),
        clock.clone(),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| {
                if stamped {
                    editor.build_view()
                } else {
                    DocumentView::new(session.clone())
                }
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Opened {
        handle,
        session,
        counts,
        clock,
        document,
        original,
        target,
        after,
        inside,
        outside,
        container,
    }
}

fn assert_unchanged(opened: &Opened) {
    let session = opened.session.borrow();
    assert_eq!(session.document().store(), opened.document.store());
    assert_eq!(session.document().revision(), opened.document.revision());
    assert_eq!(session.selection(), opened.original);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        opened.counts.get(),
        (0, 0),
        "no dirty or selection publication"
    );
}

#[gpui::test]
fn atomic_host_target_uses_exact_plan_and_original_undo_selection(cx: &mut TestAppContext) {
    for stamped in [false, true] {
        let opened = open(cx, Mode::Allow, stamped);
        opened
            .handle
            .update(cx, |view, window, cx| {
                let epoch = view.epoch.get();
                // Existing API still applies at current selection: no applicable target.
                view.apply_edit_intent(EditIntent::Delete, window, cx);
                assert_unchanged(&opened);
                assert_eq!(view.epoch.get(), epoch);
                view.apply_edit_intent_with_selection(
                    opened.target,
                    EditIntent::Delete,
                    window,
                    cx,
                );
                assert_eq!(view.epoch.get(), epoch + 1);
                assert!(view.children.iter().all(|(node, _)| *node != opened.inside));
                assert_eq!(
                    view.accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner(),
                    Some(opened.outside)
                );
            })
            .unwrap();
        let after_document = opened.session.borrow().document().clone();
        assert!(after_document.node(opened.container).is_none());
        assert_eq!(opened.session.borrow().selection(), opened.after);
        assert_eq!(opened.session.borrow().history_depths(), (1, 0));
        assert_eq!(
            opened.counts.get(),
            (1, 0),
            "never publishes a target-selection move"
        );
        assert_eq!(opened.clock.0.get(), if stamped { 2 } else { 0 });
        opened.session.borrow_mut().undo().unwrap();
        assert_eq!(
            opened.session.borrow().document().store(),
            opened.document.store()
        );
        assert_eq!(opened.session.borrow().selection(), opened.original);
        opened.session.borrow_mut().redo().unwrap();
        assert_eq!(
            opened.session.borrow().document().store(),
            after_document.store()
        );
        assert_eq!(opened.session.borrow().selection(), opened.after);
    }
}

#[gpui::test]
fn atomic_host_refusals_preserve_canonical_state_marks_history_and_epoch(cx: &mut TestAppContext) {
    for mode in [
        Mode::ReadOnly,
        Mode::NoChange,
        Mode::InvalidAfter,
        Mode::RejectCandidate,
    ] {
        let opened = open(cx, mode, true);
        opened
            .session
            .borrow_mut()
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        let marks = opened.session.borrow().stored_marks().cloned();
        opened
            .handle
            .update(cx, |view, window, cx| {
                let epoch = view.epoch.get();
                view.apply_edit_intent_with_selection(
                    opened.target,
                    EditIntent::Delete,
                    window,
                    cx,
                );
                assert_eq!(view.epoch.get(), epoch);
            })
            .unwrap();
        assert_unchanged(&opened);
        assert_eq!(opened.session.borrow().stored_marks(), marks.as_ref());
        assert_eq!(opened.clock.0.get(), 1);
    }
}

#[gpui::test]
fn invalid_host_target_is_rejected_before_policy_without_selection_move(cx: &mut TestAppContext) {
    use crate::document_view::{EditorRejection, EditorRejectionStage};
    for measured in [false, true] {
        let opened = open(cx, Mode::Allow, true);
        let entity = opened
            .handle
            .update(cx, |view, _, cx| {
                view.set_measured_table_layout(measured);
                cx.entity()
            })
            .unwrap();
        let events = Rc::new(Cell::new(0));
        let seen = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&entity, move |_, event: &EditorRejection, _| {
                assert_eq!(event.stage(), EditorRejectionStage::Intent);
                seen.set(seen.get() + 1);
            })
        });
        let invalid = DocumentSelection::collapsed(NodeGap::new(opened.document.root(), 99));
        opened
            .handle
            .update(cx, |view, window, cx| {
                let epoch = view.epoch.get();
                view.apply_edit_intent_with_selection(invalid, EditIntent::Delete, window, cx);
                assert_eq!(view.epoch.get(), epoch);
            })
            .unwrap();
        assert_unchanged(&opened);
        assert_eq!(
            events.get(),
            1,
            "invalid targets always use Runtime rejection"
        );
    }
}

#[gpui::test]
fn atomic_host_target_preserves_active_composition_and_does_not_sample(cx: &mut TestAppContext) {
    let opened = open(cx, Mode::Allow, true);
    opened
        .handle
        .update(cx, |view, window, cx| {
            let child = view
                .children
                .iter()
                .find(|(node, _)| *node == opened.inside)
                .unwrap()
                .1
                .clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            });
            let epoch = view.epoch.get();
            view.apply_edit_intent_with_selection(opened.target, EditIntent::Delete, window, cx);
            assert_eq!(view.epoch.get(), epoch);
            assert_eq!(opened.clock.0.get(), 0);
            assert!(view.has_active_composition(cx));
            assert_unchanged(&opened);
            child.update(cx, |child, cx| {
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
            });
        })
        .unwrap();
}

#[gpui::test]
fn accepted_selection_only_target_restores_native_focus(cx: &mut TestAppContext) {
    let opened = open(cx, Mode::Allow, true);
    opened
        .handle
        .update(cx, |view, window, cx| {
            let epoch = view.epoch.get();
            view.apply_edit_intent_with_selection(
                opened.after,
                EditIntent::InsertText {
                    text: String::new(),
                },
                window,
                cx,
            );
            assert_eq!(view.epoch.get(), epoch + 1);
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(opened.outside)
            );
        })
        .unwrap();
    assert_eq!(opened.session.borrow().selection(), opened.after);
    assert_eq!(
        opened.session.borrow().document().store(),
        opened.document.store()
    );
    assert_eq!(opened.session.borrow().history_depths(), (0, 0));
    assert_eq!(opened.counts.get(), (0, 1));
}

#[gpui::test]
fn accepted_range_targets_materialize_and_focus_the_correct_proxy(cx: &mut TestAppContext) {
    let opened = open(cx, Mode::Allow, true);
    opened
        .handle
        .update(cx, |view, window, cx| {
            for target in [
                DocumentSelection::all(&opened.document),
                DocumentSelection::node(&opened.document, opened.outside).unwrap(),
                opened.target,
            ] {
                view.apply_edit_intent_with_selection(
                    target,
                    EditIntent::MergeTableCells,
                    window,
                    cx,
                );
                assert_eq!(opened.session.borrow().selection(), target);
                assert!(view.range_input.is_some());
                assert!(view.range_input_is_focused(window, cx));
            }
        })
        .unwrap();
    assert_eq!(
        opened.session.borrow().document().store(),
        opened.document.store()
    );
    assert_eq!(opened.session.borrow().history_depths(), (0, 0));
    assert_eq!(opened.counts.get(), (0, 3));
}
