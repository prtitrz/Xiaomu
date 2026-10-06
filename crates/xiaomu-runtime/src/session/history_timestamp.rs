//! Explicit, session-local input time; never persisted in document or history.

/// Nonnegative milliseconds in one editing session's monotonic clock domain.
///
/// Hosts supplying timestamps must share one origin across every view of a
/// session. Zero, equal values, and `u64::MAX` are valid. Timestamps are neither
/// operation identifiers nor wall-clock dates. Runtime never samples a clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HistoryTimestamp(u64);

impl HistoryTimestamp {
    /// Wraps explicit milliseconds from the session's clock origin.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Returns the caller-supplied milliseconds.
    #[must_use]
    pub const fn as_millis(self) -> u64 {
        self.0
    }
}
