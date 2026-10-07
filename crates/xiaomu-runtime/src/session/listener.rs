//! Frontend-neutral change notification seam.

use xiaomu_core::document::XiaomuDocument;

use super::selection::DocumentSelection;

/// Classifies the publication of one committed document change.
///
/// This is not a history-grouping or persistence policy. External imports
/// retain ordinary Undo/Redo, whose later publications are always local.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DocumentChangeOrigin {
    /// An ordinary edit or history traversal in this session.
    #[default]
    Local,
    /// A host explicitly accepted content from outside this editing session.
    External,
}

/// Receives change notifications from a
/// [`DocumentSession`](super::DocumentSession).
///
/// Notifications never fire for no-ops. The seam is frontend-neutral: it
/// carries Core types only, so any view layer can subscribe without pulling
/// GPUI or other frontend types into the runtime contract.
pub trait DocumentChangeListener {
    /// A committed edit, undo, or redo produced a new snapshot.
    ///
    /// `document` is the new snapshot and `selection` the selection that the
    /// session resolved for it.
    fn document_changed(&mut self, _document: &XiaomuDocument, _selection: DocumentSelection) {}

    /// A committed snapshot with its publication origin.
    ///
    /// The default forwards every origin to [`Self::document_changed`] for
    /// compatibility. A host may override this to acknowledge external
    /// content without scheduling a redundant save. Notifications run
    /// synchronously while the session is mutably borrowed; listeners must
    /// not reenter it. Rejected edits produce no notification.
    fn document_changed_with_origin(
        &mut self,
        document: &XiaomuDocument,
        selection: DocumentSelection,
        _origin: DocumentChangeOrigin,
    ) {
        self.document_changed(document, selection);
    }

    /// The selection moved without touching the document.
    fn selection_changed(&mut self, _selection: DocumentSelection) {}
}
