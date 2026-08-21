//! Document positions and selection semantics.
//!
//! This module owns typed text/structural positions and validation against a
//! document snapshot. Visual caret projection remains a frontend concern.

mod affinity;
mod gap;
mod model;
mod point;

pub use affinity::CursorAffinity;
pub use gap::NodeGap;
pub use model::{NodeSelection, TextSelection};
pub use point::TextPoint;
