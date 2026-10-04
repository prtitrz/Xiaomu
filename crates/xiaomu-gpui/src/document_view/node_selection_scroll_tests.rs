//! Whole-block scrolling uses actual subtree bounds, never a fake inline caret.

use super::*;
use crate::editor_commands::{CommandRoute, EditorCommand, EditorCommandRouter};

pub(super) fn tall_fixture() -> Fixture {
    let mut f = fixture();
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for _ in 0..70 {
        transaction = transaction.with_step(TransactionStep::InsertNode {
            parent: f.document.root(),
            index: 1,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Inline(
                InlineContent::new([TextRun::new("padding", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        });
    }
    f.document = transaction.apply(&f.document).unwrap();
    f
}

fn assert_visible(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, node: NodeId) {
    let selected = bounds(cx, handle, "node-selection", node);
    handle
        .update(cx, |view, _, _| {
            let viewport = view.scroll_handle.bounds();
            // The absolute wrapper probe occupies its inner border box.
            assert!(
                selected.top() >= viewport.top() - px(2.0),
                "node={node:?}, selected={selected:?}, viewport={viewport:?}, offset={:?}, max={:?}",
                view.scroll_handle.offset(), view.scroll_handle.max_offset(),
            );
            assert!(
                selected.bottom() <= viewport.bottom() + px(2.0),
                "node={node:?}, selected={selected:?}, viewport={viewport:?}, offset={:?}, max={:?}",
                view.scroll_handle.offset(), view.scroll_handle.max_offset(),
            );
        })
        .unwrap();
}

struct Target(DocumentSelection);
impl EditorCommandRouter for Target {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
    fn route_node_navigation(
        &self,
        _: EditorCommandContext<'_>,
        _: NodeNavigation,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        Ok(Some(self.0))
    }
}

#[gpui::test]
fn explicit_node_selection_and_navigation_reveal_offscreen_bounds_but_passive_scroll_stays_put(
    cx: &mut TestAppContext,
) {
    let f = tall_fixture();
    let (handle, session, _) = open(cx, &f, None);
    select(cx, handle, f.quote);
    assert_visible(cx, handle, f.quote);
    handle
        .update(cx, |view, _, cx| {
            assert!(view.scroll_handle.offset().y < px(0.0));
            view.scroll_handle.set_offset(gpui::point(px(0.0), px(0.0)));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert_eq!(view.scroll_handle.offset().y, px(0.0))
        })
        .unwrap();
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.select_node(f.quote, window, cx).unwrap(),
                Some(SessionOutcome::NoChange)
            );
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_visible(cx, handle, f.quote);
    for node in [f.intro, f.rule] {
        let target = DocumentSelection::node(&f.document, node).unwrap();
        handle
            .update(cx, |view, _, _| {
                view.set_command_router(Some(Rc::new(Target(target))))
            })
            .unwrap();
        key(cx, handle, "down");
        assert_eq!(session.borrow().selection(), target);
        assert_visible(cx, handle, node);
    }
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
