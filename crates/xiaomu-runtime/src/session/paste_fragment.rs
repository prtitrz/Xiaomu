//! Rebuilds complete detached subtrees through validated, hidden stages.
//! Table construction is semantic; every other node uses ordinary Core steps.
//! Paths name fresh descendants without leaking or guessing canonical identities.

use super::SessionError;
use super::structure::{StagedPlan, children_of, user_transaction};
use crate::clipboard::{ClipboardInline, ClipboardNode, ClipboardNodeContent};
use xiaomu_core::document::{InlineContent, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::TransactionStep;

#[derive(Clone)]
pub(super) struct NodePath {
    root: NodeId,
    children: Vec<usize>,
}

impl NodePath {
    pub(super) fn new(root: NodeId) -> Self {
        Self {
            root,
            children: Vec::new(),
        }
    }

    pub(super) fn child(&self, index: usize) -> Self {
        let mut path = self.clone();
        path.children.push(index);
        path
    }

    pub(super) fn resolve(&self, document: &XiaomuDocument) -> Result<NodeId, SessionError> {
        let mut node = self.root;
        for index in &self.children {
            node = *children_of(document, node)
                .get(*index)
                .ok_or(SessionError::SelectionInvalid)?;
        }
        Ok(node)
    }
}

pub(super) fn append_node(
    mut staged: StagedPlan,
    parent: NodePath,
    index: usize,
    node: ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    if let ClipboardNodeContent::Table { rows, row_attrs } = node.content() {
        return append_table(staged, parent, index, &node, rows, row_attrs);
    }
    let path = parent.child(index);
    let content = match node.content() {
        ClipboardNodeContent::Inline(inline) => NodeContent::Inline(
            InlineContent::new(inline.runs().iter().cloned()).map_err(SessionError::Core)?,
        ),
        ClipboardNodeContent::Children(_) => NodeContent::children([]),
        ClipboardNodeContent::Atomic => NodeContent::Atomic,
        ClipboardNodeContent::Table { .. } => unreachable!(),
    };
    let kind = node.kind().clone();
    let attrs = node.attrs().clone();
    staged = staged.stage(move |document| {
        Ok(user_transaction().with_step(TransactionStep::InsertNode {
            parent: parent.resolve(document)?,
            index,
            kind,
            attrs,
            content,
        }))
    });
    match node.content() {
        ClipboardNodeContent::Inline(inline) if !inline.atoms().is_empty() => {
            staged = append_atoms(staged, path, inline.clone());
        }
        ClipboardNodeContent::Children(children) => {
            for (index, child) in children.iter().cloned().enumerate() {
                staged = append_node(staged, path.clone(), index, child)?;
            }
        }
        _ => {}
    }
    Ok(staged)
}

fn append_table(
    staged: StagedPlan,
    parent: NodePath,
    index: usize,
    table: &ClipboardNode,
    rows: &[Vec<ClipboardNode>],
    row_attrs: &[xiaomu_core::document::NodeAttrs],
) -> Result<StagedPlan, SessionError> {
    if !matches!(table.kind(), NodeKind::Table)
        || rows.is_empty()
        || rows[0].is_empty()
        || rows.iter().any(|row| row.len() != rows[0].len())
        || (!row_attrs.is_empty() && row_attrs.len() != rows.len())
    {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let path = parent.child(index);
    let row_count = rows.len();
    let columns = rows[0].len();
    let mut staged = staged.stage(move |document| {
        Ok(user_transaction().with_step(TransactionStep::InsertTable {
            parent: parent.resolve(document)?,
            index,
            rows: row_count,
            columns,
        }))
    });
    let attrs_path = path.clone();
    let attrs = table.attrs().clone();
    let row_attrs = row_attrs.to_vec();
    let cells = rows.to_vec();
    staged = staged.stage(move |document| {
        let mut transaction = user_transaction().with_step(TransactionStep::SetNodeAttrs {
            node: attrs_path.resolve(document)?,
            attrs,
        });
        for (row_index, row) in cells.iter().enumerate() {
            let row_path = attrs_path.child(row_index);
            if let Some(attrs) = row_attrs.get(row_index) {
                transaction.push_step(TransactionStep::SetNodeAttrs {
                    node: row_path.resolve(document)?,
                    attrs: attrs.clone(),
                });
            }
            for (column, cell) in row.iter().enumerate() {
                transaction.push_step(TransactionStep::SetNodeAttrs {
                    node: row_path.child(column).resolve(document)?,
                    attrs: cell.attrs().clone(),
                });
            }
        }
        Ok(transaction)
    });
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let cell_path = path.child(r).child(c);
            staged = append_cell_blocks(staged, cell_path, 0, cell)?;
        }
    }
    // Each fill starts at zero; the seed paragraph remains last until all
    // descendants have been reconstructed and validated.
    Ok(staged.stage(move |document| {
        let mut transaction = user_transaction();
        for r in 0..row_count {
            for c in 0..columns {
                let cell = path.child(r).child(c).resolve(document)?;
                let seed = *children_of(document, cell)
                    .last()
                    .ok_or(SessionError::SelectionInvalid)?;
                transaction.push_step(TransactionStep::RemoveNode { node: seed });
            }
        }
        Ok(transaction)
    }))
}

pub(super) fn append_cell_blocks(
    mut staged: StagedPlan,
    cell: NodePath,
    index: usize,
    payload: &ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    if !matches!(payload.kind(), NodeKind::TableCell) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let children = payload
        .content()
        .as_children()
        .ok_or(SessionError::ClipboardTableUnsupported)?;
    if children.is_empty() {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    for (offset, child) in children.iter().cloned().enumerate() {
        staged = append_node(staged, cell.clone(), index + offset, child)?;
    }
    Ok(staged)
}

fn append_atoms(staged: StagedPlan, path: NodePath, inline: ClipboardInline) -> StagedPlan {
    staged.stage(move |document| {
        let block = path.resolve(document)?;
        let target = document
            .node(block)
            .and_then(|node| node.content().as_inline())
            .ok_or(SessionError::SelectionInvalid)?;
        let mut transaction = user_transaction();
        let mut previous = None;
        let mut ordinal = 0;
        for atom in inline.atoms() {
            ordinal = if previous == Some(atom.anchor()) {
                ordinal + 1
            } else {
                0
            };
            previous = Some(atom.anchor());
            transaction.push_step(TransactionStep::InsertInlineAtom {
                at: InlinePoint::new(
                    block,
                    target
                        .offset_at(atom.anchor().as_usize())
                        .map_err(SessionError::Core)?,
                    ordinal,
                    CursorAffinity::Before,
                ),
                kind: atom.kind().clone(),
                attrs: atom.attrs().clone(),
                content: atom.content().clone(),
            });
        }
        Ok(transaction)
    })
}
