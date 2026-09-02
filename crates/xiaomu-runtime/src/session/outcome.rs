//! Session outcomes and typed session errors.

use core::fmt;

/// Classification of one session operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SessionOutcome {
    /// A transaction committed and produced a new snapshot.
    DocumentChanged,
    /// Only the selection changed; the snapshot and its revision are
    /// untouched.
    SelectionChanged,
    /// The operation was a legitimate no-op: no revision advance, no
    /// notification, and no history entry.
    NoChange,
}

/// Errors produced by the session orchestration layer.
///
/// On any error the session state is left exactly as it was; nothing partial
/// escapes a failed operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionError {
    /// The underlying Core transaction was rejected.
    Core(xiaomu_core::Error),
    /// The resolved selection maps to a node deleted by the transaction.
    ///
    /// Raw applies and mark edits still fail atomically when an endpoint
    /// disappears. Structural intents use an explicit after-selection
    /// policy (join seam / new-block start) instead of this error.
    SelectionDeleted,
    /// The resolved selection is not valid for the new snapshot.
    SelectionInvalid,
    /// A structured paste would have to drop detached inline atoms.
    ///
    /// Multi-block and hierarchical paste cannot address freshly inserted
    /// blocks inside one declarative transaction yet, so pasting an atom
    /// into more than one block fails closed instead of silently
    /// downgrading the fragment to its plain-text fallback.
    ClipboardAtomsUnsupported,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(f, "core transaction rejected: {error}"),
            Self::SelectionDeleted => {
                f.write_str("selection target was deleted by the transaction")
            }
            Self::SelectionInvalid => {
                f.write_str("selection is invalid for the resulting snapshot")
            }
            Self::ClipboardAtomsUnsupported => {
                f.write_str("pasting inline atoms is only supported into one inline block")
            }
        }
    }
}

impl std::error::Error for SessionError {}

impl From<xiaomu_core::Error> for SessionError {
    fn from(error: xiaomu_core::Error) -> Self {
        Self::Core(error)
    }
}
