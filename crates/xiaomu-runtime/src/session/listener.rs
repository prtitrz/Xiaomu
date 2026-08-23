//! Frontend-neutral change notification seam.

use xiaomu_core::document::XiaomuDocument;
use xiaomu_core::selection::TextSelection;

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
    fn document_changed(&mut self, _document: &XiaomuDocument, _selection: TextSelection) {}

    /// The selection moved without touching the document.
    fn selection_changed(&mut self, _selection: TextSelection) {}
}
