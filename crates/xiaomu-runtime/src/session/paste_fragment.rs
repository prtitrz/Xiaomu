//! Rebuilds detached subtrees with fresh identities through validated stages.
//!
//! Paths are relative to an existing stable parent. Tables are constructed
//! with their semantic Core step before recursively filling cells, so every
//! staged snapshot remains valid, including nested tables and atomic blocks.

use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::TransactionStep;

use super::SessionError;
use super::structure::{StagedPlan, children_of, user_transaction};
use crate::clipboard::{ClipboardNode, ClipboardNodeContent};

#[derive(Clone)]
pub(super) struct FragmentPath {
    root: NodeId,
    indices: Vec<usize>,
}

impl FragmentPath {
    pub(super) fn new(root: NodeId, index: usize) -> Self {
        Self {
            root,
            indices: vec![index],
        }
    }

    fn child(&self, index: usize) -> Self {
        let mut path = self.clone();
        path.indices.push(index);
        path
    }

    fn resolve(&self, document: &XiaomuDocument) -> Result<NodeId, SessionError> {
        self.indices.iter().try_fold(self.root, |parent, index| {
            children_of(document, parent)
                .get(*index)
                .copied()
                .ok_or(SessionError::SelectionInvalid)
        })
    }

    fn insertion(&self, document: &XiaomuDocument) -> Result<(NodeId, usize), SessionError> {
        let mut parent = self.clone();
        let index = parent.indices.pop().ok_or(SessionError::SelectionInvalid)?;
        Ok((parent.resolve(document)?, index))
    }
}

pub(super) fn append_fragment(
    staged: StagedPlan,
    path: FragmentPath,
    node: ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    if let ClipboardNodeContent::Table { rows, row_attrs } = node.content() {
        return append_table(staged, path, node.attrs().clone(), rows, row_attrs);
    }
    let content = match node.content() {
        ClipboardNodeContent::Inline(inline) => NodeContent::Inline(
            InlineContent::new(inline.runs().iter().cloned()).map_err(SessionError::Core)?,
        ),
        ClipboardNodeContent::Children(_) => NodeContent::children([]),
        ClipboardNodeContent::Atomic => NodeContent::Atomic,
        _ => return Err(SessionError::ClipboardTableUnsupported),
    };
    let kind = node.kind().clone();
    let attrs = node.attrs().clone();
    let insert_path = path.clone();
    let mut staged = staged.stage(move |document| {
        let (parent, index) = insert_path.insertion(document)?;
        Ok(user_transaction().with_step(TransactionStep::InsertNode {
            parent,
            index,
            kind,
            attrs,
            content,
        }))
    });
    match node.content() {
        ClipboardNodeContent::Children(children) => {
            for (index, child) in children.iter().enumerate() {
                staged = append_fragment(staged, path.child(index), child.clone())?;
            }
        }
        ClipboardNodeContent::Inline(inline) if !inline.atoms().is_empty() => {
            let inline = inline.clone();
            staged = staged.stage(move |document| {
                let leaf = path.resolve(document)?;
                let mut transaction = user_transaction();
                let mut previous = None;
                let mut ordinal = 0;
                for atom in inline.atoms() {
                    if previous == Some(atom.anchor()) {
                        ordinal += 1;
                    } else {
                        ordinal = 0;
                    }
                    previous = Some(atom.anchor());
                    transaction.push_step(TransactionStep::InsertInlineAtom {
                        at: InlinePoint::new(leaf, atom.anchor(), ordinal, CursorAffinity::Before),
                        kind: atom.kind().clone(),
                        attrs: atom.attrs().clone(),
                        content: atom.content().clone(),
                    });
                }
                Ok(transaction)
            });
        }
        _ => {}
    }
    Ok(staged)
}

fn set_attrs(staged: StagedPlan, path: FragmentPath, attrs: NodeAttrs) -> StagedPlan {
    staged.stage(move |document| {
        Ok(user_transaction().with_step(TransactionStep::SetNodeAttrs {
            node: path.resolve(document)?,
            attrs,
        }))
    })
}

fn append_table(
    staged: StagedPlan,
    path: FragmentPath,
    attrs: NodeAttrs,
    rows: &[Vec<ClipboardNode>],
    row_attrs: &[NodeAttrs],
) -> Result<StagedPlan, SessionError> {
    let row_count = rows.len();
    let columns = rows.first().map_or(0, Vec::len);
    if columns == 0 || row_attrs.len() != row_count || rows.iter().any(|row| row.len() != columns) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let insert_path = path.clone();
    let mut staged = staged.stage(move |document| {
        let (parent, index) = insert_path.insertion(document)?;
        Ok(user_transaction().with_step(TransactionStep::InsertTable {
            parent,
            index,
            rows: row_count,
            columns,
        }))
    });
    staged = set_attrs(staged, path.clone(), attrs);
    for (row_index, (cells, attrs)) in rows.iter().zip(row_attrs).enumerate() {
        let row_path = path.child(row_index);
        staged = set_attrs(staged, row_path.clone(), attrs.clone());
        for (column, cell) in cells.iter().enumerate() {
            let cell_path = row_path.child(column);
            let children = cell
                .content()
                .as_children()
                .ok_or(SessionError::ClipboardTableUnsupported)?;
            if !matches!(cell.kind(), NodeKind::TableCell) || children.is_empty() {
                return Err(SessionError::ClipboardTableUnsupported);
            }
            staged = set_attrs(staged, cell_path.clone(), cell.attrs().clone());
            for (index, child) in children.iter().enumerate() {
                staged = append_fragment(staged, cell_path.child(index), child.clone())?;
            }
            // Insertions precede the seed, so remove it only after the cell
            // has its complete payload. Undo restores the exact original IDs.
            let seed_index = children.len();
            staged = staged.stage(move |document| {
                Ok(user_transaction().with_step(TransactionStep::RemoveNode {
                    node: cell_path.child(seed_index).resolve(document)?,
                }))
            });
        }
    }
    Ok(staged)
}
