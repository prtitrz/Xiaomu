//! Canonical structured document values and invariants.
//!
//! The document module is implemented in small semantic layers. P0.2A defines
//! stable value types; P0.2B adds canonical nodes, storage, validation, and
//! immutable document snapshots.

mod kind;
mod marks;
mod node_id;
mod text_run;
mod version;

pub use kind::{HeadingLevel, NodeKind};
pub use marks::{LinkMark, Mark, MarkKind, MarkSet};
pub use node_id::NodeId;
pub use text_run::TextRun;
pub use version::{DocumentRevision, DocumentVersion};
