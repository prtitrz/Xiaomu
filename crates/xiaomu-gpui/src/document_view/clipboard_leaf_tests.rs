//! Actual GPUI virtual key/action dispatch; OS clipboard acceptance is separate.

use crate::block_view::SharedSession;
use crate::document_view::DocumentView;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::input::platform_clipboard::write_observation;
use gpui::{AppContext as _, ClipboardItem, TestAppContext, WindowHandle};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink};
use xiaomu_runtime::clipboard::{
    ClipboardExportPurpose, ClipboardExportSpec, ClipboardTextProjection, decode_metadata,
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, EditIntent, EditPlan, IntentDisposition,
    PolicyError, SelectionUpdate, SessionContext, SessionPolicy,
};

#[derive(Default)]
struct Calls {
    exports: Vec<ClipboardExportPurpose>,
    intents: usize,
    imports: usize,
    candidates: Vec<usize>,
    publications: Vec<usize>,
}

struct Policy {
    calls: Rc<RefCell<Calls>>,
    projection: Option<ClipboardTextProjection>,
    prepared: bool,
    reject_candidate: bool,
    selected: NodeId,
    tail: NodeId,
}

impl SessionPolicy for Policy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.calls.borrow_mut().exports.push(purpose);
        Ok(self
            .projection
            .clone()
            .map(|projection| ClipboardExportSpec::new().with_text_projection(projection)))
    }

    fn prepare_cut(&self, _: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        if !self.prepared {
            return Ok(None);
        }
        let mut tx = Transaction::new(TransactionOrigin::UserInput);
        tx.push_step(TransactionStep::RemoveNode {
            node: self.selected,
        });
        Ok(Some(EditPlan::new(
            tx,
            SelectionUpdate::Exact {
                selection: DocumentSelection::collapsed(InlinePoint::at_start_of(self.tail)),
            },
            None,
        )))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.calls.borrow_mut().intents += 1;
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.node(self.selected).is_none() {
            self.calls
                .borrow_mut()
                .candidates
                .push(write_observation::calls());
            if self.reject_candidate {
                return Err(PolicyError::new("synthetic final candidate refusal"));
            }
        }
        Ok(())
    }
}

struct Probe(Rc<RefCell<Calls>>);
impl DocumentChangeListener for Probe {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0
            .borrow_mut()
            .publications
            .push(write_observation::calls());
    }
}
impl AssetService for Probe {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(
        &self,
        _: AssetFormat,
        _: &[u8],
    ) -> Result<xiaomu_core::document::ImageAttrs, AssetError> {
        self.0.borrow_mut().imports += 1;
        Err(AssetError::InvalidImage)
    }
}

fn projection() -> ClipboardTextProjection {
    ClipboardTextProjection::text_between_lf_with_atoms([
        AtomKind::new("example.reference.v1").unwrap()
    ])
    .unwrap()
}

fn document(kind: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new(kind).unwrap()),
            NodeAttrs::new([("name".into(), AttrValue::String("Synthetic".into()))].into())
                .unwrap(),
            NodeContent::InlineAtom(
                InlineAtomContent::new("visible label")
                    .unwrap()
                    .with_marks(MarkSet::new([Mark::Italic]).unwrap()),
            ),
        )
        .unwrap();
    let selected = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("中🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
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
            NodeContent::children([selected, tail]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        selected,
        tail,
    )
}

struct Mounted {
    window: WindowHandle<DocumentView>,
    session: SharedSession,
    calls: Rc<RefCell<Calls>>,
}

fn mount(
    cx: &mut TestAppContext,
    kind: &str,
    projection: Option<ClipboardTextProjection>,
    read_only: bool,
    prepared: bool,
    reject_candidate: bool,
) -> Mounted {
    let (document, selected, tail) = document(kind);
    let selection = DocumentSelection::node(&document, selected).unwrap();
    let calls = Rc::new(RefCell::new(Calls::default()));
    let editor = EditorInstance::new_with_policy(
        document,
        selection,
        EditorHooks {
            listener: Some(Box::new(Probe(calls.clone()))),
            asset_service: Some(Rc::new(Probe(calls.clone()))),
            ..Default::default()
        },
        Box::new(Policy {
            calls: calls.clone(),
            projection,
            prepared,
            reject_candidate,
            selected,
            tail,
        }),
    )
    .unwrap();
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
        calls,
    }
}

fn previous(cx: &mut TestAppContext) -> ClipboardItem {
    let item = ClipboardItem::new_string("previous clipboard".into());
    cx.update(|cx| cx.write_to_clipboard(item.clone()));
    write_observation::reset();
    item
}

fn clipboard(cx: &mut TestAppContext) -> ClipboardItem {
    cx.update(|cx| cx.read_from_clipboard().unwrap())
}

#[gpui::test]
fn clipboard_leaf_read_only_native_copy_is_lf_and_cut_stays_silent(cx: &mut TestAppContext) {
    let m = mount(
        cx,
        "example.reference.v1",
        Some(projection()),
        true,
        false,
        false,
    );
    let before = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
    m.calls.borrow_mut().exports.clear();
    previous(cx);
    cx.simulate_keystrokes(m.window.into(), "ctrl-c");
    let item = clipboard(cx);
    assert_eq!(item.text().as_deref(), Some("\n中🙂"));
    assert!(
        item.metadata()
            .unwrap()
            .starts_with("xiaomu.clipboard.v15\n")
    );
    assert_eq!(
        decode_metadata(&item.text().unwrap(), item.metadata().unwrap()),
        Some(expected)
    );
    assert_eq!(write_observation::calls(), 1);
    cx.simulate_keystrokes(m.window.into(), "ctrl-x");
    cx.simulate_keystrokes(m.window.into(), "ctrl-v");
    assert_eq!(write_observation::calls(), 1);
    assert_eq!(clipboard(cx), item);
    assert_eq!(m.calls.borrow().exports, [ClipboardExportPurpose::Copy]);
    assert_eq!(m.calls.borrow().intents, 0);
    assert_eq!(m.calls.borrow().imports, 0);
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn clipboard_leaf_native_default_v1_unknown_and_ordinary_cut_boundaries(cx: &mut TestAppContext) {
    for (kind, rule, expected) in [
        ("example.reference.v1", None, Some("visible label中🙂")),
        (
            "example.reference.v1",
            Some(ClipboardTextProjection::TextBetweenLfV1),
            None,
        ),
        ("example.unknown.v1", Some(projection()), None),
        ("example.reference.v1", Some(projection()), Some("\n中🙂")),
    ] {
        let m = mount(cx, kind, rule.clone(), false, false, false);
        let before = m.session.borrow().document().clone();
        let selection = m.session.borrow().selection();
        let old = previous(cx);
        cx.simulate_keystrokes(m.window.into(), "ctrl-c");
        match expected {
            Some(text) => assert_eq!(clipboard(cx).text().as_deref(), Some(text)),
            None => assert_eq!(clipboard(cx), old),
        }
        if matches!(
            rule,
            Some(ClipboardTextProjection::TextBetweenLfWithAtomsV1(_))
        ) {
            let old = previous(cx);
            cx.simulate_keystrokes(m.window.into(), "ctrl-x");
            assert_eq!(clipboard(cx), old);
            assert_eq!(write_observation::calls(), 0);
        }
        assert_eq!(m.session.borrow().document().store(), before.store());
        assert_eq!(m.session.borrow().selection(), selection);
        assert_eq!(m.session.borrow().history_depths(), (0, 0));
        assert_eq!(m.calls.borrow().intents, 0);
        assert_eq!(m.calls.borrow().imports, 0);
    }
}

#[gpui::test]
fn clipboard_leaf_prepared_cut_checks_before_write_and_undo_restores_rich_atom(
    cx: &mut TestAppContext,
) {
    for reject in [false, true] {
        let m = mount(
            cx,
            "example.reference.v1",
            Some(projection()),
            false,
            true,
            reject,
        );
        let before = m.session.borrow().document().clone();
        let selection = m.session.borrow().selection();
        let old = previous(cx);
        cx.simulate_keystrokes(m.window.into(), "ctrl-x");
        assert_eq!(m.calls.borrow().candidates, [0]);
        if reject {
            assert_eq!(clipboard(cx), old);
            assert_eq!(write_observation::calls(), 0);
            assert!(m.calls.borrow().publications.is_empty());
            assert_eq!(m.session.borrow().document().store(), before.store());
            assert_eq!(m.session.borrow().selection(), selection);
            assert_eq!(m.session.borrow().history_depths(), (0, 0));
        } else {
            assert_eq!(clipboard(cx).text().as_deref(), Some("\n中🙂"));
            assert_eq!(write_observation::calls(), 1);
            assert_eq!(m.calls.borrow().publications, [1]);
            assert_eq!(m.session.borrow().history_depths(), (1, 0));
            m.session.borrow_mut().undo().unwrap();
            assert_eq!(m.session.borrow().document().store(), before.store());
            assert_eq!(m.session.borrow().selection(), selection);
        }
        assert_eq!(m.calls.borrow().intents, 0);
        assert_eq!(m.calls.borrow().imports, 0);
    }
}

#[gpui::test]
fn clipboard_leaf_tampered_native_metadata_never_reaches_edit_or_import(cx: &mut TestAppContext) {
    let m = mount(
        cx,
        "example.reference.v1",
        Some(projection()),
        false,
        false,
        false,
    );
    cx.simulate_keystrokes(m.window.into(), "ctrl-c");
    let valid = clipboard(cx);
    let metadata = valid
        .metadata()
        .unwrap()
        .replace("text_between_lf_with_atoms_v1", "unknown_projection");
    let bad = ClipboardItem::new_string_with_metadata("fallback must not paste".into(), metadata);
    cx.update(|cx| cx.write_to_clipboard(bad.clone()));
    let before = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    cx.simulate_keystrokes(m.window.into(), "ctrl-v");
    assert_eq!(clipboard(cx), bad);
    assert_eq!(m.calls.borrow().intents, 0);
    assert_eq!(m.calls.borrow().imports, 0);
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}
