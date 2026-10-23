//! Table paste: new tables preserve all structure attributes; replacement
//! of an existing range changes cell contents and preserves target attrs.

use super::cell_range::range_cells;
use super::intent::{PlannedAction, SelectionUpdate};
use super::paste_fragment::{FragmentPath, append_fragment};
use super::structure::{StagedPlan, children_of, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};
use crate::clipboard::{ClipboardNode, ClipboardSlice};
use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::transaction::TransactionStep;

pub(crate) fn plan_table_paste(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    slice: &ClipboardSlice,
) -> Result<PlannedAction, SessionError> {
    let [table] = slice.roots() else {
        return Err(SessionError::ClipboardTableUnsupported);
    };
    if !matches!(table.kind(), NodeKind::Table) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let rows = table
        .content()
        .as_table()
        .ok_or(SessionError::ClipboardTableUnsupported)?;
    let columns = rows.first().map_or(0, Vec::len);
    if columns == 0 || rows.iter().any(|row| row.len() != columns) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    if let Some(range) = selection.active_cell_range() {
        let rect = range_cells(document, range)?;
        if rect.len() != rows.len() || rect.iter().any(|row| row.len() != columns) {
            return Err(SessionError::ClipboardTableUnsupported);
        }
        let mut staged = StagedPlan::new(SelectionUpdate::MapExisting);
        let mut displaced = Vec::new();
        for (targets, sources) in rect.iter().zip(rows) {
            for (cell, payload) in targets.iter().zip(sources) {
                displaced.extend(children_of(document, *cell));
                staged = append_cell_contents(staged, *cell, 0, payload)?;
            }
        }
        staged = staged.stage(move |_| {
            let mut transaction = user_transaction();
            for node in displaced {
                transaction.push_step(TransactionStep::RemoveNode { node });
            }
            Ok(transaction)
        });
        return Ok(PlannedAction::CommitStaged(staged));
    }
    let DocumentPosition::Inline(point) = selection.focus() else {
        return Err(SessionError::ClipboardTableUnsupported);
    };
    let mut block = point.node_id();
    let mut ancestor = document.parent_of(block);
    while let Some(parent) = ancestor {
        if matches!(
            document.node(parent).map(|node| node.kind()),
            Some(NodeKind::TableCell)
        ) {
            if rows.len() != 1 || columns != 1 {
                return Err(SessionError::ClipboardTableUnsupported);
            }
            // A caret in a quote/list enters after its top-level cell block.
            let index = children_of(document, parent)
                .iter()
                .position(|child| *child == block)
                .ok_or(SessionError::SelectionInvalid)?
                + 1;
            return Ok(PlannedAction::CommitStaged(append_cell_contents(
                StagedPlan::new(SelectionUpdate::MapExisting),
                parent,
                index,
                &rows[0][0],
            )?));
        }
        block = parent;
        ancestor = document.parent_of(parent);
    }
    let parent = document
        .parent_of(point.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let index = children_of(document, parent)
        .iter()
        .position(|child| *child == point.node_id())
        .ok_or(SessionError::SelectionInvalid)?
        + 1;
    Ok(PlannedAction::CommitStaged(append_fragment(
        StagedPlan::new(SelectionUpdate::MapExisting),
        FragmentPath::new(parent, index),
        table.clone(),
    )?))
}

fn append_cell_contents(
    mut staged: StagedPlan,
    cell: NodeId,
    index: usize,
    payload: &ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    let children = payload
        .content()
        .as_children()
        .ok_or(SessionError::ClipboardTableUnsupported)?;
    if !matches!(payload.kind(), NodeKind::TableCell) || children.is_empty() {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    for (offset, block) in children.iter().enumerate() {
        staged = append_fragment(
            staged,
            FragmentPath::new(cell, index + offset),
            block.clone(),
        )?;
    }
    Ok(staged)
}
