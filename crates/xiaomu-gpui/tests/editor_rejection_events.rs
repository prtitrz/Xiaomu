//! Optional rejection feedback through mounted GPUI action dispatch.

use gpui::{AppContext as _, Entity, Subscription, TestAppContext, WindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_gpui::{
    block_view::SharedSession,
    document_view::{
        DocumentView, EditorRejection, EditorRejectionReason as Reason,
        EditorRejectionStage as Stage,
    },
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
    editor_commands::{CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter},
};
use xiaomu_runtime::{
    clipboard::{ClipboardExportPurpose, ClipboardExportSpec},
    session::{
        DocumentChangeListener, DocumentSelection, EditIntent, IntentDisposition, PolicyError,
        SessionContext, SessionPolicy,
    },
};

#[path = "rejection_events/cell_range.rs"]
mod cell_range;
#[path = "rejection_events/clipboard.rs"]
mod clipboard;

const PRIVATE: &str = "private clipboard or host data must never be emitted";

#[derive(Clone, Copy)]
enum Mode {
    Allow,
    RejectPrepare,
    RejectCandidate,
    NoChange,
    RejectExport,
}
struct Policy(Rc<Cell<Mode>>);
impl SessionPolicy for Policy {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        match self.0.get() {
            Mode::RejectPrepare => Err(PolicyError::new(PRIVATE.repeat(1000))),
            Mode::NoChange => Ok(IntentDisposition::NoChange),
            _ => Ok(IntentDisposition::Continue),
        }
    }
    fn validate_document(&self, _: &XiaomuDocument) -> Result<(), PolicyError> {
        if matches!(self.0.get(), Mode::RejectCandidate) {
            Err(PolicyError::new(PRIVATE.repeat(1000)))
        } else {
            Ok(())
        }
    }
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        if matches!(self.0.get(), Mode::RejectExport) {
            Err(PolicyError::new(PRIVATE))
        } else {
            Ok(None)
        }
    }
}

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (d, s) = self.0.get();
        self.0.set((d + 1, s));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (d, s) = self.0.get();
        self.0.set((d, s + 1));
    }
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}
fn document() -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = paragraph(&mut builder, "base");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
struct Mounted {
    window: WindowHandle<DocumentView>,
    entity: Entity<DocumentView>,
    session: SharedSession,
    counts: Counts,
}
fn mount(editor: EditorInstance, counts: Counts, cx: &mut TestAppContext) -> Mounted {
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    let entity = window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            cx.entity()
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Mounted {
        window,
        entity,
        session,
        counts,
    }
}
fn configured(cx: &mut TestAppContext) -> (Mounted, Rc<Cell<Mode>>) {
    let (document, node) = document();
    let mode = Rc::new(Cell::new(Mode::Allow));
    let counts = Rc::new(Cell::new((0, 0)));
    let editor = EditorInstance::new_with_policy(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..EditorHooks::default()
        },
        Box::new(Policy(mode.clone())),
    )
    .unwrap();
    {
        let mut session = editor.session().borrow_mut();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".into(),
            })
            .unwrap();
        session.undo().unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
            .unwrap();
    }
    (mount(editor, counts, cx), mode)
}
#[derive(Clone)]
struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    notifications: (usize, usize),
}
impl Snapshot {
    fn capture(m: &Mounted) -> Self {
        let session = m.session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            notifications: m.counts.get(),
        }
    }
    fn assert_session(&self, session: &SharedSession, counts: &Counts) {
        // A subscriber must be able to borrow the same session after rollback.
        let session = session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(counts.get(), self.notifications);
    }
}
type Events = Rc<RefCell<Vec<EditorRejection>>>;
fn watch(
    m: &Mounted,
    expected: Option<Snapshot>,
    cx: &mut TestAppContext,
) -> (Events, Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let captured = events.clone();
    let counts = m.counts.clone();
    let id = m.entity.entity_id();
    let subscription = cx.update(|cx| {
        cx.subscribe(&m.entity, move |emitter, event: &EditorRejection, cx| {
            assert_eq!(emitter.entity_id(), id);
            let session = emitter.read(cx).session();
            if let Some(expected) = &expected {
                expected.assert_session(session, &counts);
                assert_eq!(event.document_revision(), expected.document.revision());
            } else {
                let _readable = session.borrow();
            }
            assert!(event.message().len() < 128);
            assert!(!format!("{event:?}").contains(PRIVATE));
            assert!(!event.message().contains(PRIVATE));
            captured.borrow_mut().push(*event);
        })
    });
    (events, subscription)
}
fn press(m: &Mounted, keys: &str, cx: &mut TestAppContext) {
    cx.simulate_keystrokes(m.window.into(), keys);
    cx.background_executor.run_until_parked();
}
fn install_text(cx: &mut TestAppContext, text: &str) {
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.into())));
}
fn assert_event(events: &Events, stage: Stage, reason: Reason) {
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].stage(), stage);
    assert_eq!(events.borrow()[0].reason(), reason);
}

#[gpui::test]
fn ctrl_v_policy_failures_emit_once_after_rollback_without_change_notifications(
    cx: &mut TestAppContext,
) {
    for rejection in [Mode::RejectPrepare, Mode::RejectCandidate] {
        let (m, mode) = configured(cx);
        let before = Snapshot::capture(&m);
        assert_eq!(before.history, (0, 1));
        assert!(before.marks.is_some());
        let (events, _subscription) = watch(&m, Some(before.clone()), cx);
        mode.set(rejection);
        install_text(cx, PRIVATE);
        press(&m, "ctrl-v", cx);
        assert_event(&events, Stage::Intent, Reason::Policy);
        before.assert_session(&m.session, &m.counts);
        assert_eq!(
            cx.update(|cx| cx.read_from_clipboard().unwrap().text())
                .as_deref(),
            Some(PRIVATE)
        );
    }
}

#[gpui::test]
fn no_change_success_and_direct_paragraph_failure_do_not_emit(cx: &mut TestAppContext) {
    let (m, mode) = configured(cx);
    let (events, _subscription) = watch(&m, None, cx);
    let before = Snapshot::capture(&m);
    mode.set(Mode::NoChange);
    install_text(cx, "no change");
    press(&m, "ctrl-v", cx);
    before.assert_session(&m.session, &m.counts);
    assert!(events.borrow().is_empty());
    mode.set(Mode::RejectCandidate);
    cx.simulate_input(m.window.into(), "direct native input");
    cx.background_executor.run_until_parked();
    before.assert_session(&m.session, &m.counts);
    assert!(
        events.borrow().is_empty(),
        "ParagraphView input is explicitly outside this event contract"
    );
    mode.set(Mode::Allow);
    install_text(cx, "accepted");
    press(&m, "ctrl-v", cx);
    assert_eq!(m.session.borrow().history_depths(), (1, 0));
    assert!(events.borrow().is_empty());
}

struct Router(u8);
impl EditorCommandRouter for Router {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        match self.0 {
            0 => Err(PolicyError::new(PRIVATE)),
            1 => Ok(CommandRoute::NoChange),
            _ => Ok(CommandRoute::Intent(EditIntent::PasteText {
                text: PRIVATE.into(),
            })),
        }
    }
}
#[gpui::test]
fn routing_rejection_and_rejected_routed_intent_each_emit_once_but_no_change_is_silent(
    cx: &mut TestAppContext,
) {
    for decision in 0..3 {
        let (m, mode) = configured(cx);
        m.window
            .update(cx, |view, _, _| {
                view.set_command_router(Some(Rc::new(Router(decision))))
            })
            .unwrap();
        let before = Snapshot::capture(&m);
        let (events, _subscription) = watch(&m, Some(before.clone()), cx);
        mode.set(Mode::RejectCandidate);
        install_text(cx, PRIVATE);
        press(&m, "ctrl-v", cx);
        if decision == 1 {
            assert!(events.borrow().is_empty());
        } else {
            assert_event(
                &events,
                if decision == 0 {
                    Stage::CommandRouting
                } else {
                    Stage::Intent
                },
                Reason::Policy,
            );
        }
        before.assert_session(&m.session, &m.counts);
    }
}

#[gpui::test]
fn subscriptions_are_per_view_and_dropping_one_disables_only_its_feedback(cx: &mut TestAppContext) {
    let (first, first_mode) = configured(cx);
    let (second, second_mode) = configured(cx);
    let (a, subscription_a) = watch(&first, Some(Snapshot::capture(&first)), cx);
    let (b, _subscription_b) = watch(&second, Some(Snapshot::capture(&second)), cx);
    first_mode.set(Mode::RejectPrepare);
    second_mode.set(Mode::RejectPrepare);
    install_text(cx, "rejected");
    press(&first, "ctrl-v", cx);
    assert_event(&a, Stage::Intent, Reason::Policy);
    assert!(b.borrow().is_empty());
    press(&second, "ctrl-v", cx);
    assert_event(&a, Stage::Intent, Reason::Policy);
    assert_event(&b, Stage::Intent, Reason::Policy);
    drop(subscription_a);
    press(&first, "ctrl-v", cx);
    assert_eq!(a.borrow().len(), 1);
    assert_eq!(b.borrow().len(), 1);
}
