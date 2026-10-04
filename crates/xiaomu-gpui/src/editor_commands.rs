//! Opt-in, read-only routing of a bounded set of editor gestures.

use xiaomu_core::document::{MarkSet, XiaomuDocument};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent, PolicyError};

/// A gesture offered before Xiaomu's default planning or normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand<'a> {
    /// Tab or Shift-Tab, before table navigation and list/paragraph planning.
    Tab {
        /// Whether the user pressed Shift-Tab.
        reverse: bool,
    },
    /// Unmodified platform text, including CRLF/LF, outside a code block.
    ///
    /// Valid native structured clipboard data, image paste and IME transport
    /// never enter this route. A code block keeps its original paste path.
    PlainTextPaste(&'a str),
}

/// Read-only canonical state at the original command selection.
#[derive(Clone, Copy)]
pub struct EditorCommandContext<'a> {
    document: &'a XiaomuDocument,
    selection: DocumentSelection,
    stored_marks: Option<&'a MarkSet>,
}

impl<'a> EditorCommandContext<'a> {
    pub(crate) fn from_session(session: &'a DocumentSession) -> Self {
        Self {
            document: session.document(),
            selection: session.selection(),
            stored_marks: session.stored_marks(),
        }
    }

    /// Returns the current canonical document without changing it.
    #[must_use]
    pub const fn document(self) -> &'a XiaomuDocument {
        self.document
    }

    /// Returns both original endpoints, including direction and affinity.
    #[must_use]
    pub const fn selection(self) -> DocumentSelection {
        self.selection
    }

    /// Returns explicit typing marks, or `None` for run inheritance.
    #[must_use]
    pub const fn stored_marks(self) -> Option<&'a MarkSet> {
        self.stored_marks
    }
}

/// A command router's decision, made before any editor mutation.
#[derive(Clone, Debug)]
pub enum CommandRoute {
    /// Preserve Xiaomu's exact default behavior for this gesture.
    Default,
    /// Consume the gesture without changing selection, marks or history.
    NoChange,
    /// Send one intent through the ordinary session policy and undo pipeline.
    ///
    /// A rejected intent leaves canonical session state unchanged. This does
    /// not bypass policy validation or open a separate mutation path.
    Intent(EditIntent),
}

/// Optional per-view routing for Tab, ordinary text paste and Select All.
///
/// Callbacks must be pure, read-only and non-reentrant. Do not borrow or mutate
/// the editor session recursively, or perform external side effects. Runtime
/// can roll back its own state, not a callback's external effects. Returning
/// an error consumes the command without session changes. Native composition
/// uses the existing guard and is never dispatched here while active.
///
/// Install with [`EditorInstance::with_command_router`](crate::editor::EditorInstance::with_command_router)
/// or [`DocumentView::set_command_router`](crate::document_view::DocumentView::set_command_router).
pub trait EditorCommandRouter {
    /// Optionally supplies an explicit selection for the Select All gesture.
    ///
    /// `None` preserves the original text-range behavior. Hosts opting into
    /// `DocumentSelection::all(context.document())` must handle root-range
    /// edits in their session policy; generic unsupported edits fail closed.
    /// Returning an error consumes the gesture without changing session state.
    /// Existing routers need not implement this method.
    fn select_all(
        &self,
        _context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        Ok(None)
    }

    /// Routes a gesture using the original, validated selection and snapshot.
    fn route(
        &self,
        context: EditorCommandContext<'_>,
        command: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError>;
}
