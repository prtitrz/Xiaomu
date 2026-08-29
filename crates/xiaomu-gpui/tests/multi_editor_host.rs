use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::TestAppContext;
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_runtime::persistence::{DocumentPersistence, PersistenceError};
use xiaomu_runtime::session::{DocumentChangeListener, DocumentPosition, DocumentSelection};

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
        },
    )
    .unwrap()
}

#[gpui::test]
fn mounted_editors_isolate_focus_input_selection_save_and_listener(cx: &mut TestAppContext) {
    let (document_a, [a_first, a_second]) = document("a", "tail-a");
    let selection_a = DocumentSelection::new(
        DocumentPosition::Text(point(&document_a, a_first, 0)),
        DocumentPosition::Text(point(&document_a, a_second, 6)),
    );
    let (document_b, [b_first, b_second]) = document("head-b", "b");
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
