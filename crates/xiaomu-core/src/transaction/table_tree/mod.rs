//! Bounded table-copy template using the generic canonical tree copier.

use super::forest::{self, ForestData, budget::CaptureBudget};
use crate::document::{NodeId, NodeKind, XiaomuDocument};
use crate::{Error, Result};
use std::sync::Arc;

/// Immutable template for copying a table or its cell forest with fresh identities.
///
/// Capture accepts a table from a validated source document. The template
/// contains only private local references, never source canonical identities.
/// Raw attributes, kinds, normalized runs, independent atom marks, atomic
/// blocks, row wrappers and nested tables are retained without default repair.
/// Cloning a template shares its immutable payload rather than copying it.
///
/// Capture preflights one million nodes/values, 128 tree levels, 64 attribute
/// levels and 64 MiB of accounted node/key/string/text/mark payload before
/// copying it. These limits are independent of table-grid budgets and exclude
/// allocator overhead, index maps and transient copies; they are not a bound
/// on all execution memory. Applying the template also checks destination
/// aggregate grids and the complete fresh identity range before mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableTreeTemplate {
    pub(super) data: Arc<ForestData>,
}

impl TableTreeTemplate {
    /// Captures a complete Table subtree after bounded read-only preflight.
    /// Non-table roots, excessive depth/payload or invalid geometry fail
    /// without copying source payloads or changing either document.
    pub fn capture(document: &XiaomuDocument, table: NodeId) -> Result<Self> {
        if !matches!(
            document.node(table).ok_or(Error::UnknownNode)?.kind(),
            NodeKind::Table
        ) {
            return Err(Error::InvalidTableStructure);
        }
        Ok(Self {
            data: Arc::new(forest::capture(
                document.store(),
                table,
                CaptureBudget::default(),
            )?),
        })
    }

    /// Captured nodes, including outer table/row wrappers. Whole-table
    /// insertion allocates this many identities; rectangle replacement omits
    /// the outer table/rows while retaining every nested wrapper.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.data.nodes.len()
    }

    /// Accounted owned payload bytes; excludes allocator/process overhead.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.data.budget.bytes()
    }
}
