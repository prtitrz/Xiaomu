//! Construction-time user-input boundary, independent of session admission.

use super::DocumentView;

impl DocumentView {
    // Called only while assembling a fresh EditorInstance view, before any
    // input entities or gesture callbacks exist. No mounted-mode setter exists.
    pub(crate) fn with_read_only_input(mut self, read_only: bool) -> Self {
        self.read_only_input = read_only;
        self
    }

    /// Whether this view ignores user edits before native-input side effects.
    ///
    /// Fixed by [`crate::editor::EditorInstance::with_read_only_input`]. Selection,
    /// Copy and reading operations remain enabled. This does not restrict host
    /// transactions/plans or direct session access; those require SessionPolicy.
    #[must_use]
    pub const fn is_read_only_input(&self) -> bool {
        self.read_only_input
    }
}

#[cfg(test)]
#[path = "read_only_input_tests.rs"]
mod tests;
