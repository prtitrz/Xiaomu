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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use xiaomu_core::document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeStoreBuilder, TextRun,
    };
    use xiaomu_core::selection::{CursorAffinity, TextPoint};
    use xiaomu_runtime::session::{EditIntent, SessionOutcome};

    struct CountListener(Rc<Cell<u32>>);

    impl DocumentChangeListener for CountListener {
        fn document_changed(&mut self, _document: &XiaomuDocument, _selection: DocumentSelection) {
            self.0.set(self.0.get() + 1);
        }
    }

    fn two_paragraph_document(first: &str, second: &str) -> (XiaomuDocument, [NodeId; 2]) {
        let mut builder = NodeStoreBuilder::new();
        let mut leaf = |text: &str| {
            builder
                .insert(
                    xiaomu_core::document::NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        };
        let first_id = leaf(first);
        let second_id = leaf(second);
        let root = builder
            .insert(
                xiaomu_core::document::NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([first_id, second_id]),
            )
            .unwrap();
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            [first_id, second_id],
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

    #[test]
    fn editor_instance_restores_cross_block_selection_without_frontend_state() {
        let (document, [first, second]) = two_paragraph_document("alpha", "beta");
        let selection = DocumentSelection::new(
            xiaomu_runtime::session::DocumentPosition::from(point(&document, first, 2)),
            xiaomu_runtime::session::DocumentPosition::from(point(&document, second, 3)),
        );

        let instance = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
        assert_eq!(instance.session().borrow().selection(), selection);
    }

    #[test]
    fn two_editor_instances_keep_document_selection_history_and_listeners_isolated() {
        let (document_a, [a_first, _]) = two_paragraph_document("a", "tail-a");
        let selection_a = DocumentSelection::collapsed(point(&document_a, a_first, 1));
        let (document_b, [_, b_second]) = two_paragraph_document("head-b", "b");
        let selection_b = DocumentSelection::collapsed(point(&document_b, b_second, 0));
        let a_changes = Rc::new(Cell::new(0));
        let b_changes = Rc::new(Cell::new(0));

        let instance_a = EditorInstance::new(
            document_a,
            selection_a,
            EditorHooks {
                persistence: None,
                listener: Some(Box::new(CountListener(a_changes.clone()))),
            },
        )
        .unwrap();
        let instance_b = EditorInstance::new(
            document_b,
            selection_b,
            EditorHooks {
                persistence: None,
                listener: Some(Box::new(CountListener(b_changes.clone()))),
            },
        )
        .unwrap();

        let outcome = instance_a
            .session()
            .borrow_mut()
            .apply_intent(&EditIntent::InsertText {
                text: "!".to_owned(),
            })
            .unwrap();
        assert_eq!(outcome, SessionOutcome::DocumentChanged);

        assert_eq!(
            text(instance_a.session().borrow().document(), a_first),
            "a!"
        );
        assert_eq!(
            text(instance_b.session().borrow().document(), b_second),
            "b"
        );
        assert_eq!(instance_b.session().borrow().selection(), selection_b);
        assert_eq!(instance_a.session().borrow().history_depths(), (1, 0));
        assert_eq!(instance_b.session().borrow().history_depths(), (0, 0));
        assert_eq!(a_changes.get(), 1);
        assert_eq!(b_changes.get(), 0);
    }
}
