//! Opt-in, read-only routing of a bounded set of editor gestures.

use xiaomu_core::document::{MarkSet, XiaomuDocument};
use xiaomu_runtime::clipboard::ClipboardSlice;
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
    /// never enter this route. Code-block paste uses `route_code_paste` instead.
    PlainTextPaste(&'a str),
}

/// The original Enter gesture, before block-kind-specific intent planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterSource {
    /// Enter without modifiers.
    Plain,
    /// Shift-Enter, whose default is an inline LF.
    Shift,
    /// Ctrl/Cmd-Enter, only bound when the host explicitly opts in.
    ///
    /// A default route propagates this action to outer application handlers.
    PrimaryModifier,
}

/// The validated clipboard transport offered to a code-block paste hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePasteSource {
    /// Ordinary platform text without valid native structured metadata.
    ///
    /// Also covers nonempty text accompanying an image. `Default` preserves
    /// the image-first transport path; an explicit intent can prefer raw text.
    PlatformText,
    /// The exact plain-text projection of validated native structured data.
    ///
    /// This source never enters [`EditorCommand::PlainTextPaste`].
    NativeStructuredPlainText {
        /// Whether the source has explicit closed whole-root boundaries.
        ///
        /// A default route preserves `PasteSlice` for closed data. A host
        /// choosing a text intent explicitly opts into discarding structure.
        closed: bool,
    },
}

gpui::actions!(
    xiaomu_gpui,
    [
        /// Opt-in Ctrl/Cmd-Enter gesture, with no default document edit.
        ///
        /// Bind through `editor::bind_primary_modifier_enter_keys` or install
        /// an explicit host key binding. Default routing propagates outward.
        PrimaryModifierEnter,
    ]
);

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

/// Optional per-view routing for explicit editor gestures and clipboard sources.
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

    /// Optionally routes unmodified ArrowDown to an exact selection.
    ///
    /// Called before default vertical geometry, never for Shift-ArrowDown or
    /// active composition. `None` preserves ordinary navigation. A returned
    /// selection is validated and installed without a document transaction,
    /// document listener notification or undo entry. Errors consume the
    /// gesture without changing canonical state.
    fn route_arrow_down(
        &self,
        _context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        Ok(None)
    }

    /// Routes the original Enter gesture before any block-kind mapping.
    ///
    /// Called outside active composition for ordinary, Shift and explicitly
    /// bound primary-modifier Enter, including code and non-code targets.
    /// `Default` keeps existing Plain/Shift behavior and propagates the
    /// primary-modifier action without consuming an outer host command.
    fn route_enter(
        &self,
        _context: EditorCommandContext<'_>,
        _source: EnterSource,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }

    /// Routes validated native structured clipboard data at a code target.
    ///
    /// The default forwards its exact plain-text projection and closed-source
    /// flag to [`Self::route_code_paste`], preserving existing routers. Hosts
    /// needing their own block-separator convention can inspect the read-only
    /// fragment tree and return one policy-validated intent. This callback
    /// never receives foreign text, images, or unvalidated clipboard metadata.
    fn route_code_slice(
        &self,
        context: EditorCommandContext<'_>,
        slice: &ClipboardSlice,
    ) -> Result<CommandRoute, PolicyError> {
        self.route_code_paste(
            context,
            slice.plain_text(),
            CodePasteSource::NativeStructuredPlainText {
                closed: slice.is_closed(),
            },
        )
    }

    /// Routes code-target paste before any newline normalization.
    ///
    /// `raw` preserves CRLF, CR and LF from platform text or the validated
    /// native slice's plain-text projection. Image bytes, image-only paste
    /// and IME transport never enter this callback. Nonempty text alongside
    /// an image is offered, but `Default` retains the original image path.
    /// For text-only data, `Default` normalizes platform text and open native
    /// slices as before; closed native slices retain `PasteSlice` semantics
    /// and generic unsupported edits fail closed. Returned intents still pass
    /// through the session policy and the ordinary undo pipeline.
    fn route_code_paste(
        &self,
        _context: EditorCommandContext<'_>,
        _raw: &str,
        _source: CodePasteSource,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }

    /// Routes a gesture using the original, validated selection and snapshot.
    fn route(
        &self,
        context: EditorCommandContext<'_>,
        command: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError>;
}
