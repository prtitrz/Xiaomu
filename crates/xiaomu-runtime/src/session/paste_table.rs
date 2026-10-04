//! Table clipboard placement and rectangular content replacement.
//! Full subtree reconstruction lives in paste_fragment; geometry is shared
//! with clipboard and frontend projection through CellRange::cells.

use super::intent::{PlannedAction, SelectionUpdate};
use super::paste_fragment::{NodePath, append_cell_blocks, append_node};
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
    crate::clipboard::require_unit_tables(slice.roots())?;
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
    crate::clipboard::validate_roots(slice.roots()).map_err(SessionError::Core)?;
    if let Some(range) = selection.active_cell_range() {
        let rect = range.cells(document)?;
        if rect.len() != rows.len() || rect.iter().zip(rows).any(|(a, b)| a.len() != b.len()) {
            return Err(SessionError::ClipboardTableUnsupported);
        }
        let mut staged = StagedPlan::new(SelectionUpdate::MapExisting);
        let mut displaced = Vec::new();
        let mut attrs = Vec::new();
        for (targets, source) in rect.iter().zip(rows) {
            for (cell, payload) in targets.iter().zip(source) {
                displaced.extend(children_of(document, *cell));
                attrs.push((*cell, payload.kind().clone(), payload.attrs().clone()));
                staged = append_cell_blocks(staged, NodePath::new(*cell), 0, payload)?;
            }
        }
        staged = staged.stage(move |_| {
            let mut transaction = user_transaction();
            for (node, kind, attrs) in attrs {
                transaction.push_step(TransactionStep::SetNodeKind { node, kind });
                transaction.push_step(TransactionStep::SetNodeAttrs { node, attrs });
            }
            for node in displaced {
                transaction.push_step(TransactionStep::RemoveNode { node });
            }
            Ok(transaction)
        });
        return Ok(PlannedAction::CommitStaged(staged));
    }
    // Text-range replacement around an entire table is not yet addressed.
    // Reject it instead of unexpectedly inserting beside the selection.
    if !selection.is_collapsed() {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let DocumentPosition::Inline(point) = selection.focus() else {
        return Err(SessionError::ClipboardTableUnsupported);
    };
    if let Some(cell) = focused_cell(document, point.node_id()) {
        let row = document
            .parent_of(cell)
            .ok_or(SessionError::SelectionInvalid)?;
        let table = document
            .parent_of(row)
            .ok_or(SessionError::SelectionInvalid)?;
        super::table::require_unit_grid(document, table)?;
        if rows.len() != 1 || rows[0].len() != 1 {
            return Err(SessionError::ClipboardTableUnsupported);
        }
        // This route appends only cell content; do not silently downgrade a
        // source header (or ordinary cell) into a different destination kind.
        if document.node(cell).map(|node| node.kind()) != Some(rows[0][0].kind()) {
            return Err(SessionError::UnsupportedTableOperation);
        }
        // A caret may live under a quote/list inside the cell. Append after
        // that direct child, not after an assumed direct paragraph.
        let mut block = point.node_id();
        while document.parent_of(block) != Some(cell) {
            block = document
                .parent_of(block)
                .ok_or(SessionError::SelectionInvalid)?;
        }
        let index = children_of(document, cell)
            .iter()
            .position(|id| *id == block)
            .ok_or(SessionError::SelectionInvalid)?
            + 1;
        return Ok(PlannedAction::CommitStaged(append_cell_blocks(
            StagedPlan::new(SelectionUpdate::MapExisting),
            NodePath::new(cell),
            index,
            &rows[0][0],
        )?));
    }
    let parent = document
        .parent_of(point.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let index = children_of(document, parent)
        .iter()
        .position(|id| *id == point.node_id())
        .ok_or(SessionError::SelectionInvalid)?
        + 1;
    Ok(PlannedAction::CommitStaged(append_node(
        StagedPlan::new(SelectionUpdate::MapExisting),
        NodePath::new(parent),
        index,
        table.clone(),
    )?))
}

fn focused_cell(document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(id) = current {
        if document.node(id)?.kind().is_table_cell() {
            return Some(id);
        }
        current = document.parent_of(id);
    }
    None
}

/// Replaces every selected cell's content with the same non-table fragment.
/// Destination cell/table/row attributes remain unchanged.
pub(super) fn plan_fill_range(
    document: &XiaomuDocument,
    range: super::CellRange,
    blocks: &[ClipboardNode],
) -> Result<PlannedAction, SessionError> {
    let cells = range.cells(document)?;
    crate::clipboard::require_unit_tables(blocks)?;
    crate::clipboard::validate_roots(blocks).map_err(SessionError::Core)?;
    if blocks.is_empty() {
        return Ok(PlannedAction::NoChange);
    }
    let mut staged = StagedPlan::new(SelectionUpdate::MapExisting);
    let mut displaced = Vec::new();
    for cell in cells.into_iter().flatten() {
        displaced.extend(children_of(document, cell));
        for (index, block) in blocks.iter().cloned().enumerate() {
            staged = append_node(staged, NodePath::new(cell), index, block)?;
        }
    }
    staged = staged.stage(move |_| {
        let mut transaction = user_transaction();
        for node in displaced {
            transaction.push_step(TransactionStep::RemoveNode { node });
        }
        Ok(transaction)
    });
    Ok(PlannedAction::CommitStaged(staged))
}
