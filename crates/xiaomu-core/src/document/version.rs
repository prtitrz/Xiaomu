//! Version and revision identifiers for canonical document snapshots.

/// Schema version of a serialized Xiaomu document.
///
/// The schema version changes only when the canonical document representation
/// requires a migration. It is distinct from `DocumentRevision`, which tracks
/// edits to one logical document instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentVersion(u32);

impl DocumentVersion {
    /// Current canonical document schema version.
    pub const CURRENT: Self = Self(1);

    /// Creates a schema-version value, including versions that may be unknown
    /// to the current implementation and therefore need migration handling.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the numeric schema version.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for DocumentVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Monotonic revision number of one logical document snapshot sequence.
///
/// Revisions are local snapshot metadata. They are not a collaboration clock,
/// distributed operation identifier, or persistence timestamp.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocumentRevision(u64);

impl DocumentRevision {
    /// Initial revision of a newly created document.
    pub const INITIAL: Self = Self(0);

    /// Returns the numeric revision.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the next revision, or `None` if the counter is exhausted.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}
