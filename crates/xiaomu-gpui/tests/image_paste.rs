//! Actual Ctrl+V action with host success/failure, followed by Undo/Redo.
use gpui::{AppContext as _, TestAppContext};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{
        ImageAttrs, ImageSource, InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder,
        XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
};
use xiaomu_gpui::{
    document_view::DocumentView,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink},
    session::{DocumentPosition, DocumentSelection},
};

struct Host {
    fail: bool,
    calls: Cell<usize>,
}
impl AssetService for Host {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(&self, format: AssetFormat, bytes: &[u8]) -> Result<ImageAttrs, AssetError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(format, AssetFormat::Png);
        assert_eq!(bytes, [1, 2, 3]);
        if self.fail {
            return Err(AssetError::PermissionDenied);
        }
        ImageAttrs::new(
            ImageSource::AssetRef("host-image".into()),
            "Pasted".into(),
            None,
            None,
            None,
        )
        .map_err(|_| AssetError::InvalidImage)
    }
}
fn open(
    cx: &mut TestAppContext,
    host: Rc<Host>,
    kind: NodeKind,
) -> (
    gpui::WindowHandle<DocumentView>,
    xiaomu_gpui::block_view::SharedSession,
) {
    let mut builder = NodeStoreBuilder::new();
    let p = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::collapsed(InlinePoint::new(
        p,
        document
            .node(p)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(0)
            .unwrap(),
        0,
        CursorAffinity::Before,
    ));
    let editor = EditorInstance::new(
        document,
        selection,
        EditorHooks {
            asset_service: Some(host),
            ..Default::default()
        },
    )
    .unwrap();
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
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![1, 2, 3],
        )))
    });
    (window, session)
}
fn images(session: &xiaomu_gpui::block_view::SharedSession) -> Vec<xiaomu_core::document::NodeId> {
    let s = session.borrow();
    s.document()
        .node(s.document().root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .iter()
        .copied()
        .filter(|id| matches!(s.document().node(*id).unwrap().kind(), NodeKind::Image))
        .collect()
}
#[gpui::test]
fn ctrl_v_import_failure_preserves_selection_document_and_history(cx: &mut TestAppContext) {
    let host = Rc::new(Host {
        fail: true,
        calls: Cell::new(0),
    });
    let (window, session) = open(cx, host.clone(), NodeKind::Paragraph);
    let selection = session.borrow().selection();
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    assert_eq!(host.calls.get(), 1);
    assert!(images(&session).is_empty());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
#[gpui::test]
fn ctrl_v_image_is_one_history_entry_and_atomic_target_rejects_before_import(
    cx: &mut TestAppContext,
) {
    let host = Rc::new(Host {
        fail: false,
        calls: Cell::new(0),
    });
    let (window, session) = open(cx, host.clone(), NodeKind::Paragraph);
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    let inserted = images(&session);
    assert_eq!(inserted.len(), 1);
    assert_eq!(session.borrow().history_depths(), (1, 0));
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert!(images(&session).is_empty());
    cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
    assert_eq!(images(&session), inserted);
    session
        .borrow_mut()
        .set_atomic_selection(inserted[0])
        .unwrap();
    window
        .update(cx, |view, window, cx| view.focus_selection(window, cx))
        .unwrap();
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    assert_eq!(host.calls.get(), 1);
    assert_eq!(images(&session), inserted);
    assert_eq!(
        session.borrow().selection().focus(),
        DocumentPosition::Atomic(inserted[0])
    );
}
#[gpui::test]
fn code_block_rejects_image_before_import(cx: &mut TestAppContext) {
    let host = Rc::new(Host {
        fail: false,
        calls: Cell::new(0),
    });
    let (window, session) = open(cx, host.clone(), NodeKind::CodeBlock);
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    assert_eq!(host.calls.get(), 0);
    assert!(images(&session).is_empty());
}
