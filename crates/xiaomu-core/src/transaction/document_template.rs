//! Bounded whole-document copying and exact same-lineage inverse payloads.

use std::sync::Arc;

use crate::Result;
use crate::document::{DocumentLineage, NodeId, NodeStore, XiaomuDocument};

use super::forest::{self, ForestData, budget::CaptureBudget};

/// Immutable identity-free template of a complete validated document.
///
/// Captures the root's exact attributes and every admitted descendant kind,
/// including custom nodes, mixed-inline atoms and nested spanning tables.
/// Missing/null attributes and independent marks are retained without defaults.
/// Canonical source IDs, revision, allocator and lineage are not retained.
/// Clones share the immutable captured payload.
///
/// Capture bounds one million nodes/values, 128 tree levels (root at level
/// zero), 64 attribute levels and 64 MiB of accounted node/key/string/text/mark
/// payload before cloning it. Replacement additionally applies these aggregate
/// budgets cumulatively across all replacement/restoration steps in one
/// transaction: captured templates, materialized after-states, retained
/// before-states, operation overhead and mapping entries/IDs. At most 64
/// whole-snapshot steps are admitted per transaction. Table-grid limits apply
/// separately. Other transaction steps retain their existing resource rules.
/// Bounds exclude allocator/index overhead and caller-owned snapshots; they
/// are checked admission limits, not a measurement of total process memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTemplate {
    pub(super) data: Arc<ForestData>,
}

impl DocumentTemplate {
    /// Captures a validated snapshot after bounded read-only preflight.
    /// Excessive tree/attribute depth or payload returns `SnapshotResourceLimit`;
    /// table-grid errors retain their usual typed categories. No state changes.
    pub fn capture(document: &XiaomuDocument) -> Result<Self> {
        Ok(Self {
            data: Arc::new(forest::capture(
                document.store(),
                document.root(),
                CaptureBudget::document(),
            )?),
        })
    }

    /// Number of fresh descendant identities required by replacement.
    /// The source root is excluded because the receiver retains its root ID.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.data.nodes.len() - 1
    }

    /// Accounted capture payload bytes, including the root's attributes.
    /// This excludes allocator overhead and replacement/inverse accounting.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.data.budget.bytes()
    }
}

/// Opaque, guarded exact inverse of a complete document replacement.
///
/// Only Core application can construct this payload. Both immutable stores
/// are retained with structural sharing; no source template or foreign node
/// identity is used for restoration. Application requires the exact expected
/// store and root in the same receiving lineage, regardless of store `Arc`
/// allocation or current revision. Stale or foreign application fails atomically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentRestore {
    pub(super) lineage: DocumentLineage,
    pub(super) root: NodeId,
    pub(super) expected: NodeStore,
    pub(super) replacement: NodeStore,
    pub(super) minimum_next_id: u64,
}
