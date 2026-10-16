//! Synthetic prepared text Cut through GPUI's real coordinator and stock writer.
//! Writer observations count calls, not OS clipboard ownership acknowledgments.

#[path = "clipboard_cut_text_tests/all.rs"]
mod all;

use crate::block_view::{ClipboardCut, SharedSession};
use crate::document_view::DocumentView;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::input::platform_clipboard::{prepare_lossless_slice, write_observation};
use gpui::{AppContext as _, ClipboardItem, TestAppContext, WindowHandle};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::{
    ClipboardExportPurpose, ClipboardExportSpec, ClipboardTextProjection, decode_metadata,
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, EditIntent, IntentDisposition, PolicyError,
    SessionContext, SessionPolicy,
};

#[derive(Default)]
struct Calls {
    gates: usize,
    export_specs: usize,
    deletes: usize,
    candidates: Vec<usize>,
    publications: Vec<usize>,
    selections: usize,
    sources: Vec<DocumentSelection>,
}

struct Policy {
    calls: Rc<RefCell<Calls>>,
    gate: Rc<Cell<bool>>,
    reject: bool,
    head: NodeId,
    original: InlineContent,
}
impl SessionPolicy for Policy {
    fn prepare_text_cut(&self, context: SessionContext<'_>) -> Result<bool, PolicyError> {
        self.calls.borrow_mut().gates += 1;
        self.calls.borrow_mut().sources.push(context.selection());
        if !self.gate.get() {
            return Err(PolicyError::new("synthetic source permission expired"));
        }
        Ok(true)
    }
    fn clipboard_export_spec(
        &self,
        context: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(purpose, ClipboardExportPurpose::Cut);
        self.calls.borrow_mut().export_specs += 1;
        self.calls.borrow_mut().sources.push(context.selection());
        Ok(Some(
            ClipboardExportSpec::new().with_text_projection(
                ClipboardTextProjection::text_between_lf_with_atoms([AtomKind::new(
                    "example.token.v1",
                )
                .unwrap()])
                .unwrap(),
            ),
        ))
    }
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Delete) {
            self.calls.borrow_mut().deletes += 1;
            return Err(PolicyError::new("prepared Cut must not use legacy Delete"));
        }
        Ok(IntentDisposition::Continue)
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .node(self.head)
            .and_then(|n| n.content().as_inline())
            != Some(&self.original)
        {
            self.calls
                .borrow_mut()
                .candidates
                .push(write_observation::calls());
            if self.reject {
                return Err(PolicyError::new("synthetic final candidate refusal"));
            }
        }
        Ok(())
    }
}
struct Listener(Rc<RefCell<Calls>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
        self.0
            .borrow_mut()
            .publications
            .push(write_observation::calls());
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.borrow_mut().selections += 1;
    }
}

struct Fixture {
    document: XiaomuDocument,
    head: NodeId,
    tail: NodeId,
    atom: NodeId,
}
fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new(text, MarkSet::new([Mark::Italic]).unwrap()).unwrap()
                ])
                .unwrap(),
            ),
        )
        .unwrap()
}
fn fixture(barrier: Option<NodeKind>, depth: usize) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let mut payload = AttrValue::String("opaque extension payload".into());
    for _ in 0..depth {
        payload = AttrValue::List(vec![payload]);
    }
    let atom = b
        .insert(
            NodeKind::InlineAtom(AtomKind::new("example.token.v1").unwrap()),
            NodeAttrs::new([("payload".into(), payload)].into()).unwrap(),
            NodeContent::InlineAtom(
                InlineAtomContent::new("token")
                    .unwrap()
                    .with_marks(MarkSet::new([Mark::Strike]).unwrap()),
            ),
        )
        .unwrap();
    let head = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("a中🙂z", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let mut children = vec![head];
    if let Some(kind) = barrier {
        let content = if kind == NodeKind::Table {
            let leaf = paragraph(&mut b, "cell");
            let cell = b
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([leaf]),
                )
                .unwrap();
            let row = b
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children([cell]),
                )
                .unwrap();
            NodeContent::children([row])
        } else {
            NodeContent::Atomic
        };
        children.push(b.insert(kind, NodeAttrs::empty(), content).unwrap());
    }
    let tail = paragraph(&mut b, "qéw");
    children.push(tail);
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        head,
        tail,
        atom,
    }
}
fn point(f: &Fixture, node: NodeId, byte: usize) -> InlinePoint {
    InlinePoint::new(
        node,
        f.document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(byte)
            .unwrap(),
        0,
        CursorAffinity::Before,
    )
}
fn range(f: &Fixture, cross: bool, reverse: bool) -> DocumentSelection {
    let a = point(f, f.head, 1);
    let b = if cross {
        point(f, f.tail, 1)
    } else {
        point(f, f.head, 8)
    };
    if reverse {
        DocumentSelection::new(b, a)
    } else {
        DocumentSelection::new(a, b)
    }
}
struct Mounted {
    window: WindowHandle<DocumentView>,
    session: SharedSession,
    calls: Rc<RefCell<Calls>>,
    gate: Rc<Cell<bool>>,
}
fn mount(
    cx: &mut TestAppContext,
    f: &Fixture,
    selection: DocumentSelection,
    reject: bool,
    legacy: bool,
) -> Mounted {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let gate = Rc::new(Cell::new(true));
    let hooks = EditorHooks {
        listener: Some(Box::new(Listener(calls.clone()))),
        ..Default::default()
    };
    let editor = if legacy {
        EditorInstance::new(f.document.clone(), selection, hooks).unwrap()
    } else {
        EditorInstance::new_with_policy(
            f.document.clone(),
            selection,
            hooks,
            Box::new(Policy {
                calls: calls.clone(),
                gate: gate.clone(),
                reject,
                head: f.head,
                original: f
                    .document
                    .node(f.head)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .clone(),
            }),
        )
        .unwrap()
    };
    let session = editor.session().clone();
    // Keep a genuine redo entry so every refused/cancelled Cut proves that
    // preparation does not clear existing history.
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
        session.set_document_selection(selection).unwrap();
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
    Mounted {
        window,
        session,
        calls,
        gate,
    }
}
fn previous(cx: &mut TestAppContext) -> ClipboardItem {
    let item = ClipboardItem::new_string("clipboard sentinel".into());
    cx.update(|cx| cx.write_to_clipboard(item.clone()));
    write_observation::reset();
    item
}
fn clipboard(cx: &mut TestAppContext) -> ClipboardItem {
    cx.update(|cx| cx.read_from_clipboard().unwrap())
}
fn cut(m: &Mounted, cx: &mut TestAppContext) {
    m.window
        .update(cx, |view, window, cx| view.cut(&ClipboardCut, window, cx))
        .unwrap();
    cx.background_executor.run_until_parked();
}
struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    history: (usize, usize),
    marks: Option<MarkSet>,
}
impl Snapshot {
    fn capture(m: &Mounted) -> Self {
        let s = m.session.borrow();
        Self {
            document: s.document().clone(),
            selection: s.selection(),
            history: s.history_depths(),
            marks: s.stored_marks().cloned(),
        }
    }
    fn unchanged(&self, m: &Mounted) {
        let s = m.session.borrow();
        assert_eq!(s.document().store(), self.document.store());
        assert_eq!(s.document().revision(), self.document.revision());
        assert_eq!(s.selection(), self.selection);
        assert_eq!(s.history_depths(), self.history);
        assert_eq!(s.stored_marks(), self.marks.as_ref());
        assert!(m.calls.borrow().publications.is_empty());
        assert_eq!(m.calls.borrow().selections, 0);
    }
}

#[gpui::test]
fn text_cut_native_unicode_marks_directions_and_unselected_atom_round_trip(
    cx: &mut TestAppContext,
) {
    for cross in [false, true] {
        for reverse in [false, true] {
            let f = fixture(None, 1);
            let selection = range(&f, cross, reverse);
            let m = mount(cx, &f, selection, false, false);
            let before = Snapshot::capture(&m);
            previous(cx);
            cx.simulate_keystrokes(m.window.into(), "ctrl-x");
            let item = clipboard(cx);
            assert_eq!(
                item.text().as_deref(),
                Some(if cross { "中🙂z\nq" } else { "中🙂" })
            );
            let decoded = decode_metadata(&item.text().unwrap(), item.metadata().unwrap()).unwrap();
            assert_eq!(prepare_lossless_slice(&decoded).unwrap(), item);
            assert_eq!(decoded.roots().len(), if cross { 2 } else { 1 });
            for (index, root) in decoded.roots().iter().enumerate() {
                let inline = root.content().as_inline().unwrap();
                assert!(
                    inline.atoms().is_empty(),
                    "unselected extension stays outside clipboard"
                );
                let marks =
                    MarkSet::new([if index == 0 { Mark::Bold } else { Mark::Italic }]).unwrap();
                assert!(inline.runs().iter().all(|run| run.marks() == &marks));
            }

            assert_eq!(write_observation::calls(), 1);
            assert_eq!(m.calls.borrow().candidates, [0]);
            assert_eq!(m.calls.borrow().publications, [1]);
            assert_eq!(m.calls.borrow().gates, 1);
            assert_eq!(m.calls.borrow().export_specs, 1);
            assert_eq!(m.calls.borrow().deletes, 0);
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            let after = m.session.borrow().document().clone();
            let caret = m.session.borrow().selection();
            assert!(caret.is_collapsed());
            assert_eq!(after.node(f.atom), before.document.node(f.atom));
            let inline = after.node(f.head).unwrap().content().as_inline().unwrap();
            assert_eq!(
                inline
                    .runs()
                    .iter()
                    .map(|r| r.text().as_str())
                    .collect::<String>(),
                if cross { "aéw" } else { "az" }
            );
            assert_eq!(
                inline.runs()[0].marks(),
                &MarkSet::new([Mark::Bold]).unwrap()
            );
            if cross {
                assert_eq!(
                    inline.runs().last().unwrap().marks(),
                    &MarkSet::new([Mark::Italic]).unwrap()
                );
            }
            m.session.borrow_mut().undo().unwrap();
            assert_eq!(
                m.session.borrow().document().store(),
                before.document.store()
            );
            assert_eq!(m.session.borrow().selection(), selection);
            m.session.borrow_mut().redo().unwrap();
            assert_eq!(m.session.borrow().document().store(), after.store());
            assert_eq!(m.session.borrow().selection(), caret);
            m.session
                .borrow_mut()
                .apply_intent(&EditIntent::InsertText {
                    text: "next".into(),
                })
                .unwrap();
            assert_eq!(
                m.session.borrow().history_depths(),
                (2, 0),
                "typing cannot merge into Cut"
            );
            m.session.borrow_mut().undo().unwrap();
            assert_eq!(m.session.borrow().document().store(), after.store());
            assert_eq!(clipboard(cx), item);
            assert_eq!(
                write_observation::calls(),
                1,
                "history does not rewrite clipboard"
            );
        }
    }
}

#[gpui::test]
fn text_cut_candidate_and_expired_source_gate_leave_everything_unchanged(cx: &mut TestAppContext) {
    for reject in [false, true] {
        let f = fixture(None, 1);
        let m = mount(cx, &f, range(&f, false, true), reject, false);
        let before = Snapshot::capture(&m);
        let sentinel = previous(cx);
        if !reject {
            m.gate.set(false);
        }
        cut(&m, cx);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(m.calls.borrow().gates, 1);
        assert_eq!(m.calls.borrow().candidates.len(), usize::from(reject));
        assert_eq!(m.calls.borrow().export_specs, usize::from(reject));
        assert_eq!(m.calls.borrow().deletes, 0);
    }
}

#[gpui::test]
fn text_cut_cross_atomic_and_table_never_write_or_fall_back(cx: &mut TestAppContext) {
    for barrier in [NodeKind::HorizontalRule, NodeKind::Image, NodeKind::Table] {
        for reverse in [false, true] {
            let f = fixture(Some(barrier.clone()), 1);
            let m = mount(cx, &f, range(&f, true, reverse), false, false);
            let before = Snapshot::capture(&m);
            let sentinel = previous(cx);
            cut(&m, cx);
            before.unchanged(&m);
            assert_eq!(clipboard(cx), sentinel);
            assert_eq!(write_observation::calls(), 0);
            assert_eq!(m.calls.borrow().export_specs, 1);
            assert_eq!(m.calls.borrow().deletes, 0);
            assert!(m.calls.borrow().candidates.is_empty());
        }
    }
}

#[gpui::test]
fn text_cut_prepared_item_cancel_does_not_publish(cx: &mut TestAppContext) {
    let f = fixture(None, 1);
    let m = mount(cx, &f, range(&f, false, false), false, false);
    let before = Snapshot::capture(&m);
    let sentinel = previous(cx);
    {
        let mut session = m.session.borrow_mut();
        let prepared = session.prepare_cut().unwrap().unwrap();
        let item = prepare_lossless_slice(prepared.clipboard_slice()).unwrap();
        assert_eq!(
            decode_metadata(&item.text().unwrap(), item.metadata().unwrap()),
            Some(prepared.clipboard_slice().clone())
        );
        drop(item);
        drop(prepared);
    }
    before.unchanged(&m);
    assert_eq!(clipboard(cx), sentinel);
    assert_eq!(write_observation::calls(), 0);
    assert_eq!(m.calls.borrow().candidates, [0]);
}

#[gpui::test]
fn text_cut_budget_refusal_and_real_legacy_metadata_failure_are_lossless(cx: &mut TestAppContext) {
    // The prepared source budget is stricter than the wire codec. Do not fake
    // a post-preparation encoding failure; exercise actual source rejection and
    // the naturally reachable legacy lossless encoder failure independently.
    for legacy in [false, true] {
        let f = fixture(None, 160);
        let selection = DocumentSelection::new(point(&f, f.head, 0), point(&f, f.head, 8));
        let m = mount(cx, &f, selection, false, legacy);
        let before = Snapshot::capture(&m);
        let sentinel = previous(cx);
        if legacy {
            let slice = m
                .session
                .borrow()
                .clipboard_slice_for(ClipboardExportPurpose::Cut)
                .unwrap()
                .unwrap();
            assert!(prepare_lossless_slice(&slice).is_none());
        }
        cut(&m, cx);
        before.unchanged(&m);
        assert_eq!(clipboard(cx), sentinel);
        assert_eq!(write_observation::calls(), 0);
        assert_eq!(m.calls.borrow().deletes, 0);
        assert!(m.calls.borrow().candidates.is_empty());
    }
}

#[gpui::test]
fn text_cut_default_false_keeps_legacy_text_cut_working(cx: &mut TestAppContext) {
    let f = fixture(None, 1);
    let selection = range(&f, false, false);
    let m = mount(cx, &f, selection, false, true);
    assert!(m.session.borrow_mut().prepare_cut().unwrap().is_none());
    previous(cx);
    cut(&m, cx);
    assert_eq!(clipboard(cx).text().as_deref(), Some("中🙂"));
    assert_eq!(write_observation::calls(), 1);
    assert_eq!(m.calls.borrow().publications, [1]);
    m.session.borrow_mut().undo().unwrap();
    assert_eq!(m.session.borrow().document().store(), f.document.store());
    assert_eq!(m.session.borrow().selection(), selection);
}
