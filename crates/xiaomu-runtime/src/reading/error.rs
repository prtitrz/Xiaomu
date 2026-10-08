//! Explicit failures; reading operations never return silently truncated output.

use std::fmt;

/// Independently bounded resources used by reading queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingBudget {
    /// Canonical nodes visited, including containers and inline atoms.
    Nodes,
    /// Owned projected/canonical UTF-8 bytes and cloned kind-key bytes.
    ProjectionBytes,
    /// Source-mapping segments, including omitted atoms.
    Spans,
    /// UTF-8 bytes in one literal query.
    QueryBytes,
    /// Complete, non-overlapping search results.
    Matches,
}

/// Reading failure. The snapshot and caller's previous result remain unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadingError {
    /// A budget would be exceeded; no partial projection/result is returned.
    BudgetExceeded(ReadingBudget),
    /// The allocator could not reserve a bounded output buffer.
    AllocationFailed,
    /// A point is unknown, out of range, or not a real source boundary.
    InvalidPoint,
    /// A projected offset is not a UTF-8 scalar boundary within the block.
    InvalidProjectedBoundary,
}

impl fmt::Display for ReadingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded(budget) => write!(f, "reading {budget:?} budget exceeded"),
            Self::AllocationFailed => f.write_str("reading allocation failed"),
            Self::InvalidPoint => f.write_str("invalid reading source point"),
            Self::InvalidProjectedBoundary => f.write_str("invalid reading projection boundary"),
        }
    }
}

impl std::error::Error for ReadingError {}

pub(super) fn checked_add(
    used: &mut usize,
    amount: usize,
    limit: usize,
    budget: ReadingBudget,
) -> Result<(), ReadingError> {
    *used = used
        .checked_add(amount)
        .filter(|total| *total <= limit)
        .ok_or(ReadingError::BudgetExceeded(budget))?;
    Ok(())
}
