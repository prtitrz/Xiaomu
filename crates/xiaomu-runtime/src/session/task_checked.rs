//! Identity-addressed task checkbox updates through the atomic edit pipeline.

use xiaomu_core::document::{AttrValue, NodeAttrs, NodeId, NodeKind};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::{DocumentSession, EditPlan, SelectionUpdate, SessionError, SessionOutcome};

impl DocumentSession {
    pub(super) fn set_task_checked(
        &mut self,
        item: NodeId,
        checked: bool,
    ) -> Result<SessionOutcome, SessionError> {
        let node = self
            .document
            .node(item)
            .ok_or(xiaomu_core::Error::UnknownNode)?;
        if !matches!(node.kind(), NodeKind::TaskItem) {
            return Err(xiaomu_core::Error::InvalidNodeContent.into());
        }
        if node.attrs().get("checked") == Some(&AttrValue::Bool(checked)) {
            // Like an already-effective SetMark, a true no-op preserves
            // transient typing state and the currently open history group.
            return Ok(SessionOutcome::NoChange);
        }

        // SetNodeAttrs replaces the full map, so copy unknown keys and exact
        // nested/null values before patching only the checkbox field.
        let mut values = node
            .attrs()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        values.insert("checked".into(), AttrValue::Bool(checked));
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeAttrs {
                node: item,
                attrs: NodeAttrs::new(values)?,
            },
        );
        let plan = EditPlan::new(
            transaction,
            SelectionUpdate::Exact {
                selection: self.selection,
            },
            None,
        );
        self.history.break_group();
        self.clear_stored_marks();
        // commit validates Core, the exact selection and the host's candidate
        // policy before publishing. apply_intent rolls back transient state
        // on any error, including policy rejection after marks were cleared.
        self.commit(plan)
    }
}
