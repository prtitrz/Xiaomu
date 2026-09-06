//! Table construction (P5.1).
//!
//! Table shapes cannot pass through validated `InsertNode` staging — rows
//! without cells and cells without paragraphs are invalid snapshots — so
//! Core owns whole-table construction as the semantic `InsertTable` step
//! (the way `InsertInlineAtom` is). Runtime plans it as one single-step
//! command inserted right after the focused block; the inverse removes the
//! whole subtree.

use xiaomu_core::document::NodeId;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::intent::{EditPlan, HistoryPolicy, PlannedAction, SelectionUpdate};
use super::selection::DocumentPosition;
use super::{DocumentSession, SessionError};

/// Plans a `rows × columns` table inserted right after the focused block.
///
/// Cells start as one empty paragraph each. Structural row/column operations
/// and cell navigation build on this seam in P5.2 / P5.3.
impl DocumentSession {
    pub(crate) fn plan_insert_table(
        &self,
        rows: usize,
        columns: usize,
    ) -> Result<PlannedAction, SessionError> {
        if rows == 0 || columns == 0 {
            return Err(SessionError::SelectionInvalid);
        }
        let (head, _) = self.selection.ordered(&self.document)?;
        let DocumentPosition::Inline(head) = head else {
            return Err(SessionError::SelectionInvalid);
        };
        let parent = self
            .document
            .parent_of(head.node_id())
            .ok_or(SessionError::SelectionInvalid)?;
        let position = self
            .document
            .node(parent)
            .and_then(|parent| parent.content().as_children().map(<[NodeId]>::to_vec))
            .ok_or(SessionError::SelectionInvalid)?
            .iter()
            .position(|child| *child == head.node_id())
            .ok_or(SessionError::SelectionInvalid)?;

        let mut transaction = Transaction::new(TransactionOrigin::UserInput);
        transaction.push_step(TransactionStep::InsertTable {
            parent,
            index: position + 1,
            rows,
            columns,
        });
        Ok(PlannedAction::Commit(
            EditPlan::new(transaction, SelectionUpdate::MapExisting, None)
                .with_history_policy(HistoryPolicy::Isolated),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{DocumentPosition, DocumentSelection, InlinePoint};
    use xiaomu_core::document::{NodeContent, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument};
    use xiaomu_core::selection::CursorAffinity;

    fn paragraph_document() -> (XiaomuDocument, xiaomu_core::document::NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                xiaomu_core::document::NodeAttrs::empty(),
                NodeContent::Inline(
                    xiaomu_core::document::InlineContent::new([TextRun::new(
                        "前",
                        Default::default(),
                    )
                    .unwrap()])
                    .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                xiaomu_core::document::NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
        )
    }

    #[test]
    fn insert_table_builds_a_valid_whole_subtree() {
        let (document, paragraph) = paragraph_document();
        let selection = DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
            paragraph,
            xiaomu_core::text::TextBuffer::from_string("前".to_owned())
                .offset_at(0)
                .unwrap(),
            0,
            CursorAffinity::Before,
        )));
        let mut session = DocumentSession::new(document, selection).unwrap();

        match session.plan_insert_table(2, 3).unwrap() {
            PlannedAction::Commit(plan) => {
                session.commit(plan).unwrap();
            }
            PlannedAction::CommitStaged(_) | PlannedAction::NoChange => {
                panic!("single-step plan expected")
            }
        }

        let document = session.document();
        document.validate().unwrap();
        let children = document
            .node(document.root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec();
        assert_eq!(children.len(), 2, "paragraph then table");
        let table = children[1];
        assert!(matches!(
            document.node(table).unwrap().kind(),
            NodeKind::Table
        ));
        let rows = document
            .node(table)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .to_vec();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert!(matches!(
                document.node(row).unwrap().kind(),
                NodeKind::TableRow
            ));
            let cells = document
                .node(row)
                .unwrap()
                .content()
                .as_children()
                .unwrap()
                .to_vec();
            assert_eq!(cells.len(), 3);
            for cell in cells {
                assert!(matches!(
                    document.node(cell).unwrap().kind(),
                    NodeKind::TableCell
                ));
                let blocks = document
                    .node(cell)
                    .unwrap()
                    .content()
                    .as_children()
                    .unwrap();
                assert_eq!(blocks.len(), 1, "one empty paragraph per cell");
            }
        }

        // Removing the whole table removes the subtree and validates.
        let removed = xiaomu_core::transaction::Transaction::new(
            xiaomu_core::transaction::TransactionOrigin::UserInput,
        )
        .with_step(xiaomu_core::transaction::TransactionStep::RemoveNode { node: table })
        .apply_with_changes(document)
        .unwrap();
        removed.document().validate().unwrap();
        assert!(removed.document().node(table).is_none());
    }
}
