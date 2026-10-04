//! Native HR pointer routing opts into node identity without changing defaults.

use super::*;
use crate::editor_commands::{CommandRoute, EditorCommand, EditorCommandRouter};
use xiaomu_core::document::Mark;

enum AtomicResponse {
    Default,
    Node,
    Exact(DocumentSelection),
    Error,
}
type PointerCalls = Rc<RefCell<Vec<(DocumentSelection, NodeId)>>>;
struct AtomicRouter {
    response: AtomicResponse,
    calls: PointerCalls,
}
impl EditorCommandRouter for AtomicRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
    fn select_atomic(
        &self,
        context: EditorCommandContext<'_>,
        node: NodeId,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.calls.borrow_mut().push((context.selection(), node));
        match self.response {
            AtomicResponse::Default => Ok(None),
            AtomicResponse::Node => DocumentSelection::node(context.document(), node)
                .map(Some)
                .map_err(|error| PolicyError::new(error.to_string())),
            AtomicResponse::Exact(selection) => Ok(Some(selection)),
            AtomicResponse::Error => Err(PolicyError::new("fixture atomic routing rejection")),
        }
    }
}

fn click_rule(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, node: NodeId) {
    let point = bounds(cx, handle, "atomic-block", node).center();
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
        point,
        gpui::MouseButton::Left,
        Default::default(),
    );
    cx.background_executor.run_until_parked();
}

#[gpui::test]
fn native_rule_pointer_uses_opt_in_node_selection_and_preserves_default_atomic(
    cx: &mut TestAppContext,
) {
    for explicit in [false, true] {
        let f = fixture();
        let (handle, session, counts) = open(cx, &f, None);
        let before = session.borrow().selection();
        let calls = Rc::new(RefCell::new(Vec::new()));
        handle
            .update(cx, |view, _, _| {
                view.set_command_router(Some(Rc::new(AtomicRouter {
                    response: if explicit {
                        AtomicResponse::Node
                    } else {
                        AtomicResponse::Default
                    },
                    calls: calls.clone(),
                })))
            })
            .unwrap();
        click_rule(cx, handle, f.rule);
        assert_eq!(calls.borrow().as_slice(), &[(before, f.rule)]);
        let selection = session.borrow().selection();
        if explicit {
            assert_eq!(selection.as_node_selection(), Some(f.rule));
            assert_eq!(selection.as_atomic_node(), None);
        } else {
            assert_eq!(selection.as_atomic_node(), Some(f.rule));
            assert_eq!(selection.as_node_selection(), None);
        }
        handle
            .update(cx, |view, window, cx| {
                if explicit {
                    assert!(view.range_input_is_focused(window, cx));
                } else {
                    assert!(view.range_input.is_none());
                    assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
                }
            })
            .unwrap();
        assert_eq!(counts.get(), (0, 1));
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
        // Repeating the real pointer route is still selection-only NoChange.
        click_rule(cx, handle, f.rule);
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(counts.get(), (0, 1));
    }
}

#[gpui::test]
fn native_atomic_pointer_rejects_stale_or_error_targets_without_fallback_or_focus_change(
    cx: &mut TestAppContext,
) {
    for reject in [false, true] {
        let f = fixture();
        let stale = DocumentSelection::node(&f.document, f.quote).unwrap();
        let (handle, session, counts) = open(cx, &f, None);
        session
            .borrow_mut()
            .apply(
                &Transaction::new(TransactionOrigin::System)
                    .with_step(TransactionStep::RemoveNode { node: f.quote }),
            )
            .unwrap();
        session
            .borrow_mut()
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        let document = session.borrow().document().clone();
        let selection = session.borrow().selection();
        let marks = session.borrow().stored_marks().cloned();
        let history = session.borrow().history_depths();
        counts.set((0, 0));
        let calls = Rc::new(RefCell::new(Vec::new()));
        handle
            .update(cx, |view, _, cx| {
                view.set_command_router(Some(Rc::new(AtomicRouter {
                    response: if reject {
                        AtomicResponse::Error
                    } else {
                        AtomicResponse::Exact(stale)
                    },
                    calls: calls.clone(),
                })));
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        click_rule(cx, handle, f.rule);
        assert_eq!(calls.borrow().as_slice(), &[(selection, f.rule)]);
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().document().revision(), document.revision());
        assert_eq!(session.borrow().stored_marks(), marks.as_ref());
        assert_eq!(session.borrow().history_depths(), history);
        assert_eq!(counts.get(), (0, 0));
        handle
            .update(cx, |view, window, cx| {
                assert!(view.range_input.is_none());
                assert!(
                    view.children[0]
                        .1
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
    }
}
