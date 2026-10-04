//! After-selection resolution for a committed plan.
//!
//! Structural and text after-selection policies read [`ChangeMap`] step
//! identities or the plan's explicit caret rule. The session does not keep a
//! second implicit offset-patching path.

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::mapping::{ChangeMap, StepMap};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};

use super::intent::{EditPlan, SelectionUpdate};
use super::{DocumentPosition, DocumentSelection, SessionError};

/// Resolves the after-selection of one committed plan.
pub(super) fn resolve_selection(
    plan: &EditPlan,
    changes: &ChangeMap,
    before: DocumentSelection,
    before_document: &XiaomuDocument,
    document: &XiaomuDocument,
) -> Result<DocumentSelection, SessionError> {
    match plan.selection_update() {
        SelectionUpdate::Exact { selection } => {
            selection.validate(document)?;
            Ok(*selection)
        }
        SelectionUpdate::AllDocument => Ok(DocumentSelection::all(document)),
        SelectionUpdate::CaretAfterReplacement | SelectionUpdate::CaretAtEditStart => {
            let edit = plan.primary_edit().ok_or(SessionError::SelectionInvalid)?;
            let raw = match plan.selection_update() {
                SelectionUpdate::CaretAfterReplacement => edit
                    .range()
                    .start()
                    .as_usize()
                    .checked_add(edit.inserted_len())
                    .ok_or(SessionError::SelectionInvalid)?,
                _ => edit.range().start().as_usize(),
            };
            collapsed_caret(document, edit.node(), raw, affinity_of(before))
        }
        SelectionUpdate::CaretAtLastInsertedOffset { .. }
        | SelectionUpdate::CaretAtStartOfLastInsertedSubtree => {
            let inserted = changes
                .steps()
                .iter()
                .rev()
                .find_map(|step| match step {
                    StepMap::NodeInserted { inserted, .. } => Some(*inserted),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            match plan.selection_update() {
                SelectionUpdate::CaretAtLastInsertedOffset { offset } => {
                    collapsed_caret(document, inserted, *offset, affinity_of(before))
                }
                _ => {
                    let mut pending = vec![inserted];
                    while let Some(node) = pending.pop() {
                        let content = document
                            .node(node)
                            .ok_or(SessionError::SelectionInvalid)?
                            .content();
                        if content.as_inline().is_some() {
                            return collapsed_caret(document, node, 0, affinity_of(before));
                        }
                        if let Some(children) = content.as_children() {
                            pending.extend(children.iter().rev().copied());
                        }
                    }
                    Err(SessionError::SelectionInvalid)
                }
            }
        }
        SelectionUpdate::CaretAtJoinPoint => {
            let edit = plan.primary_edit().ok_or(SessionError::SelectionInvalid)?;
            collapsed_caret(
                document,
                edit.node(),
                edit.range().start().as_usize(),
                affinity_of(before),
            )
        }
        SelectionUpdate::MapExisting => {
            let mapped = before.map_through(changes, before_document)?;
            mapped
                .validate(document)
                .map_err(|_| SessionError::SelectionInvalid)?;
            Ok(mapped)
        }
        SelectionUpdate::CaretAtSplitTail => {
            let inserted = changes
                .steps()
                .iter()
                .rev()
                .find_map(|step| match step {
                    StepMap::NodeSplit { inserted, .. }
                    | StepMap::InlineNodeSplit { inserted, .. } => Some(*inserted),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            collapsed_caret(document, inserted, 0, affinity_of(before))
        }
        SelectionUpdate::CaretAtJoinSeam => {
            let (first, first_len, atom_index) = changes
                .steps()
                .iter()
                .rev()
                .find_map(|step| match step {
                    StepMap::NodeJoined {
                        first, first_len, ..
                    } => Some((*first, *first_len, 0)),
                    StepMap::InlineNodeJoined {
                        first,
                        first_len,
                        seam_atom_index,
                        ..
                    } => Some((*first, *first_len, *seam_atom_index)),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            collapsed_inline_caret(document, first, first_len, atom_index, affinity_of(before))
        }
        SelectionUpdate::CaretAtInline { caret } => {
            let selection = DocumentSelection::collapsed(*caret);
            selection
                .validate(document)
                .map_err(|_| SessionError::SelectionInvalid)?;
            Ok(selection)
        }
        // Single-transaction plans may promise PreserveFocus when the
        // focused block's identity survives a structural move (lift out,
        // outdent); staged list commands resolve the same policy in
        // `commit_staged`.
        SelectionUpdate::PreserveFocus => preserved_focus(before, document),
        SelectionUpdate::PreserveSelection => preserved_selection(before, document),
        SelectionUpdate::CaretAtGap { gap } => {
            gap.validate(document)
                .map_err(|_| SessionError::SelectionInvalid)?;
            Ok(DocumentSelection::collapsed(*gap))
        }
    }
}

/// Retains the full selection in the final snapshot, irrespective of temporary
/// removals. Explicit node identity refreshes its surrounding gaps; all other
/// forms retain every coordinate exactly and must remain valid.
pub(super) fn preserved_selection(
    before: DocumentSelection,
    document: &XiaomuDocument,
) -> Result<DocumentSelection, SessionError> {
    if let Some(selection) = before.preserved_node_selection(document)? {
        return Ok(selection);
    }
    before
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    Ok(before)
}

/// Focus affinity of `selection`, defaulting to Before at a gap.
pub(super) fn affinity_of(selection: DocumentSelection) -> CursorAffinity {
    match selection.focus() {
        DocumentPosition::Inline(point) => point.affinity(),
        DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => CursorAffinity::Before,
    }
}

/// Collapsed caret at `raw` on `node`, validated for `document`.
pub(super) fn collapsed_caret(
    document: &XiaomuDocument,
    node: NodeId,
    raw: usize,
    affinity: CursorAffinity,
) -> Result<DocumentSelection, SessionError> {
    let inline = document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
    let selection = DocumentSelection::collapsed(TextPoint::new(node, offset, affinity));
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    Ok(selection)
}

/// Resolves a canonical atom seam without projecting its ordinal away.
fn collapsed_inline_caret(
    document: &XiaomuDocument,
    node: NodeId,
    raw: usize,
    atom_index: usize,
    affinity: CursorAffinity,
) -> Result<DocumentSelection, SessionError> {
    let inline = document
        .node(node)
        .ok_or(SessionError::Core(xiaomu_core::Error::UnknownNode))?
        .content()
        .as_inline()
        .ok_or(SessionError::SelectionInvalid)?;
    let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
    let selection = DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
        node, offset, atom_index, affinity,
    )));
    selection.validate(document)?;
    Ok(selection)
}

/// Collapses the caret onto the focus endpoint's node and offset, validated
/// against the post-command snapshot.
pub(super) fn preserved_focus(
    before: DocumentSelection,
    document: &XiaomuDocument,
) -> Result<DocumentSelection, SessionError> {
    let point = match before.focus() {
        DocumentPosition::Inline(point) => point,
        // Structural endpoints have no inline coordinate to preserve.
        DocumentPosition::Gap(_) | DocumentPosition::Atomic(_) => {
            return Err(SessionError::SelectionInvalid);
        }
    };
    let selection = DocumentSelection::collapsed(point);
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    Ok(selection)
}
