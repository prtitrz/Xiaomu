//! Construction-time behavior for the default nonempty inline text-input plan.

/// What happens to explicit pending marks after default text input commits.
///
/// This affects only nonempty `InsertText` and `CommitComposition` handled by
/// the default single-inline planner. Host `Apply` plans, raw transactions,
/// deletion, paste, structural commands and no-ops keep their own contracts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefaultTextInputMarks {
    /// Keep the existing pending marks after the successful input.
    #[default]
    PreservePending,
    /// Use pending marks to produce the canonical input, then restore normal
    /// mark inheritance atomically before publication/listener notification.
    /// Subsequent text inherits the newly inserted marks; it is not unformatted.
    ConsumePending,
}
