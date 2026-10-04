//! Clipboard projection methods attached to [`DocumentSession`].

use crate::clipboard::{ClipboardExportPurpose, ClipboardSlice, export_selection, slice_selection};
use crate::session::{DocumentSession, SessionError};

impl DocumentSession {
    /// Projects the current non-collapsed selection into a detached clipboard
    /// slice.
    ///
    /// The result carries both a plain-text fallback and structured block
    /// slices with marks. A collapsed selection returns `Ok(None)`.
    pub fn clipboard_slice(&self) -> Result<Option<ClipboardSlice>, SessionError> {
        self.clipboard_slice_for(ClipboardExportPurpose::Copy)
    }

    /// Preflights the requested Copy/Cut purpose before detached projection.
    ///
    /// A policy rejection leaves both canonical state and the caller's platform
    /// clipboard untouched. Call this before writing for Cut, not the Copy
    /// convenience method. Successful projection does not itself delete source.
    pub fn clipboard_slice_for(
        &self,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardSlice>, SessionError> {
        match self.clipboard_export_spec(purpose)? {
            None => slice_selection(self.document(), self.selection()),
            Some(spec) => export_selection(self.document(), self.selection(), purpose, spec),
        }
    }
}
