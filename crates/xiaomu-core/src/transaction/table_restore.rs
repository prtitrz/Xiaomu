//! Exact, preconditioned inverse payload for semantic cell edits.

use crate::document::{Node, NodeId};
use crate::mapping::StepMap;

/// Opaque inverse of one merge, split, or earlier table-cell restoration.
///
/// Produced only by transaction application. Applying it requires every
/// affected live payload to match its recorded post-edit state and every
/// restored identity to be absent. Full-tree validation still runs before a
/// snapshot is published. Unaffected descendant content is never overwritten.
/// Undo after unrelated edits is permitted; stale edits to recorded rows or
/// cells fail atomically rather than clobbering them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellRestore {
    pub(super) table: NodeId,
    pub(super) expected: Vec<Node>,
    pub(super) replacement: Vec<Node>,
    pub(super) maps: Vec<StepMap>,
    pub(super) inverse_maps: Vec<StepMap>,
}
