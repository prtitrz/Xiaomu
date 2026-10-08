//! Process-local session identity, independent of document IDs and revisions.

use std::{fmt, sync::Arc};

use super::DocumentSession;

/// Opaque lifetime identity of one [`DocumentSession`] instance.
///
/// A new session always creates a fresh token, even over the same document.
/// Cloning this token preserves its identity and does not retain the session or
/// document. Edits, Undo/Redo, selection changes and moving the session leave
/// it unchanged. It is not serializable or a document/persistence identifier.
#[derive(Clone)]
pub struct DocumentSessionIdentity(Arc<()>);

impl DocumentSessionIdentity {
    pub(super) fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for DocumentSessionIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for DocumentSessionIdentity {}

impl fmt::Debug for DocumentSessionIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DocumentSessionIdentity(..)")
    }
}

impl DocumentSession {
    /// Returns a cloneable process-local token for this session's lifetime.
    ///
    /// Compare it together with a document revision when validating cached
    /// read-only results. In-place replacement with a new session is distinct
    /// even if the container address and revision happen to be equal.
    #[must_use]
    pub fn identity(&self) -> DocumentSessionIdentity {
        self.identity.clone()
    }
}
