//! Synthetic host policy exercised through the actual clipboard and Ctrl-V route.
use super::*;
use crate::{
    block_view::SharedSession,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
    editor_commands::{
        CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter, ImagePasteAdmission,
    },
};
use gpui::{AppContext as _, ClipboardItem, TestAppContext, WindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::{
    document::{
        ImageAttrs, ImageSource, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink},
    session::{
        DocumentChangeListener, DocumentSelection, EditPlan, IntentDisposition, PolicyError,
        SelectionUpdate, SessionContext, SessionPolicy,
    },
};

#[derive(Clone, Copy)]
enum Decision {
    Default,
    Allow,
    Reject,
}
#[derive(Clone, Copy)]
enum Target {
    Caret,
    Range,
    All,
    Node,
    Code,
}
#[derive(Default)]
struct Seen {
    admitted: Vec<(DocumentSelection, Option<MarkSet>)>,
    planned: Vec<DocumentSelection>,
    planned_marks: Vec<Option<MarkSet>>,
    imports: usize,
    publications: usize,
    ordinary: usize,
    reject_candidate: bool,
}
struct Router {
    decision: Decision,
    seen: Rc<RefCell<Seen>>,
}
impl EditorCommandRouter for Router {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        self.seen.borrow_mut().ordinary += 1;
        Ok(CommandRoute::Default)
    }
    fn admit_image_paste(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<ImagePasteAdmission, PolicyError> {
        self.seen
            .borrow_mut()
            .admitted
            .push((context.selection(), context.stored_marks().cloned()));
        match self.decision {
            Decision::Default => Ok(ImagePasteAdmission::Default),
            Decision::Allow => Ok(ImagePasteAdmission::Allow),
            Decision::Reject => Err(PolicyError::new("private synthetic admission refusal")),
        }
    }
}
struct Planner {
    seen: Rc<RefCell<Seen>>,
    tail: NodeId,
    reject: bool,
}
impl SessionPolicy for Planner {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.seen.borrow().reject_candidate
            && document
                .store()
                .iter()
                .any(|node| node.kind() == &NodeKind::Image)
        {
            return Err(PolicyError::new("synthetic final candidate refusal"));
        }
        Ok(())
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let EditIntent::InsertImage { image } = intent else {
            return Ok(IntentDisposition::Continue);
        };
        assert_eq!(context.selection(), context.original_selection());
        self.seen.borrow_mut().planned.push(context.selection());
        self.seen
            .borrow_mut()
            .planned_marks
            .push(context.stored_marks().cloned());
        if self.reject {
            return Err(PolicyError::new("synthetic planner refusal"));
        }
        let transaction =
            Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertNode {
                parent: context.document().root(),
                index: 0,
                kind: NodeKind::Image,
                attrs: image.to_attrs().unwrap(),
                content: NodeContent::Atomic,
            });
        Ok(IntentDisposition::Apply(EditPlan::new(
            transaction,
            SelectionUpdate::Exact {
                selection: DocumentSelection::collapsed(InlinePoint::at_start_of(self.tail)),
            },
            None,
        )))
    }
}
struct Host {
    seen: Rc<RefCell<Seen>>,
    during_import: RefCell<Option<Box<dyn FnOnce()>>>,
    remote: Cell<bool>,
}
impl AssetService for Host {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(&self, format: AssetFormat, bytes: &[u8]) -> Result<ImageAttrs, AssetError> {
        assert_eq!(format, AssetFormat::Png);
        assert_eq!(bytes, [1, 2, 3]);
        self.seen.borrow_mut().imports += 1;
        if let Some(run) = self.during_import.borrow_mut().take() {
            run();
        }
        ImageAttrs::new(
            if self.remote.get() {
                ImageSource::ExternalUrl("https://example.com/image.png".into())
            } else {
                ImageSource::AssetRef("synthetic-image".into())
            },
            "synthetic".into(),
            None,
            None,
            None,
        )
        .map_err(|_| AssetError::InvalidImage)
    }
}
struct Listener(Rc<RefCell<Seen>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.borrow_mut().publications += 1;
    }
}
struct Mounted {
    window: WindowHandle<DocumentView>,
    session: SharedSession,
    host: Rc<Host>,
    seen: Rc<RefCell<Seen>>,
    head: NodeId,
}
fn mount(
    cx: &mut TestAppContext,
    target: Target,
    decision: Decision,
    read_only: bool,
    reject: bool,
) -> Mounted {
    let mut builder = NodeStoreBuilder::new();
    let head = builder
        .insert(
            if matches!(target, Target::Code) {
                NodeKind::CodeBlock
            } else {
                NodeKind::Paragraph
            },
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("abc", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let tail = builder
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
            NodeContent::children([head, tail]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let point = |offset| {
        InlinePoint::new(
            head,
            document
                .node(head)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at(offset)
                .unwrap(),
            0,
            CursorAffinity::After,
        )
    };
    let selection = match target {
        Target::Range => DocumentSelection::new(point(3), point(1)),
        Target::All => DocumentSelection::all(&document),
        Target::Node => DocumentSelection::node(&document, head).unwrap(),
        _ => DocumentSelection::collapsed(point(1)),
    };
    let seen = Rc::new(RefCell::new(Seen::default()));
    let host = Rc::new(Host {
        seen: seen.clone(),
        during_import: RefCell::new(None),
        remote: Cell::new(false),
    });
    let editor = EditorInstance::new_with_policy(
        document,
        selection,
        EditorHooks {
            listener: Some(Box::new(Listener(seen.clone()))),
            asset_service: Some(host.clone()),
            ..Default::default()
        },
        Box::new(Planner {
            seen: seen.clone(),
            tail,
            reject,
        }),
    )
    .unwrap()
    .with_command_router(Rc::new(Router {
        decision,
        seen: seen.clone(),
    }));
    let editor = if read_only {
        editor.with_read_only_input()
    } else {
        editor
    };
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Mounted {
        window,
        session,
        host,
        seen,
        head,
    }
}
fn image_clipboard(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![1, 2, 3],
        )))
    });
}
fn paste(m: &Mounted, cx: &mut TestAppContext) {
    image_clipboard(cx);
    cx.simulate_keystrokes(m.window.into(), "ctrl-v");
    cx.background_executor.run_until_parked();
}
fn events(
    m: &Mounted,
    cx: &mut TestAppContext,
) -> (
    Rc<RefCell<Vec<super::super::EditorRejection>>>,
    gpui::Subscription,
) {
    let entity = m.window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(
            &entity,
            move |emitter, event: &super::super::EditorRejection, cx| {
                assert!(emitter.read(cx).session().try_borrow_mut().is_ok());
                seen.borrow_mut().push(*event);
            },
        )
    });
    (events, subscription)
}

#[gpui::test]
fn image_admission_allow_preserves_original_range_all_node_and_one_undo(cx: &mut TestAppContext) {
    for target in [Target::Range, Target::All, Target::Node, Target::Code] {
        let m = mount(cx, target, Decision::Allow, false, false);
        let document = m.session.borrow().document().clone();
        let selection = m.session.borrow().selection();
        paste(&m, cx);
        assert_eq!(m.seen.borrow().admitted, [(selection, None)]);
        assert_eq!(m.seen.borrow().planned, [selection]);
        assert_eq!(m.seen.borrow().imports, 1);
        assert_eq!(m.seen.borrow().publications, 1);
        assert_eq!(m.seen.borrow().ordinary, 0);
        assert_eq!(m.session.borrow().history_depths(), (1, 0));
        m.session.borrow_mut().undo().unwrap();
        assert_eq!(m.session.borrow().document().store(), document.store());
        assert_eq!(m.session.borrow().selection(), selection);
    }
}
#[gpui::test]
fn image_admission_default_keeps_legacy_target_boundary(cx: &mut TestAppContext) {
    for target in [
        Target::Caret,
        Target::Range,
        Target::All,
        Target::Node,
        Target::Code,
    ] {
        let m = mount(cx, target, Decision::Default, false, false);
        let before = m.session.borrow().document().clone();
        paste(&m, cx);
        let allowed = usize::from(matches!(target, Target::Caret));
        assert_eq!(m.seen.borrow().imports, allowed);
        assert_eq!(m.seen.borrow().planned.len(), allowed);
        if allowed == 0 {
            assert_eq!(m.session.borrow().document().store(), before.store());
        }
    }
}
#[gpui::test]
fn image_admission_rejection_is_deferred_payload_free_and_preimport(cx: &mut TestAppContext) {
    let m = mount(cx, Target::Range, Decision::Reject, false, false);
    let before = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    let (events, _subscription) = events(&m, cx);
    image_clipboard(cx);
    m.window
        .update(cx, |view, window, cx| {
            view.paste(&ClipboardPaste, window, cx);
            assert!(events.borrow().is_empty());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(
        events.borrow()[0].stage(),
        EditorRejectionStage::CommandRouting
    );
    assert_eq!(events.borrow()[0].reason(), EditorRejectionReason::Policy);
    assert!(!format!("{:?}", events.borrow()[0]).contains("private synthetic"));
    assert_eq!(m.seen.borrow().imports, 0);
    assert!(m.seen.borrow().planned.is_empty());
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}
#[gpui::test]
fn image_admission_read_only_and_non_asset_import_do_not_plan(cx: &mut TestAppContext) {
    for read_only in [false, true] {
        let m = mount(cx, Target::Range, Decision::Allow, read_only, false);
        m.host.remote.set(true);
        paste(&m, cx);
        assert_eq!(m.seen.borrow().imports, usize::from(!read_only));
        assert_eq!(m.seen.borrow().admitted.len(), usize::from(!read_only));
        assert!(m.seen.borrow().planned.is_empty());
        assert_eq!(m.session.borrow().history_depths(), (0, 0));
    }
}
#[gpui::test]
fn image_admission_cannot_bypass_session_policy(cx: &mut TestAppContext) {
    let m = mount(cx, Target::All, Decision::Allow, false, true);
    let before = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    paste(&m, cx);
    assert_eq!(m.seen.borrow().imports, 1);
    assert_eq!(m.seen.borrow().planned, [selection]);
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}
#[path = "clipboard_image_admission_stale.rs"]
mod stale;

#[path = "clipboard_image_admission_guards.rs"]
mod guards;

#[gpui::test]
fn image_admission_observes_stored_marks_without_changing_them_before_planning(
    cx: &mut TestAppContext,
) {
    let m = mount(cx, Target::Caret, Decision::Allow, false, false);
    m.session
        .borrow_mut()
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let selection = m.session.borrow().selection();
    let marks = m.session.borrow().stored_marks().cloned();
    paste(&m, cx);
    assert_eq!(m.seen.borrow().admitted, [(selection, marks.clone())]);
    assert_eq!(m.seen.borrow().planned, [selection]);
    m.session.borrow_mut().undo().unwrap();
    assert_eq!(m.session.borrow().selection(), selection);
    // Existing Undo semantics clear explicit typing marks. The admission hook
    // preserves them through planning, not across a later history command.
    assert_eq!(m.seen.borrow().planned_marks, [marks]);
    assert_eq!(m.session.borrow().stored_marks(), None);
}

#[path = "clipboard_image_admission_cells.rs"]
mod cells;
