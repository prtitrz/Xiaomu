//! Native callbacks and range proxies preserve rollback and rejection locality.
//! Virtual GPUI events do not certify OS input or visible host-banner behavior.

use super::*;
use crate::{
    block_view::{ParagraphView, SharedSession},
    editor::{EditorHooks, EditorInstance},
    font_size::FontSizeContext,
    text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider},
};
use gpui::{
    AppContext as _, Entity, EntityInputHandler, Subscription, TestAppContext, WindowHandle,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::{
        InlineContent, Mark, MarkSet, Node, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::InlinePoint,
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, EditIntent, IntentDisposition, PolicyError,
    SessionContext, SessionPolicy,
};

const PRIVATE: &str = "private native input or policy details";
#[derive(Clone, Copy)]
enum Mode {
    Allow,
    Preflight,
    Candidate,
    NoChange,
}
struct Policy(Rc<Cell<Mode>>);
impl SessionPolicy for Policy {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        match self.0.get() {
            Mode::Preflight => Err(PolicyError::new(PRIVATE.repeat(1000))),
            Mode::NoChange => Ok(IntentDisposition::NoChange),
            _ => Ok(IntentDisposition::Continue),
        }
    }
    fn validate_document(&self, _: &XiaomuDocument) -> Result<(), PolicyError> {
        if matches!(self.0.get(), Mode::Candidate) {
            Err(PolicyError::new(PRIVATE.repeat(1000)))
        } else {
            Ok(())
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
struct Style;
impl TextSizeStyleProvider for Style {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            gpui::font(".SystemUIFont"),
            FontSizeContext::new(20.0, 20.0, 20.0).unwrap(),
            1.4,
        )
    }
}
fn paragraph(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("b😀se", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}
struct Mounted {
    window: WindowHandle<DocumentView>,
    entity: Entity<DocumentView>,
    session: SharedSession,
    mode: Rc<Cell<Mode>>,
    counts: Counts,
    image: NodeId,
    cells: [NodeId; 2],
}
fn open(cx: &mut TestAppContext, sized: bool) -> Mounted {
    let mut builder = NodeStoreBuilder::new();
    let node = paragraph(&mut builder);
    let image = builder
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap();
    let cells = std::array::from_fn(|_| {
        let leaf = paragraph(&mut builder);
        builder
            .insert(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([leaf]),
            )
            .unwrap()
    });
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node, image, table]),
        )
        .unwrap();
    let mode = Rc::new(Cell::new(Mode::Allow));
    let counts = Rc::new(Cell::new((0, 0)));
    let editor = EditorInstance::new_with_policy(
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..EditorHooks::default()
        },
        Box::new(Policy(mode.clone())),
    )
    .unwrap();
    let session = editor.session().clone();
    {
        let mut session = session.borrow_mut();
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
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let editor = if sized {
                editor.with_text_size_capability(Rc::new(TextSizeCapability::new(
                    window.text_system().clone(),
                    Rc::new(Style),
                )))
            } else {
                editor
            };
            cx.new(|_| editor.build_view())
        })
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
        mode,
        counts,
        image,
        cells,
    }
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
    let seen = events.clone();
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
            assert!(!format!("{event:?}").contains(PRIVATE));
            assert!(!event.message().contains(PRIVATE));
            assert!(event.message().len() < 128);
            seen.borrow_mut().push(*event);
        })
    });
    (events, subscription)
}
fn input(view: &DocumentView) -> Entity<ParagraphView> {
    view.range_input
        .as_ref()
        .map(|(_, input)| input)
        .unwrap_or(&view.children[0].1)
        .clone()
}
#[derive(Clone, Copy)]
enum Callback {
    Typing,
    Replacement,
    Ime,
    #[cfg(target_os = "linux")]
    Unmark,
}
fn callbacks() -> Vec<Callback> {
    vec![
        Callback::Typing,
        Callback::Replacement,
        Callback::Ime,
        #[cfg(target_os = "linux")]
        Callback::Unmark,
    ]
}
fn perform(
    view: &mut ParagraphView,
    callback: Callback,
    window: &mut gpui::Window,
    cx: &mut Context<ParagraphView>,
) {
    match callback {
        Callback::Typing => view.replace_text_in_range(None, PRIVATE, window, cx),
        Callback::Replacement => view.replace_text_in_range(Some(1..3), PRIVATE, window, cx),
        Callback::Ime => {
            view.replace_and_mark_text_in_range(None, PRIVATE, None, window, cx);
            assert!(view.is_composing());
            view.replace_text_in_range(None, PRIVATE, window, cx);
            assert!(!view.is_composing());
        }
        #[cfg(target_os = "linux")]
        Callback::Unmark => {
            view.replace_and_mark_text_in_range(None, PRIVATE, None, window, cx);
            assert!(view.is_composing());
            view.unmark_text(window, cx);
            assert!(!view.is_composing());
            view.unmark_text(window, cx);
        }
    }
}
fn invoke(m: &Mounted, callback: Callback, cx: &mut TestAppContext) {
    m.window
        .update(cx, |view, window, cx| {
            input(view).update(cx, |view, cx| perform(view, callback, window, cx))
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}
fn assert_event(events: &Events, stage: EditorRejectionStage) {
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].stage(), stage);
    assert_eq!(events.borrow()[0].reason(), EditorRejectionReason::Policy);
}

#[gpui::test]
fn native_callbacks_emit_fresh_policy_feedback_after_rollback(cx: &mut TestAppContext) {
    for sized in [false, true] {
        for mode in [Mode::Preflight, Mode::Candidate] {
            for callback in callbacks() {
                let m = open(cx, sized);
                let before = Snapshot::capture(&m);
                assert_eq!(before.history, (0, 1));
                assert!(before.marks.is_some());
                let (events, _subscription) = watch(&m, Some(before.clone()), cx);
                let item = gpui::ClipboardItem::new_string_with_metadata(
                    "clipboard".into(),
                    "foreign-metadata".into(),
                );
                cx.update(|cx| cx.write_to_clipboard(item.clone()));
                m.mode.set(mode);
                for _ in 0..2 {
                    invoke(&m, callback, cx);
                    assert_event(
                        &events,
                        if sized {
                            EditorRejectionStage::TextSizeInput
                        } else {
                            EditorRejectionStage::NativeInput
                        },
                    );
                    before.assert_session(&m.session, &m.counts);
                    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
                    events.borrow_mut().clear();
                }
            }
        }
    }
}

#[gpui::test]
fn native_success_no_change_preedit_and_cancellation_are_silent(cx: &mut TestAppContext) {
    for sized in [false, true] {
        for callback in callbacks() {
            let m = open(cx, sized);
            let before = Snapshot::capture(&m);
            let (events, _subscription) = watch(&m, None, cx);
            m.mode.set(Mode::NoChange);
            invoke(&m, callback, cx);
            before.assert_session(&m.session, &m.counts);
            m.mode.set(Mode::Preflight);
            m.window
                .update(cx, |view, window, cx| {
                    input(view).update(cx, |view, cx| {
                        view.replace_and_mark_text_in_range(None, "cancelled", None, window, cx);
                        assert!(view.is_composing());
                    })
                })
                .unwrap();
            assert!(events.borrow().is_empty());
            m.window
                .update(cx, |view, window, cx| {
                    input(view).update(cx, |view, cx| {
                        view.replace_and_mark_text_in_range(None, "", None, window, cx);
                        view.unmark_text(window, cx);
                        assert!(!view.is_composing());
                        view.replace_and_mark_text_in_range(None, "cancelled", None, window, cx);
                        view.replace_text_in_range(None, "", window, cx);
                        assert!(!view.is_composing());
                    })
                })
                .unwrap();
            before.assert_session(&m.session, &m.counts);
            assert!(events.borrow().is_empty());
            m.mode.set(Mode::Allow);
            invoke(&m, callback, cx);
            assert_ne!(
                m.session.borrow().document().revision(),
                before.document.revision()
            );
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            assert!(events.borrow().is_empty());
        }
    }
}

#[gpui::test]
fn native_feedback_stays_with_its_editor_and_subscription(cx: &mut TestAppContext) {
    let a = open(cx, false);
    let b = open(cx, false);
    let (a_events, a_subscription) = watch(&a, Some(Snapshot::capture(&a)), cx);
    let (b_events, _b_subscription) = watch(&b, Some(Snapshot::capture(&b)), cx);
    a.mode.set(Mode::Preflight);
    b.mode.set(Mode::Candidate);
    invoke(&a, Callback::Typing, cx);
    assert_event(&a_events, EditorRejectionStage::NativeInput);
    assert!(b_events.borrow().is_empty());
    invoke(&b, Callback::Ime, cx);
    assert_event(&a_events, EditorRejectionStage::NativeInput);
    assert_event(&b_events, EditorRejectionStage::NativeInput);
    drop(a_subscription);
    invoke(&a, Callback::Typing, cx);
    assert_eq!(a_events.borrow().len(), 1);
    assert_eq!(b_events.borrow().len(), 1);
}

#[gpui::test]
fn queued_native_rejections_keep_each_emission_revision_across_success(cx: &mut TestAppContext) {
    let m = open(cx, false);
    let (events, _subscription) = watch(&m, None, cx);
    let first_revision = m.session.borrow().document().revision();
    m.window
        .update(cx, |view, window, cx| {
            input(view).update(cx, |view, cx| {
                m.mode.set(Mode::Preflight);
                perform(view, Callback::Typing, window, cx);
                assert!(events.borrow().is_empty());
                m.mode.set(Mode::Allow);
                view.replace_text_in_range(None, "accepted", window, cx);
                m.mode.set(Mode::Candidate);
                perform(view, Callback::Typing, window, cx);
                assert!(events.borrow().is_empty());
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let current = m.session.borrow().document().revision();
    assert_ne!(first_revision, current);
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(events.borrow()[0].document_revision(), first_revision);
    assert_eq!(events.borrow()[1].document_revision(), current);
    assert!(
        events
            .borrow()
            .iter()
            .all(|event| event.stage() == EditorRejectionStage::NativeInput)
    );
}

#[path = "rejection_range_input_tests.rs"]
mod ranges;
