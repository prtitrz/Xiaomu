use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{AppContext as _, Modifiers, Point, TestAppContext, VisualTestContext, px};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_gpui::atom_capability::{AtomAction, InlineAtomHostCapability};
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_gpui::inline_atom::InlineAtomRendererRegistry;
use xiaomu_gpui::inline_atom_display::InlineAtomDisplayProjection;
use xiaomu_runtime::persistence::{DocumentPersistence, PersistenceError};
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSelection};

struct CountListener(Rc<Cell<u32>>);

impl DocumentChangeListener for CountListener {
    fn document_changed(&mut self, _document: &XiaomuDocument, _selection: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

struct CountPersistence(Rc<Cell<u32>>);

impl DocumentPersistence for CountPersistence {
    fn save(&mut self, _document: &XiaomuDocument) -> Result<(), PersistenceError> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }

    fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError> {
        Ok(None)
    }
}

fn document(first: &str, second: &str) -> (XiaomuDocument, [NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(first, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(second, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        [first, second],
    )
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
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

fn instance(
    document: XiaomuDocument,
    selection: DocumentSelection,
    saves: Rc<Cell<u32>>,
    changes: Rc<Cell<u32>>,
) -> EditorInstance {
    EditorInstance::new(
        document,
        selection,
        EditorHooks {
            persistence: Some(Rc::new(RefCell::new(CountPersistence(saves)))),
            listener: Some(Box::new(CountListener(changes))),
            atom_renderers: None,
            atom_capability: None,
        },
    )
    .unwrap()
}

#[gpui::test]
fn mounted_editors_isolate_focus_input_selection_save_and_listener(cx: &mut TestAppContext) {
    let (document_a, [_a_first, a_second]) = document("a", "tail-a");
    let selection_a = DocumentSelection::collapsed(point(&document_a, a_second, 6));
    let (document_b, [b_first, _b_second]) = document("head-b", "b");
    let selection_b = DocumentSelection::collapsed(point(&document_b, b_first, 6));

    let saves_a = Rc::new(Cell::new(0));
    let saves_b = Rc::new(Cell::new(0));
    let changes_a = Rc::new(Cell::new(0));
    let changes_b = Rc::new(Cell::new(0));
    let editor_a = instance(document_a, selection_a, saves_a.clone(), changes_a.clone());
    let editor_b = instance(document_b, selection_b, saves_b.clone(), changes_b.clone());
    let session_a = editor_a.session().clone();
    let session_b = editor_b.session().clone();

    cx.update(bind_default_editor_keys);
    let window_a = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| editor_a.build_view())
        })
        .unwrap()
    });
    let window_b = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| editor_b.build_view())
        })
        .unwrap()
    });

    window_a
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();

    let a_focus = window_a
        .update(cx, |view, window, cx| {
            view.accessibility_projection(window, cx)
                .unwrap()
                .focus_owner()
        })
        .unwrap();
    assert_eq!(a_focus, Some(a_second));

    cx.simulate_input(window_a.into(), "Z");
    assert_eq!(text(session_a.borrow().document(), a_second), "tail-aZ");
    assert_eq!(text(session_b.borrow().document(), b_first), "head-b");
    assert_eq!(session_b.borrow().selection(), selection_b);
    assert_eq!(changes_a.get(), 1);
    assert_eq!(changes_b.get(), 0);

    cx.simulate_keystrokes(window_a.into(), "ctrl-s");
    assert_eq!(saves_a.get(), 1);
    assert_eq!(saves_b.get(), 0);

    window_b
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();

    let b_focus = window_b
        .update(cx, |view, window, cx| {
            view.accessibility_projection(window, cx)
                .unwrap()
                .focus_owner()
        })
        .unwrap();
    let inactive_a_focus = window_a
        .update(cx, |view, window, cx| {
            view.accessibility_projection(window, cx)
                .unwrap()
                .focus_owner()
        })
        .unwrap();
    assert_eq!(b_focus, Some(b_first));
    assert_eq!(inactive_a_focus, None);

    cx.simulate_input(window_b.into(), "Q");
    assert_eq!(text(session_b.borrow().document(), b_first), "head-bQ");
    assert_eq!(text(session_a.borrow().document(), a_second), "tail-aZ");
    assert_eq!(changes_a.get(), 1);
    assert_eq!(changes_b.get(), 1);

    cx.simulate_keystrokes(window_b.into(), "ctrl-s");
    assert_eq!(saves_a.get(), 1);
    assert_eq!(saves_b.get(), 1);
}

/// Records atom activations per editor instance.
#[derive(Default)]
struct CapabilityRecorder {
    actions: RefCell<Vec<AtomAction>>,
}

impl InlineAtomHostCapability for CapabilityRecorder {
    fn atom_action(&self, action: AtomAction) {
        self.actions.borrow_mut().push(action);
    }
}

/// One paragraph whose text starts with a mention chip anchored at byte 0.
fn chip_document(handle: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            NodeAttrs::new(
                [(
                    "handle".to_owned(),
                    xiaomu_core::document::AttrValue::String(handle.to_owned()),
                )]
                .into_iter()
                .collect(),
            )
            .unwrap(),
            NodeContent::InlineAtom(InlineAtomContent::new("@mention").unwrap()),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("body", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(
                        atom,
                        xiaomu_core::text::TextBuffer::from_string("body".to_owned())
                            .offset_at(0)
                            .unwrap(),
                    )],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
        atom,
    )
}

fn chip_instance(document: XiaomuDocument, recorder: Rc<CapabilityRecorder>) -> EditorInstance {
    let first = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let selection = DocumentSelection::collapsed(point(&document, first, 0));
    EditorInstance::new(
        document,
        selection,
        EditorHooks {
            persistence: None,
            listener: None,
            atom_renderers: None,
            atom_capability: Some(recorder),
        },
    )
    .unwrap()
}

#[gpui::test]
fn atom_clicks_activate_only_the_clicked_editor(cx: &mut TestAppContext) {
    let (document_a, paragraph_a, atom_a) = chip_document("alice");
    let (document_b, paragraph_b, atom_b) = chip_document("bob");
    let recorder_a = Rc::new(CapabilityRecorder::default());
    let recorder_b = Rc::new(CapabilityRecorder::default());
    let editor_a = chip_instance(document_a, recorder_a.clone());
    let editor_b = chip_instance(document_b, recorder_b.clone());

    cx.update(bind_default_editor_keys);
    let window_a = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| editor_a.build_view())
        })
        .unwrap()
    });
    let window_b = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| editor_b.build_view())
        })
        .unwrap()
    });

    window_a
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();

    // The chip is the first thing in the only paragraph: root padding is
    // 16px and the first text row spans y 16..44, so (48, 30) lands inside
    // the rendered chip.
    let chip = Point::new(px(48.0), px(30.0));
    let mut view_a = VisualTestContext::from_window(window_a.into(), cx);
    view_a.simulate_click(chip, Modifiers::default());
    cx.background_executor.run_until_parked();

    let actions_a = recorder_a.actions.borrow();
    assert_eq!(actions_a.len(), 1, "click on editor A's chip activates A");
    assert_eq!(actions_a[0].node, atom_a);
    assert_eq!(actions_a[0].kind.as_str(), "mention");
    assert_eq!(actions_a[0].action.as_str(), "click");
    assert_eq!(
        actions_a[0]
            .attrs
            .get("handle")
            .and_then(|value| match value {
                xiaomu_core::document::AttrValue::String(text) => Some(text.as_str()),
                _ => None,
            }),
        Some("alice")
    );
    drop(actions_a);
    assert!(
        recorder_b.actions.borrow().is_empty(),
        "editor B must not observe A's activation"
    );
    assert_eq!(
        session_focus_node(&session_of(&editor_a)),
        Some(paragraph_a),
        "click also places the caret in the clicked block"
    );

    // Activating editor B's chip stays isolated from A.
    window_b
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let mut view_b = VisualTestContext::from_window(window_b.into(), cx);
    view_b.simulate_click(chip, Modifiers::default());
    cx.background_executor.run_until_parked();

    assert_eq!(recorder_b.actions.borrow().len(), 1);
    assert_eq!(recorder_b.actions.borrow()[0].node, atom_b);
    assert_eq!(
        recorder_a.actions.borrow().len(),
        1,
        "A stays at one action"
    );
    assert_eq!(
        session_focus_node(&session_of(&editor_b)),
        Some(paragraph_b),
        "B's caret also lands in B's clicked block"
    );
}

fn session_of(editor: &EditorInstance) -> xiaomu_gpui::block_view::SharedSession {
    editor.session().clone()
}

fn session_focus_node(session: &xiaomu_gpui::block_view::SharedSession) -> Option<NodeId> {
    match session.borrow().selection().focus() {
        xiaomu_runtime::session::DocumentPosition::Inline(point) => Some(point.node_id()),
        xiaomu_runtime::session::DocumentPosition::Gap(_) => None,
    }
}

/// Renderer registries are per-editor values; the same canonical document
/// projects differently through different registries without any shared
/// state.
#[test]
fn renderer_registries_project_independently() {
    let (document, paragraph, _atom) = chip_document("alice");

    let mut with_renderer = InlineAtomRendererRegistry::new();
    with_renderer.register(
        &AtomKind::new("mention").unwrap(),
        Rc::new(xiaomu_gpui::inline_atom::FallbackAtomRenderer),
    );
    let projected = InlineAtomDisplayProjection::build(&document, paragraph, &with_renderer)
        .unwrap()
        .display_text()
        .to_owned();
    let fallback = InlineAtomDisplayProjection::build(
        &document,
        paragraph,
        &InlineAtomRendererRegistry::new(),
    )
    .unwrap()
    .display_text()
    .to_owned();

    assert_eq!(fallback, "@mentionbody");
    assert_eq!(projected, fallback, "fallback renderer is deterministic");
}
