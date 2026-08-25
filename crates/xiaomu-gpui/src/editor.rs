//! Single-block editor application assembly.
//!
//! Wires key bindings, the window, and one [`ParagraphView`] into a running
//! GPUI application. Used by `editor_harness`; hosts assemble their own
//! windows on top of [`crate::block_view`] instead.

use gpui::{
    App, Application, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions,
    prelude::*, px, size,
};

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::selection::TextSelection;
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

use crate::block_view::{
    Backspace, ClipboardCopy, ClipboardCut, ClipboardPaste, Delete, End, Home, Left, ParagraphView,
    Redo, Right, SelectAll, SelectEnd, SelectHome, SelectLeft, SelectRight, ToggleBold, ToggleCode,
    ToggleItalic, ToggleStrike, ToggleUnderline, Undo,
};

/// Runs a single-paragraph editor window over `document`.
///
/// The initial selection must be valid for the document and live inside
/// `node`. This function takes over the main thread and returns only when
/// the application quits.
pub fn run_single_block_editor(
    document: XiaomuDocument,
    node: NodeId,
    selection: TextSelection,
) -> Result<(), xiaomu_runtime::session::SessionError> {
    let session = DocumentSession::new(document, DocumentSelection::text(selection))?;

    Application::new().run(move |cx: &mut App| {
        // Quit when the last window closes so the harness terminates cleanly.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("home", Home, None),
            KeyBinding::new("end", End, None),
            KeyBinding::new("shift-home", SelectHome, None),
            KeyBinding::new("shift-end", SelectEnd, None),
            KeyBinding::new("cmd-a", SelectAll, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("cmd-c", ClipboardCopy, None),
            KeyBinding::new("ctrl-c", ClipboardCopy, None),
            KeyBinding::new("cmd-x", ClipboardCut, None),
            KeyBinding::new("ctrl-x", ClipboardCut, None),
            KeyBinding::new("cmd-v", ClipboardPaste, None),
            KeyBinding::new("ctrl-v", ClipboardPaste, None),
            KeyBinding::new("cmd-b", ToggleBold, None),
            KeyBinding::new("ctrl-b", ToggleBold, None),
            KeyBinding::new("cmd-i", ToggleItalic, None),
            KeyBinding::new("ctrl-i", ToggleItalic, None),
            KeyBinding::new("cmd-e", ToggleCode, None),
            KeyBinding::new("ctrl-e", ToggleCode, None),
            KeyBinding::new("cmd-u", ToggleUnderline, None),
            KeyBinding::new("ctrl-u", ToggleUnderline, None),
            KeyBinding::new("cmd-shift-x", ToggleStrike, None),
            KeyBinding::new("ctrl-shift-x", ToggleStrike, None),
            KeyBinding::new("cmd-z", Undo, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("cmd-shift-z", Redo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("ctrl-y", Redo, None),
        ]);

        let bounds = Bounds::centered(None, size(px(640.0), px(200.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Xiaomu".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| ParagraphView::new(session, node, cx)),
            )
            .expect("open editor window");

        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
            })
            .expect("focus editor window");
    });

    Ok(())
}
