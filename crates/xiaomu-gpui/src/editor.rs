//! Single-block editor application assembly.
//!
//! Wires key bindings, the window, and one [`ParagraphView`] into a running
//! GPUI application. Used by `editor_harness`; hosts assemble their own
//! windows on top of [`crate::block_view`] instead.

use gpui::{
    App, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
    px, size,
};

use std::cell::RefCell;
use std::rc::Rc;

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::selection::TextSelection;
use xiaomu_runtime::persistence::DocumentPersistence;
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSelection, DocumentSession};

use crate::block_view::{
    Backspace, ClipboardCopy, ClipboardCut, ClipboardPaste, Delete, Down, End, Enter, Home, Left,
    Redo, Right, SaveDocument, SelectAll, SelectDown, SelectEnd, SelectHome, SelectLeft,
    SelectRight, SelectUp, ShiftTabIndent, TabIndent, ToggleBold, ToggleCode, ToggleItalic,
    ToggleStrike, ToggleUnderline, Undo, Up,
};
use crate::document_view::DocumentView;

/// Optional host integrations handed to [`run_document_editor`].
///
/// Everything is frontend-neutral: persistence goes through the runtime
/// [`DocumentPersistence`] seam, change listening through
/// [`DocumentChangeListener`]. Absent hooks simply disable the feature.
#[derive(Default)]
pub struct EditorHooks {
    /// Adapter used by Ctrl/Cmd-S to persist the current snapshot.
    pub persistence: Option<Rc<RefCell<dyn DocumentPersistence>>>,
    /// Listener notified on every committed edit / undo / redo and every
    /// selection move.
    pub listener: Option<Box<dyn DocumentChangeListener>>,
}

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
    let _ = node; // the multi-block view derives blocks from the document
    run_document_editor(document, selection)
}

/// Runs a multi-block editor window over `document`.
///
/// The initial selection must be valid for the document. This function takes
/// over the main thread and returns only when the application quits.
pub fn run_document_editor(
    document: XiaomuDocument,
    selection: TextSelection,
) -> Result<(), xiaomu_runtime::session::SessionError> {
    run_document_editor_with_hooks(document, selection, EditorHooks::default())
}

/// Runs a multi-block editor window with host integration hooks.
///
/// This is the minimal host-contract entry point: hosts load their initial
/// document themselves, hand in a persistence adapter and a change listener,
/// and receive Ctrl/Cmd-S save semantics.
pub fn run_document_editor_with_hooks(
    document: XiaomuDocument,
    selection: TextSelection,
    hooks: EditorHooks,
) -> Result<(), xiaomu_runtime::session::SessionError> {
    let mut session = DocumentSession::new(document, DocumentSelection::text(selection))?;
    if let Some(listener) = hooks.listener {
        session.add_listener(listener);
    }
    let session = Rc::new(RefCell::new(session));

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
            KeyBinding::new("enter", Enter, None),
            KeyBinding::new("tab", TabIndent, None),
            KeyBinding::new("shift-tab", ShiftTabIndent, None),
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("up", Up, None),
            KeyBinding::new("down", Down, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("shift-up", SelectUp, None),
            KeyBinding::new("shift-down", SelectDown, None),
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
            KeyBinding::new("cmd-s", SaveDocument, None),
            KeyBinding::new("ctrl-s", SaveDocument, None),
        ]);

        let bounds = Bounds::centered(None, size(px(640.0), px(480.0)), cx);
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
                |_, cx| {
                    let mut view = DocumentView::new(session.clone());
                    if let Some(persistence) = &hooks.persistence {
                        view.set_persistence(persistence.clone());
                    }
                    cx.new(|_| view)
                },
            )
            .expect("open editor window");

        // Initial focus goes to the block holding the selection focus once
        // the view has built its children.
        window
            .update(cx, |view: &mut DocumentView, window, cx| {
                view.route_focus_initial(window, cx);
                cx.activate(true);
            })
            .expect("focus editor window");
    });

    Ok(())
}
