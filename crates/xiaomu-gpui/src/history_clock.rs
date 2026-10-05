//! Frontend-owned monotonic time for opt-in Runtime typing history.
//!
//! A clock is a session-lifetime dependency, shared by every input surface.
//! Standalone views do not invent an origin: their legacy constructors remain
//! unstamped, which safely isolates ordinary typing in a timed Runtime session.

use std::{rc::Rc, time::Instant};

use xiaomu_runtime::session::HistoryTimestamp;

/// Clock used at the frontend input boundary, never by Runtime or host policy.
///
/// Return milliseconds in one nondecreasing domain for the whole session.
/// Implementations should be cheap and must not dispatch edits from `now`.
/// A manual implementation can supply deterministic timestamps without sleeps.
/// Runtime safely isolates missing or regressing stamps instead of dropping text.
pub trait HistoryClock {
    /// Samples the current time in this clock's fixed session domain.
    fn now(&self) -> HistoryTimestamp;
}

/// A clock shared by all document views and input children of one editor.
///
/// Hosts constructing standalone views over the same SharedSession must clone
/// the same provider into each view, including views materialized later.
pub type SharedHistoryClock = Rc<dyn HistoryClock>;

/// Monotonic elapsed milliseconds since construction, with a fixed origin.
///
/// Each EditorInstance creates one provider. Building another view never
/// creates another origin. Elapsed values exceeding u64 milliseconds saturate
/// at u64::MAX, preserving nondecreasing timestamps without a narrowing wrap.
#[derive(Debug)]
pub struct MonotonicHistoryClock {
    origin: Instant,
}

impl MonotonicHistoryClock {
    /// Starts a new independent session's clock domain.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicHistoryClock {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryClock for MonotonicHistoryClock {
    fn now(&self) -> HistoryTimestamp {
        timestamp_from_elapsed_millis(self.origin.elapsed().as_millis())
    }
}

fn timestamp_from_elapsed_millis(milliseconds: u128) -> HistoryTimestamp {
    HistoryTimestamp::from_millis(u64::try_from(milliseconds).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_conversion_preserves_zero_boundary_and_saturates_without_wrap() {
        for (elapsed, expected) in [
            (0, 0),
            (500, 500),
            (u128::from(u64::MAX), u64::MAX),
            (u128::from(u64::MAX) + 1, u64::MAX),
            (u128::MAX, u64::MAX),
        ] {
            assert_eq!(timestamp_from_elapsed_millis(elapsed).as_millis(), expected);
        }
    }
}
