//! Borrowed replacement attrs affect only the admitted All destination paragraph.

use super::rejections::{assert_cut_rejection, capture_rejections};
use super::*;
use crate::document_view::EditorRejectionReason;

#[derive(Clone, Copy)]
enum Failure {
    None,
    Hook,
    Budget,
    Candidate,
}

type HookCalls = Rc<RefCell<Vec<(DocumentSelection, usize)>>>;

struct ParagraphAttrsPolicy {
    base: Policy,
    attrs: NodeAttrs,
    failure: Failure,
    hooks: HookCalls,
}

impl SessionPolicy for ParagraphAttrsPolicy {
    fn prepare_text_cut(&self, context: SessionContext<'_>) -> Result<bool, PolicyError> {
        self.base.prepare_text_cut(context)
    }
    fn clipboard_export_spec(
        &self,
        context: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.base.clipboard_export_spec(context, purpose)
    }
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.base.prepare_intent(context, intent)
    }
    fn text_cut_all_paragraph_attrs(
        &self,
        context: SessionContext<'_>,
    ) -> Result<Option<&NodeAttrs>, PolicyError> {
        assert!(context.selection().is_all(context.document()));
        assert_eq!(context.selection(), context.original_selection());
        assert!(context.document().node(self.base.head).is_some());
        self.hooks
            .borrow_mut()
            .push((context.selection(), write_observation::calls()));
        if matches!(self.failure, Failure::Hook) {
            return Err(PolicyError::new("synthetic replacement attrs refusal"));
        }
        Ok(Some(&self.attrs))
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.node(self.base.head).is_none() {
            let roots = document
                .node(document.root())
                .unwrap()
                .content()
                .as_children()
                .unwrap();
            assert_eq!(roots.len(), 1);
            let replacement = document.node(roots[0]).unwrap();
            assert_eq!(replacement.kind(), &NodeKind::Paragraph);
            assert_eq!(replacement.attrs(), &self.attrs);
            assert_eq!(replacement.content(), &NodeContent::empty_inline());
        }
        self.base.validate_document(document)
    }
}

fn replacement_attrs(failure: Failure) -> NodeAttrs {
    if matches!(failure, Failure::Budget) {
        let mut value = AttrValue::String("bounded before cloning".into());
        for _ in 0..40 {
            value = AttrValue::List(vec![value]);
        }
        return NodeAttrs::new([("payload".into(), value)].into()).unwrap();
    }
    NodeAttrs::new(
        [
            (
                "label".into(),
                AttrValue::String("new empty paragraph".into()),
            ),
            ("nullable".into(), AttrValue::Null),
            ("level".into(), AttrValue::Integer(0)),
            ("flag".into(), AttrValue::Bool(false)),
        ]
        .into(),
    )
    .unwrap()
}

fn mount_attrs(cx: &mut TestAppContext, f: &Fixture, failure: Failure) -> (Mounted, HookCalls) {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let gate = Rc::new(Cell::new(true));
    let hooks = Rc::new(RefCell::new(Vec::new()));
    let editor = EditorInstance::new_with_policy(
        f.document.clone(),
        DocumentSelection::collapsed(point(f, f.head, 0)),
        EditorHooks {
            listener: Some(Box::new(Listener(calls.clone()))),
            ..Default::default()
        },
        Box::new(ParagraphAttrsPolicy {
            base: Policy {
                calls: calls.clone(),
                gate: gate.clone(),
                reject: matches!(failure, Failure::Candidate),
                head: f.head,
                original: f
                    .document
                    .node(f.head)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .clone(),
            },
            attrs: replacement_attrs(failure),
            failure,
            hooks: hooks.clone(),
        }),
    )
    .unwrap()
    .with_command_router(Rc::new(AllRouter));
    let session = editor.session().clone();
    {
        let mut session = session.borrow_mut();
        session
            .set_document_selection(DocumentSelection::collapsed(point(f, f.tail, 0)))
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".into(),
            })
            .unwrap();
        session.undo().unwrap();
        session
            .set_document_selection(DocumentSelection::collapsed(point(f, f.head, 0)))
            .unwrap();
    }
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
    *calls.borrow_mut() = Calls::default();
    let m = Mounted {
        window,
        session,
        calls,
        gate,
    };
    let before = Snapshot::capture(&m);
    key(&m, cx, "ctrl-a");
    assert_eq!(
        m.session.borrow().selection(),
        DocumentSelection::all(&f.document)
    );
    assert_eq!(
        m.session.borrow().document().store(),
        before.document.store()
    );
    assert_eq!(
        m.session.borrow().document().revision(),
        before.document.revision()
    );
    assert_eq!(m.session.borrow().history_depths(), before.history);
    assert_eq!(m.calls.borrow().selections, 1);
    assert!(hooks.borrow().is_empty());
    *m.calls.borrow_mut() = Calls::default();
    (m, hooks)
}

#[gpui::test]
fn all_text_cut_custom_paragraph_attrs_keys_preserve_source_and_history(cx: &mut TestAppContext) {
    let f = rich_fixture(None, 1);
    let (m, hooks) = mount_attrs(cx, &f, Failure::None);
    let before = Snapshot::capture(&m);
    previous(cx);
    key(&m, cx, "ctrl-x");
    let item = clipboard(cx);
    let slice = decode_metadata(&item.text().unwrap(), item.metadata().unwrap()).unwrap();
    assert!(slice.is_closed());
    assert_eq!(
        slice.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert_eq!(slice.roots().len(), 3);
    for (copy, id) in slice.roots().iter().zip(
        f.document
            .node(f.document.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap(),
    ) {
        assert_original_node(&f.document, *id, copy);
    }
    assert_eq!(prepare_lossless_slice(&slice).unwrap(), item);
    let after = Snapshot::capture(&m);
    assert_eq!(after.document.node_count(), 2);
    assert_eq!(after.document.root(), before.document.root());
    let (start, end) = after.selection.as_same_node_inline().unwrap();
    assert_eq!(start, end);
    assert_eq!(start, InlinePoint::at_start_of(start.node_id()));
    let replacement = after.document.node(start.node_id()).unwrap();
    assert_eq!(replacement.kind(), &NodeKind::Paragraph);
    assert_eq!(replacement.attrs(), &replacement_attrs(Failure::None));
    assert_eq!(replacement.content(), &NodeContent::empty_inline());
    assert!(before.document.node(start.node_id()).is_none());
    assert_eq!(after.history, (1, 0));
    assert_eq!(after.marks, None);
    assert_eq!(*hooks.borrow(), [(before.selection, 0)]);
    assert_eq!(
        m.calls.borrow().sources,
        [before.selection, before.selection]
    );
    assert_eq!(m.calls.borrow().candidates, [0]);
    assert_eq!(m.calls.borrow().publications, [1]);
    assert_eq!(m.calls.borrow().selections, 0);
    assert_eq!(m.calls.borrow().deletes, 0);
    key(&m, cx, "ctrl-z");
    assert_eq!(
        m.session.borrow().document().store(),
        before.document.store()
    );
    assert_eq!(m.session.borrow().selection(), before.selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 1));
    key(&m, cx, "ctrl-shift-z");
    assert_eq!(
        m.session.borrow().document().store(),
        after.document.store()
    );
    assert_eq!(m.session.borrow().selection(), after.selection);
    assert_eq!(m.session.borrow().history_depths(), (1, 0));
    assert_eq!(
        *hooks.borrow(),
        [(before.selection, 0)],
        "history never replans attrs"
    );
    assert_eq!(clipboard(cx), item);
    assert_eq!(write_observation::calls(), 1);
}

#[gpui::test]
fn all_text_cut_custom_paragraph_attrs_refusals_never_write(cx: &mut TestAppContext) {
    for failure in [Failure::Hook, Failure::Budget, Failure::Candidate] {
        let f = rich_fixture(None, 1);
        let (m, hooks) = mount_attrs(cx, &f, failure);
        let before = Snapshot::capture(&m);
        let (events, _subscription) = capture_rejections(&m, cx);
        let sentinel = previous(cx);
        key(&m, cx, "ctrl-x");
        assert_cut_rejection(&events, EditorRejectionReason::Policy);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(*hooks.borrow(), [(before.selection, 0)]);
        assert_eq!(
            m.calls.borrow().sources,
            [before.selection, before.selection]
        );
        assert_eq!(
            m.calls.borrow().candidates,
            if matches!(failure, Failure::Candidate) {
                vec![0]
            } else {
                vec![]
            }
        );
        assert_eq!(m.calls.borrow().deletes, 0);
    }
}

#[gpui::test]
fn all_text_cut_custom_paragraph_attrs_hook_follows_source_admission(cx: &mut TestAppContext) {
    for (barrier, depth, expired) in [
        (None, 160, false),
        (Some(NodeKind::Table), 1, false),
        (Some(NodeKind::Quote), 1, false),
        (Some(NodeKind::Image), 1, false),
        (None, 1, true),
    ] {
        let f = rich_fixture(barrier, depth);
        let (m, hooks) = mount_attrs(cx, &f, Failure::Hook);
        let before = Snapshot::capture(&m);
        let (events, _subscription) = capture_rejections(&m, cx);
        let sentinel = previous(cx);
        m.gate.set(!expired);
        key(&m, cx, "ctrl-x");
        assert_cut_rejection(&events, EditorRejectionReason::Policy);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert!(hooks.borrow().is_empty());
        assert!(m.calls.borrow().candidates.is_empty());
        assert_eq!(m.calls.borrow().deletes, 0);
    }
}
