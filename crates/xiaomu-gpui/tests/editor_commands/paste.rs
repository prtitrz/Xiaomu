use super::*;
use xiaomu_core::document::{ImageAttrs, ImageSource};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink},
    clipboard::encode_metadata,
};

#[gpui::test]
fn raw_line_endings_reach_router_before_default_folding(cx: &mut TestAppContext) {
    let raw = "甲\r\n乙\n丙\r丁";
    let (document, node) = single(NodeKind::Paragraph, "");
    let selection = caret(&document, node, 0);
    let router = Router::new(Decision::Default);
    let editor = EditorInstance::new(document, selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    paste_text(window, raw, cx);
    assert_eq!(text(session.borrow().document(), node), "甲 乙 丙 丁");
    let observed = router.observed.borrow();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].command, Gesture::Paste(raw.into()));
    assert_eq!(observed[0].selection, selection);
    assert_eq!(session.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn custom_multiline_slice_is_one_edit_notification_and_undo_unit(cx: &mut TestAppContext) {
    let raw = "甲\r\n乙\n丙";
    let (document, node) = single(NodeKind::Paragraph, "");
    let selection = caret(&document, node, 0);
    let router = Router::new(Decision::Multiline);
    let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    let counts = listen(&session);
    paste_text(window, raw, cx);
    let pasted = session.borrow().document().clone();
    let blocks = children(&pasted, pasted.root());
    assert_eq!(
        blocks
            .iter()
            .map(|id| text(&pasted, *id))
            .collect::<Vec<_>>(),
        ["甲", "乙", "丙"]
    );
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(counts.get().0, 1);
    assert_eq!(router.observed.borrow().len(), 1);
    assert_eq!(
        router.observed.borrow()[0].command,
        Gesture::Paste(raw.into())
    );
    press(window, "ctrl-z", cx);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 1));
    press(window, "ctrl-shift-z", cx);
    assert_eq!(session.borrow().document().store(), pasted.store());
    assert_eq!(router.observed.borrow().len(), 1);
}

fn structured_clipboard(slice: &ClipboardSlice, cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
            slice.plain_text().into(),
            encode_metadata(slice).unwrap(),
        ))
    });
}

#[gpui::test]
fn native_structured_paste_never_enters_raw_router(cx: &mut TestAppContext) {
    let (document, node) = single(NodeKind::Paragraph, "");
    let selection = caret(&document, node, 0);
    let router = Router::new(Decision::Reject);
    let editor = EditorInstance::new(document, selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    structured_clipboard(&multiline_slice("a\nb"), cx);
    press(window, "ctrl-v", cx);
    let changed = session.borrow().document().clone();
    assert_eq!(
        children(&changed, changed.root())
            .iter()
            .map(|id| text(&changed, *id))
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(router.observed.borrow().is_empty());
    assert_eq!(session.borrow().history_depths(), (1, 0));
}

#[gpui::test]
fn code_block_plain_and_structured_paste_keep_multiline_path_without_raw_hook(
    cx: &mut TestAppContext,
) {
    let (document, node) = single(NodeKind::CodeBlock, "");
    let selection = caret(&document, node, 0);
    let router = Router::new(Decision::Reject);
    let editor = EditorInstance::new(document, selection, EditorHooks::default())
        .unwrap()
        .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    paste_text(window, "a\r\nb\nc", cx);
    assert_eq!(text(session.borrow().document(), node), "a\nb\nc");
    assert!(router.observed.borrow().is_empty());
    press(window, "ctrl-z", cx);
    structured_clipboard(&multiline_slice("x\ny"), cx);
    press(window, "ctrl-v", cx);
    assert_eq!(text(session.borrow().document(), node), "x\ny");
    assert!(router.observed.borrow().is_empty());
    assert_eq!(
        children(
            session.borrow().document(),
            session.borrow().document().root()
        ),
        [node]
    );
    assert_eq!(session.borrow().history_depths(), (1, 0));
}

struct ImageHost(Cell<usize>);

impl AssetService for ImageHost {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(&self, format: AssetFormat, bytes: &[u8]) -> Result<ImageAttrs, AssetError> {
        assert_eq!(format, AssetFormat::Png);
        assert_eq!(bytes, [1, 2, 3]);
        self.0.set(self.0.get() + 1);
        ImageAttrs::new(
            ImageSource::AssetRef("image".into()),
            "pasted".into(),
            None,
            None,
            None,
        )
        .map_err(|_| AssetError::InvalidImage)
    }
}

#[gpui::test]
fn native_image_paste_never_enters_raw_router(cx: &mut TestAppContext) {
    let (document, node) = single(NodeKind::Paragraph, "");
    let selection = caret(&document, node, 0);
    let router = Router::new(Decision::Reject);
    let host = Rc::new(ImageHost(Cell::new(0)));
    let editor = EditorInstance::new(
        document,
        selection,
        EditorHooks {
            asset_service: Some(host.clone()),
            ..Default::default()
        },
    )
    .unwrap()
    .with_command_router(router.clone());
    let (window, session) = mount(editor, cx);
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![1, 2, 3],
        )))
    });
    press(window, "ctrl-v", cx);
    assert_eq!(host.0.get(), 1);
    assert!(router.observed.borrow().is_empty());
    assert_eq!(
        session
            .borrow()
            .document()
            .store()
            .iter()
            .filter(|node| matches!(node.kind(), NodeKind::Image))
            .count(),
        1
    );
    assert_eq!(session.borrow().history_depths(), (1, 0));
}
