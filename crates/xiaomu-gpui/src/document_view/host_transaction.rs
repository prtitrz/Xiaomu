//! Host-planned, one-transaction edits share native frontend protection.
use super::DocumentView;
use gpui::{App, Context, Window};
use xiaomu_core::transaction::Transaction;
use xiaomu_runtime::session::{SessionError, SessionOutcome};

impl DocumentView {
    /// Reports transient native composition without changing focus or input state.
    ///
    /// Hosts reading a separate form editor's confirmed canonical value can
    /// use this to avoid interpreting a still-virtual empty field as deletion.
    /// Normal Linux pointer dispatch unmarks the old handler before MouseDown;
    /// callers must preserve that platform ordering, not trap focus or ask an
    /// input method to wait. This also covers a cross-block range input view.
    pub fn has_active_composition(&self, cx: &App) -> bool {
        self.children
            .iter()
            .any(|(_, child)| child.read(cx).is_composing())
            || self
                .range_input
                .as_ref()
                .is_some_and(|(_, child)| child.read(cx).is_composing())
    }
    /// Applies one host-planned transaction through native composition and view routing.
    ///
    /// This is the frontend counterpart of `DocumentSession::apply`: it runs
    /// final document policy validation, not typed-intent preflight. Hosts must
    /// build the complete transaction against the current session snapshot and
    /// must not yield between checking their captured revision and this call.
    /// `None` means active composition deferred the command without any change.
    /// Errors preserve canonical state/history and do not advance the view epoch.
    /// A successful transaction is one isolated Undo unit with mapped selection.
    pub fn apply_edit_transaction(
        &mut self,
        transaction: &Transaction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<SessionOutcome>, SessionError> {
        if self.focused_child_composing(window, cx) {
            return Ok(None);
        }
        if transaction.steps().is_empty() {
            return Ok(Some(SessionOutcome::NoChange));
        }
        let outcome = self.session.borrow_mut().apply(transaction)?;
        self.desired_x = None;
        if outcome != SessionOutcome::NoChange {
            self.epoch.set(self.epoch.get() + 1);
        }
        if outcome == SessionOutcome::DocumentChanged {
            self.sync_children(cx);
            self.route_focus(window, cx);
            self.request_focus_scroll(cx);
        }
        cx.notify();
        Ok(Some(outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{EditorHooks, EditorInstance};
    use gpui::{AppContext as _, EntityInputHandler, TestAppContext};
    use xiaomu_core::{
        document::{
            InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeKind,
            NodeStoreBuilder, TextRun, XiaomuDocument,
        },
        selection::TextPoint,
        transaction::{TransactionOrigin, TransactionStep},
    };
    use xiaomu_runtime::session::{DocumentSelection, PolicyError, SessionPolicy};

    struct NoCode;
    impl SessionPolicy for NoCode {
        fn validate_document(&self, doc: &XiaomuDocument) -> Result<(), PolicyError> {
            for id in doc
                .node(doc.root())
                .unwrap()
                .content()
                .as_children()
                .unwrap()
            {
                if doc
                    .node(*id)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .runs()
                    .iter()
                    .any(|r| r.marks().contains(MarkKind::Code))
                {
                    return Err(PolicyError::new("no code"));
                }
            }
            Ok(())
        }
    }
    #[gpui::test]
    fn planned_edit_keeps_composition_guard_and_failure_epoch_then_one_undo(
        cx: &mut TestAppContext,
    ) {
        let mut builder = NodeStoreBuilder::new();
        let node = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("a", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([node]),
            )
            .unwrap();
        let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
        let inline = doc.node(node).unwrap().content().as_inline().unwrap();
        let range = xiaomu_core::text::TextRange::new(
            inline.offset_at(0).unwrap(),
            inline.offset_at(1).unwrap(),
        )
        .unwrap();
        let editor = EditorInstance::new_with_policy(
            doc.clone(),
            DocumentSelection::collapsed(TextPoint::at_start_of(node)),
            EditorHooks::default(),
            Box::new(NoCode),
        )
        .unwrap();
        let session = editor.session().clone();
        let handle = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
                .unwrap()
        });
        handle
            .update(cx, |view, window, cx| {
                window.activate_window();
                view.focus_selection(window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |view, window, cx| {
                let child = view.children[0].1.clone();
                child.update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "ni", None, window, cx)
                });
                let epoch = view.epoch.get();
                let bold = Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::AddMark {
                        node,
                        range,
                        mark: Mark::Bold,
                    },
                );
                assert_eq!(
                    view.apply_edit_transaction(&bold, window, cx).unwrap(),
                    None
                );
                assert_eq!(session.borrow().document().store(), doc.store());
                child.update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "", None, window, cx)
                });
                let code = Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::AddMark {
                        node,
                        range,
                        mark: Mark::Code,
                    },
                );
                assert!(view.apply_edit_transaction(&code, window, cx).is_err());
                assert_eq!(view.epoch.get(), epoch);
                assert_eq!(session.borrow().history_depths(), (0, 0));
                assert_eq!(
                    view.apply_edit_transaction(&bold, window, cx).unwrap(),
                    Some(SessionOutcome::DocumentChanged)
                );
                assert_eq!(view.epoch.get(), epoch + 1);
                session.borrow_mut().undo().unwrap();
                assert_eq!(session.borrow().document().store(), doc.store());
            })
            .unwrap();
    }
}
