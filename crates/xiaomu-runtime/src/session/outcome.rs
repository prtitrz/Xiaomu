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
    /// The construction-time host policy rejected an intent or snapshot.
    Policy(super::PolicyError),
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
    /// The default planner cannot preserve the requested edit semantics.
    ///
    /// Task-containing clipboard slices require an explicit task-aware policy
    /// plan; default fitting must not downgrade them to ordinary text/lists.
    UnsupportedEdit,
    /// An optional input-rule reversal exceeds the bounded session budget,
    /// or contains a new Core payload shape this Runtime cannot account for.
    ///
    /// The host must choose a literal-input fallback or report the refusal;
    /// Runtime never silently converts while discarding requested metadata.
    InputRuleUndoBudgetExceeded,
    /// A structured paste would have to drop detached inline atoms.
    ///
    /// Multi-block and hierarchical paste cannot address freshly inserted
    /// blocks inside one declarative transaction yet, so pasting an atom
    /// into more than one block fails closed instead of silently
    /// downgrading the fragment to its plain-text fallback.
    ClipboardAtomsUnsupported,
    /// Whole-root clipboard boundaries require a host-aware closed-slice fit.
    /// Generic text fitting rejects these rather than discarding block shells.
    ClipboardClosedUnsupported,
    /// A structured paste would have to place an atomic block into a
    /// context the planner cannot address yet.
    ///
    /// Atomic blocks pasted as a whole-root fragment insert as siblings of
    /// the focused block; hierarchical containers mixing atomic and inline
    /// children fail closed instead of guessing a layout.
    ClipboardAtomicUnsupported,
    /// A structured paste would have to place a table payload into a
    /// context the planner cannot address yet.
    ///
    /// Table payloads replace a cell range with matching dimensions, enter
    /// the focused cell when they are a single cell, or insert as a sibling
    /// table of a focused plain block; every other placement fails closed
    /// instead of guessing a layout (P5.5).
    ClipboardTableUnsupported,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(f, "session policy rejected: {error}"),
            Self::Core(error) => write!(f, "core transaction rejected: {error}"),
            Self::SelectionDeleted => {
                f.write_str("selection target was deleted by the transaction")
            }
            Self::SelectionInvalid => {
                f.write_str("selection is invalid for the resulting snapshot")
            }
            Self::UnsupportedEdit => {
                f.write_str("edit requires an explicit semantics-preserving planner")
            }
            Self::InputRuleUndoBudgetExceeded => {
                f.write_str("input-rule reversal exceeds the supported session payload budget")
            }
            Self::ClipboardAtomicUnsupported => {
                f.write_str("clipboard fragment places an atomic block in an unsupported context")
            }
            Self::ClipboardClosedUnsupported => {
                f.write_str("closed clipboard boundaries require an explicit paste planner")
            }
            Self::ClipboardAtomsUnsupported => {
                f.write_str("pasting inline atoms is only supported into one inline block")
            }
            Self::ClipboardTableUnsupported => {
                f.write_str("clipboard fragment places a table payload in an unsupported context")
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

impl From<super::PolicyError> for SessionError {
    fn from(error: super::PolicyError) -> Self {
        Self::Policy(error)
    }
}
