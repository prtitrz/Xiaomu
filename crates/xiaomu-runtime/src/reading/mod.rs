//! Immutable, source-mapped reading primitives independent of editor sessions.
//!
//! Projection, search and position queries never dispatch an edit or change
//! selection, marks, history, persistence or focus. Revision numbers are local
//! to a document sequence: a host must additionally bind cached results to its
//! owning session/view and discard them when that owner is replaced.

mod case_fold;
mod error;
mod position;
mod prefix;
mod projection;
mod search;
mod text_block;

pub use case_fold::UNICODE_SIMPLE_FOLD_VERSION;
pub use error::{ReadingBudget, ReadingError};
pub use prefix::{ReadingBlockPrefix, ReadingEvent, ReadingPrefix};
pub use projection::{
    AtomText, ReadingProjection, ReadingProjectionLimits, ReadingProjectionOptions,
};
pub use search::{ReadingCase, ReadingMatch, ReadingSearchLimits, ReadingSearchResults};
pub use text_block::{BoundarySide, ReadingSpan, ReadingSpanKind, ReadingTextBlock};
