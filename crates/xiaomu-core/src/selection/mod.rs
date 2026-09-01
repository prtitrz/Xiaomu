//! Document positions and selection semantics.
//!
//! This module owns typed text/structural positions and validation against a
//! document snapshot. Visual caret projection remains a frontend concern.

mod affinity;
mod gap;
mod inline_point;
mod model;
mod point;

pub use affinity::CursorAffinity;
pub use gap::NodeGap;
pub use inline_point::InlinePoint;
pub use model::{NodeSelection, TextSelection};
pub use point::TextPoint;
