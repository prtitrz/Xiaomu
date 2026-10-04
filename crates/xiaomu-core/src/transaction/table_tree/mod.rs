//! Identity-free, bounded capture of a complete canonical table tree.

mod budget;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::document::{
    InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, TableGrid,
    TableGridBudget, XiaomuDocument,
};
use crate::text::TextOffset;
use crate::{Error, Result};

/// Immutable template for inserting a complete table with fresh identities.
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
    pub(super) data: Arc<TableTreeData>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TableTreeData {
    pub(super) nodes: Vec<TemplateNode>,
    pub(super) grids: Vec<GridUsage>,
    payload_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TemplateNode {
    pub(super) kind: NodeKind,
    pub(super) attrs: NodeAttrs,
    pub(super) content: TemplateContent,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TemplateContent {
    Inline {
        text: InlineContent,
        atoms: Vec<(usize, TextOffset)>,
    },
    Children(Vec<usize>),
    InlineAtom(InlineAtomContent),
    Atomic,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct GridUsage {
    pub(super) rows: usize,
    pub(super) columns: usize,
    pub(super) cells: usize,
}

impl TableTreeTemplate {
    /// Captures one complete Table subtree after bounded, read-only preflight.
    ///
    /// Every source node is copied at most once. Source IDs are used only
    /// during capture to build checked local references and are not retained.
    /// Non-table roots, excessive depth/payload, or invalid geometry fail
    /// without constructing a template or changing either document.
    pub fn capture(document: &XiaomuDocument, table: NodeId) -> Result<Self> {
        if !matches!(
            document.node(table).ok_or(Error::UnknownNode)?.kind(),
            NodeKind::Table
        ) {
            return Err(Error::InvalidTableStructure);
        }
        let mut budget = budget::CaptureBudget::default();
        let mut grid_budget = TableGridBudget::default();
        let mut local = BTreeMap::new();
        let mut ordered = Vec::new();
        let mut grids = Vec::new();
        let mut pending = vec![(table, 0)];
        while let Some((id, depth)) = pending.pop() {
            let node = document.node(id).ok_or(Error::UnknownNode)?;
            budget.node(node, depth)?;
            if local.insert(id, ordered.len()).is_some() {
                return Err(Error::InvalidTransaction);
            }
            ordered.push(id);
            if matches!(node.kind(), NodeKind::Table) {
                let grid = TableGrid::from_store(document.store(), id, &mut grid_budget)?;
                grids.push(GridUsage {
                    rows: grid.rows(),
                    columns: grid.columns(),
                    cells: grid.origins().len(),
                });
            }
            if let Some(children) = node.content().as_children() {
                budget.pending(ordered.len(), pending.len(), children.len())?;
                pending.extend(children.iter().rev().map(|child| (*child, depth + 1)));
            }
            if let Some(inline) = node.content().as_inline() {
                budget.pending(ordered.len(), pending.len(), inline.atoms().len())?;
                pending.extend(
                    inline
                        .atoms()
                        .iter()
                        .rev()
                        .map(|atom| (atom.atom(), depth + 1)),
                );
            }
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(ordered.len())
            .map_err(|_| Error::TableResourceLimit)?;
        for id in ordered {
            let node = document.node(id).ok_or(Error::UnknownNode)?;
            let content = match node.content() {
                NodeContent::Children(children) => TemplateContent::Children(
                    children
                        .iter()
                        .map(|id| local.get(id).copied().ok_or(Error::UnknownNode))
                        .collect::<Result<_>>()?,
                ),
                NodeContent::Inline(inline) => TemplateContent::Inline {
                    // Source runs are already normalized, so this does not
                    // repeatedly concatenate adjacent equivalent mark runs.
                    text: InlineContent::new(inline.runs().iter().cloned())?,
                    atoms: inline
                        .atoms()
                        .iter()
                        .map(|atom| {
                            Ok((
                                *local.get(&atom.atom()).ok_or(Error::UnknownNode)?,
                                atom.text_offset(),
                            ))
                        })
                        .collect::<Result<_>>()?,
                },
                NodeContent::InlineAtom(atom) => TemplateContent::InlineAtom(atom.clone()),
                NodeContent::Atomic => TemplateContent::Atomic,
            };
            nodes.push(TemplateNode {
                kind: node.kind().clone(),
                attrs: node.attrs().clone(),
                content,
            });
        }
        Ok(Self {
            data: Arc::new(TableTreeData {
                nodes,
                grids,
                payload_bytes: budget.bytes(),
            }),
        })
    }

    /// Returns the number of fresh canonical identities application allocates.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.data.nodes.len()
    }

    /// Returns the accounted owned payload bytes checked during capture.
    /// This is not a measurement of allocator or whole-process memory.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.data.payload_bytes
    }
}
