//! After-selection resolution for a committed plan.
//!
//! Structural and text after-selection policies read [`ChangeMap`] step
//! identities or the plan's explicit caret rule. The session does not keep a
//! second implicit offset-patching path.

use xiaomu_core::document::{NodeId, XiaomuDocument};
use xiaomu_core::mapping::{ChangeMap, StepMap};
use xiaomu_core::selection::{CursorAffinity, TextPoint};

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
        SelectionUpdate::CaretAfterReplacement | SelectionUpdate::CaretAtEditStart => {
            let edit = plan.primary_edit().ok_or(SessionError::SelectionInvalid)?;
            let raw = match plan.selection_update() {
                SelectionUpdate::CaretAfterReplacement => {
                    edit.range().start().as_usize() + edit.inserted_len()
                }
                _ => edit.range().start().as_usize(),
            };
            collapsed_caret(document, edit.node(), raw, affinity_of(before))
        }
        SelectionUpdate::CaretAtLastInsertedOffset { offset } => {
            let inserted = changes
                .steps()
                .iter()
                .rev()
                .find_map(|step| match step {
                    StepMap::NodeInserted { inserted, .. } => Some(*inserted),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            collapsed_caret(document, inserted, *offset, affinity_of(before))
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
                    StepMap::NodeSplit { inserted, .. } => Some(*inserted),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            collapsed_caret(document, inserted, 0, affinity_of(before))
        }
        SelectionUpdate::CaretAtJoinSeam => {
            let (first, first_len) = changes
                .steps()
                .iter()
                .rev()
                .find_map(|step| match step {
                    StepMap::NodeJoined {
                        first, first_len, ..
                    } => Some((*first, *first_len)),
                    _ => None,
                })
                .ok_or(SessionError::SelectionInvalid)?;
            collapsed_caret(document, first, first_len, affinity_of(before))
        }
        // Single-transaction plans may promise PreserveFocus when the
        // focused block's identity survives a structural move (lift out,
        // outdent); staged list commands resolve the same policy in
        // `commit_staged`.
        SelectionUpdate::PreserveFocus => preserved_focus(before, document),
    }
}

/// Focus affinity of `selection`, defaulting to Before at a gap.
pub(super) fn affinity_of(selection: DocumentSelection) -> CursorAffinity {
    match selection.focus() {
        DocumentPosition::Text(point) => point.affinity(),
        DocumentPosition::Gap(_) => CursorAffinity::Before,
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

/// Collapses the caret onto the focus endpoint's node and offset, validated
/// against the post-command snapshot.
pub(super) fn preserved_focus(
    before: DocumentSelection,
    document: &XiaomuDocument,
) -> Result<DocumentSelection, SessionError> {
    let point = match before.focus() {
        DocumentPosition::Text(point) => point,
        DocumentPosition::Gap(_) => return Err(SessionError::SelectionInvalid),
    };
    let selection = DocumentSelection::collapsed(point);
    selection
        .validate(document)
        .map_err(|_| SessionError::SelectionInvalid)?;
    Ok(selection)
}
