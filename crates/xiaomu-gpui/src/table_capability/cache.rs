//! One bounded current-snapshot cache, shared by render and native input gates.
//!
//! Revision is only a cache discriminator, never a measurement capability.
//! Store equality also detects different reloads with the same local revision
//! and root. A cloned NodeStore shares its Arc, making normal comparisons O(1).
//! A changed snapshot rebuilds each queried table once; equal structural keys
//! reuse the measured Rc immediately, including consecutive edits in one frame.

use std::{collections::HashMap, rc::Rc};

use xiaomu_core::document::{DocumentRevision, NodeId, NodeKind, NodeStore, XiaomuDocument};
use xiaomu_runtime::session::DocumentSelection;

use super::{TableCapabilityKey, TableLayoutError, build_key};

#[derive(Debug)]
struct CachedTable {
    key: Result<Rc<TableCapabilityKey>, TableLayoutError>,
    admitted: bool,
}

#[derive(Debug)]
struct Snapshot {
    revision: DocumentRevision,
    root: NodeId,
    store: NodeStore,
    parents: HashMap<NodeId, NodeId>,
    tables: HashMap<NodeId, CachedTable>,
    selection: Option<(DocumentSelection, bool)>,
}

#[derive(Debug, Default)]
pub(super) struct CapabilityCache {
    snapshot: Option<Snapshot>,
    measured: HashMap<NodeId, Rc<TableCapabilityKey>>,
    #[cfg(test)]
    counts: (usize, usize, usize),
}

impl CapabilityCache {
    pub(super) fn prepare(&mut self, document: &XiaomuDocument) {
        if let Some(snapshot) = &mut self.snapshot
            && snapshot.revision == document.revision()
            && snapshot.root == document.root()
            && snapshot.store == *document.store()
        {
            // Equal independently loaded stores may have different allocations.
            // Adopt the newest clone so only that first check needs deep Eq.
            snapshot.store = document.store().clone();
            return;
        }
        #[cfg(test)]
        {
            self.counts.0 += 1;
        }
        self.measured.retain(|table, _| {
            document
                .node(*table)
                .is_some_and(|node| node.kind() == &NodeKind::Table)
        });
        let mut parents = HashMap::with_capacity(document.node_count());
        for node in document.store().iter() {
            if let Some(children) = node.content().as_children() {
                parents.extend(children.iter().map(|child| (*child, node.id())));
            }
            if let Some(inline) = node.content().as_inline() {
                parents.extend(inline.atoms().iter().map(|atom| (atom.atom(), node.id())));
            }
        }
        self.snapshot = Some(Snapshot {
            revision: document.revision(),
            root: document.root(),
            store: document.store().clone(),
            parents,
            tables: HashMap::new(),
            selection: None,
        });
    }

    fn table(&mut self, document: &XiaomuDocument, table: NodeId) -> Option<&CachedTable> {
        self.prepare(document);
        // Do not retain misses for arbitrary/deleted IDs: entries are bounded
        // by the current snapshot's actual table count, errors included.
        if !document
            .node(table)
            .is_some_and(|node| node.kind() == &NodeKind::Table)
        {
            return None;
        }
        let snapshot = self.snapshot.as_mut().expect("prepared snapshot");
        Some(snapshot.tables.entry(table).or_insert_with(|| {
            #[cfg(test)]
            {
                self.counts.1 += 1;
            }
            let key = build_key(document, table).map(Rc::new);
            let measured = self.measured.get(&table);
            let admitted = key.as_ref().is_ok_and(|key| measured == Some(key));
            CachedTable {
                // Rc equality shortcuts every subsequent record/comparison.
                key: if admitted {
                    Ok(measured.expect("matching key").clone())
                } else {
                    key
                },
                admitted,
            }
        }))
    }

    pub(super) fn key(
        &mut self,
        document: &XiaomuDocument,
        table: NodeId,
    ) -> Result<Rc<TableCapabilityKey>, TableLayoutError> {
        self.table(document, table)
            .map(|entry| entry.key.clone())
            .unwrap_or(Err(TableLayoutError::InvalidDimension))
    }

    pub(super) fn permits(&mut self, document: &XiaomuDocument, table: NodeId) -> bool {
        self.table(document, table)
            .is_some_and(|entry| entry.admitted)
    }

    pub(super) fn record(&mut self, table: NodeId, key: Rc<TableCapabilityKey>) {
        let Some(entry) = self
            .snapshot
            .as_mut()
            .and_then(|snapshot| snapshot.tables.get_mut(&table))
        else {
            return;
        };
        // A delayed layout must neither admit another key nor overwrite its
        // newer success. Requiring a current entry also bounds retained IDs.
        if entry.key.as_ref().is_ok_and(|current| *current == key) {
            entry.admitted = true;
            self.measured.insert(table, key);
        }
    }

    pub(super) fn revoke(&mut self, table: NodeId) {
        self.measured.remove(&table);
        if let Some(entry) = self
            .snapshot
            .as_mut()
            .and_then(|snapshot| snapshot.tables.get_mut(&table))
        {
            entry.admitted = false;
        }
    }

    pub(super) fn parent_of(&mut self, document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
        self.prepare(document);
        self.snapshot
            .as_ref()
            .expect("prepared snapshot")
            .parents
            .get(&node)
            .copied()
    }

    pub(super) fn selection_is_valid(
        &mut self,
        document: &XiaomuDocument,
        selection: DocumentSelection,
    ) -> bool {
        self.prepare(document);
        let snapshot = self.snapshot.as_mut().expect("prepared snapshot");
        if let Some((cached, valid)) = snapshot.selection
            && cached == selection
        {
            return valid;
        }
        #[cfg(test)]
        {
            self.counts.2 += 1;
        }
        let valid = selection.validate(document).is_ok();
        snapshot.selection = Some((selection, valid));
        valid
    }

    #[cfg(test)]
    pub(super) fn counts(&self) -> (usize, usize, usize) {
        self.counts
    }

    #[cfg(test)]
    pub(super) fn sizes(&self) -> (usize, usize, usize) {
        self.snapshot
            .as_ref()
            .map_or((0, self.measured.len(), 0), |snapshot| {
                (
                    snapshot.tables.len(),
                    self.measured.len(),
                    snapshot.parents.len(),
                )
            })
    }
}
