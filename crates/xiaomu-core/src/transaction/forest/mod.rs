//! Shared identity-free capture and materialization for canonical trees.

pub(super) mod budget;

use std::collections::BTreeMap;

use crate::document::{
    InlineAtomContent, InlineAtomPlacement, InlineContent, Node, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStore, TableGrid, TableGridBudget,
};
use crate::text::TextOffset;
use crate::{Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ForestData {
    pub(super) nodes: Vec<TemplateNode>,
    pub(super) grids: Vec<GridUsage>,
    pub(super) budget: budget::CaptureBudget,
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

pub(super) fn capture(
    store: &NodeStore,
    root: NodeId,
    mut budget: budget::CaptureBudget,
) -> Result<ForestData> {
    let (ordered, local, grids) = inspect(store, root, &mut budget)?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(ordered.len())
        .map_err(|_| budget.limit())?;
    for id in ordered {
        let node = store.get(id).ok_or(Error::UnknownNode)?;
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
    Ok(ForestData {
        nodes,
        grids,
        budget,
    })
}

/// Traversal/accounting only: no node payload clones.
type TreeIndex = (Vec<NodeId>, BTreeMap<NodeId, usize>, Vec<GridUsage>);

fn inspect(
    store: &NodeStore,
    root: NodeId,
    budget: &mut budget::CaptureBudget,
) -> Result<TreeIndex> {
    let mut grid_budget = TableGridBudget::default();
    let mut local = BTreeMap::new();
    let mut ordered = Vec::new();
    let mut grids = Vec::new();
    let mut pending = vec![(root, 0)];
    while let Some((id, depth)) = pending.pop() {
        let node = store.get(id).ok_or(Error::UnknownNode)?;
        budget.node(node, depth)?;
        if local.insert(id, ordered.len()).is_some() {
            return Err(Error::InvalidTransaction);
        }
        ordered.push(id);
        if matches!(node.kind(), NodeKind::Table) {
            let grid = TableGrid::from_store(store, id, &mut grid_budget)?;
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
    Ok((ordered, local, grids))
}

pub(super) fn account_document(
    store: &NodeStore,
    root: NodeId,
    budget: &mut budget::CaptureBudget,
) -> Result<()> {
    if !matches!(
        store.get(root).ok_or(Error::UnknownNode)?.kind(),
        NodeKind::Document
    ) {
        return Err(Error::InvalidRootNode);
    }
    let (ordered, _, _) = inspect(store, root, budget)?;
    if ordered.len() != store.len() {
        return Err(Error::UnreachableNode);
    }
    Ok(())
}

/// Shared payload copy; the caller chooses which local identities are retained.
pub(super) fn materialize_node(
    node: &TemplateNode,
    id: NodeId,
    fresh: impl Fn(usize) -> Result<NodeId>,
) -> Result<Node> {
    let content = match &node.content {
        TemplateContent::Children(children) => NodeContent::children(
            children
                .iter()
                .map(|child| fresh(*child))
                .collect::<Result<Vec<_>>>()?,
        ),
        TemplateContent::Inline { text, atoms } => {
            let placements = atoms
                .iter()
                .map(|(local, offset)| Ok(InlineAtomPlacement::new(fresh(*local)?, *offset)))
                .collect::<Result<Vec<_>>>()?;
            NodeContent::Inline(text.with_replaced_atom_placements(placements)?)
        }
        TemplateContent::InlineAtom(content) => NodeContent::InlineAtom(content.clone()),
        TemplateContent::Atomic => NodeContent::Atomic,
    };
    Node::new(id, node.kind.clone(), node.attrs.clone(), content)
}
