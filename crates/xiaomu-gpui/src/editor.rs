//! Editor instance and convenience application assembly.
//!
//! Hosts that already own a GPUI [`Application`] can construct independent
//! [`EditorInstance`]s, mount each instance's [`DocumentView`] wherever they
//! need, and install the default Xiaomu key bindings once. The `run_*`
//! helpers remain convenience entry points for the standalone harness.

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
    SelectRight, SelectUp, SharedSession, ShiftTabIndent, TabIndent, ToggleBold, ToggleCode,
    ToggleItalic, ToggleStrike, ToggleUnderline, Undo, Up,
};
use crate::document_view::{DocumentView, actions::HardBreak};

/// Optional host integrations handed to an [`EditorInstance`].
///
/// Everything outside the GPUI crate remains frontend-neutral: persistence
/// goes through the Runtime [`DocumentPersistence`] seam and change listening
/// through [`DocumentChangeListener`]. Absent hooks simply disable that leg.
#[derive(Default)]
pub struct EditorHooks {
    /// Adapter used by Ctrl/Cmd-S to persist this editor's current snapshot.
    pub persistence: Option<Rc<RefCell<dyn DocumentPersistence>>>,
    /// Listener notified on committed document changes and selection moves.
    pub listener: Option<Box<dyn DocumentChangeListener>>,
}

/// One independent Xiaomu editor instance owned by a host.
///
/// Instances never share session, history, stored marks, listener, persistence
/// or focus state unless the host explicitly supplies shared adapters. This is
/// the embedding seam used by the P3.6 multi-editor integration fixture.
pub struct EditorInstance {
    session: SharedSession,
    persistence: Option<Rc<RefCell<dyn DocumentPersistence>>>,
}

impl EditorInstance {
    /// Creates one editor from a canonical snapshot and an already-restored
    /// document-level selection.
    ///
    /// Accepting [`DocumentSelection`] rather than only [`TextSelection`]
    /// lets a host restore cross-block selection state without inventing a
    /// GPUI-specific persistence format.
    pub fn new(
        document: XiaomuDocument,
        selection: DocumentSelection,
        hooks: EditorHooks,
    ) -> Result<Self, xiaomu_runtime::session::SessionError> {
        let mut session = DocumentSession::new(document, selection)?;
        if let Some(listener) = hooks.listener {
            session.add_listener(listener);
        }
        Ok(Self {
            session: Rc::new(RefCell::new(session)),
            persistence: hooks.persistence,
        })
    }

    /// Returns this instance's independent shared session handle.
    #[must_use]
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    /// Builds a fresh [`DocumentView`] mounted over this instance.
    ///
    /// Hosts may build multiple views deliberately over the same instance;
    /// distinct editor state requires distinct [`EditorInstance`] values.
    #[must_use]
    pub fn build_view(&self) -> DocumentView {
        let mut view = DocumentView::new(self.session.clone());
        if let Some(persistence) = &self.persistence {
            view.set_persistence(persistence.clone());
        }
        view
    }
}

/// Installs Xiaomu's default keyboard bindings into an existing GPUI app.
///
/// Hosts that own their application lifecycle can call this once before
/// mounting one or more editor instances.
pub fn bind_default_editor_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", Delete, None),
        KeyBinding::new("enter", Enter, None),
        KeyBinding::new("shift-enter", HardBreak, None),
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
/// Compatibility wrapper for the original P2/P3 harness entry point. Hosts
/// that restore a document-level selection or host multiple editors should
/// construct [`EditorInstance`] directly and own their GPUI application.
pub fn run_document_editor_with_hooks(
    document: XiaomuDocument,
    selection: TextSelection,
    hooks: EditorHooks,
) -> Result<(), xiaomu_runtime::session::SessionError> {
    let instance = EditorInstance::new(document, DocumentSelection::text(selection), hooks)?;
    run_editor_instance(instance)
}

/// Runs one preconstructed editor instance in a standalone GPUI application.
///
/// This remains a convenience for examples. Product hosts should normally own
/// their application/window shell and mount [`EditorInstance::build_view`].
pub fn run_editor_instance(
    instance: EditorInstance,
) -> Result<(), xiaomu_runtime::session::SessionError> {
    Application::new().run(move |cx: &mut App| {
        // Quit when the last window closes so the harness terminates cleanly.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        bind_default_editor_keys(cx);

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
                |_, cx| cx.new(|_| instance.build_view()),
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
