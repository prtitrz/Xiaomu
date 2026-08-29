//! Clipboard projection methods attached to [`DocumentSession`].

use crate::clipboard::{ClipboardSlice, slice_selection};
use crate::session::{DocumentSession, SessionError};

impl DocumentSession {
    /// Projects the current non-collapsed selection into a detached clipboard
    /// slice.
    ///
    /// The result carries both a plain-text fallback and structured block
    /// slices with marks. A collapsed selection returns `Ok(None)`.
    pub fn clipboard_slice(&self) -> Result<Option<ClipboardSlice>, SessionError> {
        slice_selection(self.document(), self.selection())
    }
}
