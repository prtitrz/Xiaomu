//! Table clipboard paste planning (P5.5).
//!
//! A rectangular table payload replaces an active cell range with matching
//! dimensions, enters the focused cell when it is a single cell, or inserts
//! as a sibling table of a focused plain block. Every other placement fails
//! closed. Staging keeps every intermediate snapshot valid: payload blocks
//! are inserted before displaced blocks are removed in one final stage, and
//! a pasted sibling table is built through the semantic `InsertTable` step
//! before its cells are filled.

use xiaomu_core::document::{InlineContent, NodeContent, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::TransactionStep;

use crate::clipboard::{ClipboardNode, ClipboardNodeContent, ClipboardSlice};

use super::intent::{PlannedAction, SelectionUpdate};
use super::structure::{StagedPlan, children_of, user_transaction};
use super::{DocumentPosition, DocumentSelection, SessionError};

/// Where a paste stage finds its target cell.
#[derive(Clone, Copy)]
enum CellRef {
    /// A cell whose identity is stable across all stages (range replacement
    /// and cell entry; nothing structural touches the cell before removal).
    Fixed(NodeId),
    /// A cell of the table inserted by the first stage, addressed by slot.
    Inserted {
        parent: NodeId,
        table_index: usize,
        row: usize,
        column: usize,
    },
}

fn resolve_cell(document: &XiaomuDocument, cell: CellRef) -> Result<NodeId, SessionError> {
    match cell {
        CellRef::Fixed(id) => Ok(id),
        CellRef::Inserted {
            parent,
            table_index,
            row,
            column,
        } => {
            let table = children_of(document, parent)
                .get(table_index)
                .copied()
                .ok_or(SessionError::SelectionInvalid)?;
            let row_id = children_of(document, table)
                .get(row)
                .copied()
                .ok_or(SessionError::SelectionInvalid)?;
            children_of(document, row_id)
                .get(column)
                .copied()
                .ok_or(SessionError::SelectionInvalid)
        }
    }
}

/// Plans the paste of a slice whose roots carry a table payload.
pub(crate) fn plan_table_paste(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    slice: &ClipboardSlice,
) -> Result<PlannedAction, SessionError> {
    let mut tables = slice.roots().iter().filter(|root| {
        matches!(root.kind(), NodeKind::Table) && root.content().as_table().is_some()
    });
    let table = tables
        .next()
        .ok_or(SessionError::ClipboardTableUnsupported)?;
    if tables.next().is_some() || slice.roots().len() != 1 {
        // Mixed table/text payloads have no addressed layout yet.
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let rows = table
        .content()
        .as_table()
        .ok_or(SessionError::ClipboardTableUnsupported)?;
    if rows.is_empty() || rows.iter().any(|row| row.is_empty()) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let columns = rows[0].len();
    if rows.iter().any(|row| row.len() != columns) {
        return Err(SessionError::ClipboardTableUnsupported);
    }

    if let Some(range) = selection.active_cell_range() {
        return plan_range_replacement(document, range, rows);
    }

    // The caret decides: inside a cell a 1×1 payload enters that cell; on a
    // plain block the table inserts as a sibling.
    match selection.focus() {
        DocumentPosition::Inline(point) => match focused_cell(document, point.node_id()) {
            Some(cell) if rows.len() == 1 && columns == 1 => {
                plan_cell_entry(document, cell, point.node_id(), &rows[0][0])
            }
            Some(_) => Err(SessionError::ClipboardTableUnsupported),
            None => plan_sibling_table(document, selection, rows),
        },
        _ => Err(SessionError::ClipboardTableUnsupported),
    }
}

/// The innermost table cell containing `node`, if any.
fn focused_cell(document: &XiaomuDocument, node: NodeId) -> Option<NodeId> {
    let mut current = Some(node);
    while let Some(id) = current {
        if matches!(document.node(id)?.kind(), NodeKind::TableCell) {
            return Some(id);
        }
        current = document.parent_of(id);
    }
    None
}

/// Replaces the content of every cell in the active range with the payload.
///
/// All payload blocks are inserted before any original block is removed, so
/// cells always keep at least one child; the final stage removes the original
/// blocks across all cells in one transaction.
fn plan_range_replacement(
    document: &XiaomuDocument,
    range: crate::session::CellRange,
    rows: &[Vec<ClipboardNode>],
) -> Result<PlannedAction, SessionError> {
    let rect = range_cells(document, range)?;
    if rect.len() != rows.len()
        || rect
            .iter()
            .zip(rows)
            .any(|(range_row, payload_row)| range_row.len() != payload_row.len())
    {
        return Err(SessionError::ClipboardTableUnsupported);
    }

    let mut staged = StagedPlan::new(SelectionUpdate::MapExisting);
    let mut displaced: Vec<NodeId> = Vec::new();
    for (range_row, payload_row) in rect.iter().zip(rows) {
        for (cell, payload_cell) in range_row.iter().zip(payload_row) {
            displaced.extend(children_of(document, *cell));
            staged = append_block_stages(staged, CellRef::Fixed(*cell), 0, payload_cell)?;
        }
    }
    staged = staged.stage(move |_| {
        let mut transaction = user_transaction();
        for block in &displaced {
            transaction.push_step(TransactionStep::RemoveNode { node: *block });
        }
        Ok(transaction)
    });
    Ok(PlannedAction::CommitStaged(staged))
}

/// Inserts a 1×1 payload's blocks after the focused block inside its cell.
fn plan_cell_entry(
    document: &XiaomuDocument,
    cell: NodeId,
    focused_block: NodeId,
    payload_cell: &ClipboardNode,
) -> Result<PlannedAction, SessionError> {
    let position = children_of(document, cell)
        .iter()
        .position(|block| *block == focused_block)
        .ok_or(SessionError::SelectionInvalid)?;
    let staged = append_block_stages(
        StagedPlan::new(SelectionUpdate::MapExisting),
        CellRef::Fixed(cell),
        position + 1,
        payload_cell,
    )?;
    Ok(PlannedAction::CommitStaged(staged))
}

/// Inserts the payload as a whole sibling table after the focused block.
///
/// Stage one builds the empty `rows × columns` table through the semantic
/// step; later stages fill each cell by slot, and the final stage removes
/// every cell's initial empty paragraph so the pasted content stands alone.
fn plan_sibling_table(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    rows: &[Vec<ClipboardNode>],
) -> Result<PlannedAction, SessionError> {
    let DocumentPosition::Inline(point) = selection.focus() else {
        return Err(SessionError::ClipboardTableUnsupported);
    };
    let parent = document
        .parent_of(point.node_id())
        .ok_or(SessionError::SelectionInvalid)?;
    let table_index = children_of(document, parent)
        .iter()
        .position(|block| *block == point.node_id())
        .ok_or(SessionError::SelectionInvalid)?
        + 1;
    let row_count = rows.len();
    let column_count = rows[0].len();

    let mut staged = StagedPlan::new(SelectionUpdate::MapExisting).stage(move |_| {
        Ok(user_transaction().with_step(TransactionStep::InsertTable {
            parent,
            index: table_index,
            rows: row_count,
            columns: column_count,
        }))
    });
    for (row_index, row) in rows.iter().enumerate() {
        for (column_index, payload_cell) in row.iter().enumerate() {
            let cell = CellRef::Inserted {
                parent,
                table_index,
                row: row_index,
                column: column_index,
            };
            staged = append_block_stages(staged, cell, 0, payload_cell)?;
        }
    }
    // The fills all land at position 0, so every cell's original seed
    // paragraph ends up last; the final stage resolves and removes it.
    staged = staged.stage(move |document| {
        let mut transaction = user_transaction();
        for row_index in 0..row_count {
            for column_index in 0..column_count {
                let cell = resolve_cell(
                    document,
                    CellRef::Inserted {
                        parent,
                        table_index,
                        row: row_index,
                        column: column_index,
                    },
                )?;
                let mut children = children_of(document, cell);
                let seed = children.pop().ok_or(SessionError::SelectionInvalid)?;
                transaction.push_step(TransactionStep::RemoveNode { node: seed });
            }
        }
        Ok(transaction)
    });
    Ok(PlannedAction::CommitStaged(staged))
}

/// The validated rectangle of the active cell range, row-major.
fn range_cells(
    document: &XiaomuDocument,
    range: crate::session::CellRange,
) -> Result<Vec<Vec<NodeId>>, SessionError> {
    let locate = |cell: NodeId| -> Result<(NodeId, usize, usize), SessionError> {
        let row = document
            .parent_of(cell)
            .ok_or(SessionError::SelectionInvalid)?;
        let table = document
            .parent_of(row)
            .ok_or(SessionError::SelectionInvalid)?;
        let rows = children_of(document, table);
        let row_index = rows
            .iter()
            .position(|candidate| *candidate == row)
            .ok_or(SessionError::SelectionInvalid)?;
        let cells = children_of(document, row);
        let col_index = cells
            .iter()
            .position(|candidate| *candidate == cell)
            .ok_or(SessionError::SelectionInvalid)?;
        Ok((table, row_index, col_index))
    };
    let (table, anchor_row, anchor_col) = locate(range.anchor())?;
    let (_, focus_row, focus_col) = locate(range.focus())?;
    let (row_min, row_max) = (anchor_row.min(focus_row), anchor_row.max(focus_row));
    let (col_min, col_max) = (anchor_col.min(focus_col), anchor_col.max(focus_col));

    let table_rows = children_of(document, table);
    let mut rect = Vec::new();
    for row in &table_rows[row_min..=row_max] {
        let row_cells = children_of(document, *row);
        rect.push(row_cells[col_min..=col_max].to_vec());
    }
    Ok(rect)
}

/// Appends the insert (and atom materialization) stages of one payload cell's
/// blocks at `position`.
fn append_block_stages(
    staged: StagedPlan,
    cell: CellRef,
    position: usize,
    payload_cell: &ClipboardNode,
) -> Result<StagedPlan, SessionError> {
    if !matches!(payload_cell.kind(), NodeKind::TableCell) {
        return Err(SessionError::ClipboardTableUnsupported);
    }
    let Some(children) = payload_cell.content().as_children() else {
        return Err(SessionError::ClipboardTableUnsupported);
    };
    let children = children.to_vec();
    let mut staged = staged;
    for (offset, block) in children.into_iter().enumerate() {
        let index = position + offset;
        let ClipboardNodeContent::Inline(inline) = block.content() else {
            // Nested containers inside a pasted cell are not addressed yet.
            return Err(SessionError::ClipboardTableUnsupported);
        };
        let inline = inline.clone();
        let insert_runs = inline.clone();
        staged = staged.stage(move |document| {
            let cell = resolve_cell(document, cell)?;
            Ok(user_transaction().with_step(TransactionStep::InsertNode {
                parent: cell,
                index,
                kind: block.kind().clone(),
                attrs: block.attrs().clone(),
                content: NodeContent::Inline(
                    InlineContent::new(insert_runs.runs().iter().cloned())
                        .map_err(SessionError::Core)?,
                ),
            }))
        });
        if inline.atoms().is_empty() {
            continue;
        }
        let detached = inline;
        staged = staged.stage(move |document| {
            let cell = resolve_cell(document, cell)?;
            let block = children_of(document, cell)
                .get(index)
                .copied()
                .ok_or(SessionError::SelectionInvalid)?;
            let target = document
                .node(block)
                .and_then(|node| node.content().as_inline().cloned())
                .ok_or(SessionError::SelectionInvalid)?;
            let mut transaction = user_transaction();
            let mut emitted: Vec<usize> = Vec::new();
            for atom in detached.atoms() {
                let raw = atom.anchor().as_usize();
                let ordinal = emitted.iter().filter(|anchor| **anchor == raw).count();
                emitted.push(raw);
                transaction.push_step(TransactionStep::InsertInlineAtom {
                    at: InlinePoint::new(
                        block,
                        target.offset_at(raw).map_err(SessionError::Core)?,
                        ordinal,
                        CursorAffinity::Before,
                    ),
                    kind: atom.kind().clone(),
                    attrs: atom.attrs().clone(),
                    content: atom.content().clone(),
                });
            }
            Ok(transaction)
        });
    }
    Ok(staged)
}
