//! Native control callbacks remain safe across stale renders and scrolling.

use super::*;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

#[gpui::test]
fn removed_rendered_checkbox_cannot_edit_the_item_now_at_its_old_index(cx: &mut TestAppContext) {
    let (document, items, blocks) = fixture();
    let selection = other_item_range(&document, blocks[1]);
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    let point = checkbox_point(handle, items[0], cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_down(point, MouseButton::Left, Default::default());
    // Change the canonical snapshot without a repaint: MouseUp must still
    // resolve the old captured NodeId, never whichever item is now first.
    session
        .borrow_mut()
        .apply(
            &Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::RemoveNode { node: items[0] }),
        )
        .unwrap();
    let before_click = session.borrow().document().clone();
    let history = session.borrow().history_depths();
    visual.simulate_mouse_up(point, MouseButton::Left, Default::default());
    assert!(session.borrow().document().node(items[0]).is_none());
    assert_eq!(checked_value(&session, items[1]), Some(AttrValue::Null));
    assert_eq!(session.borrow().document().store(), before_click.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), history);
}

#[gpui::test]
fn checkbox_reads_live_checked_state_after_external_change_before_mouse_up(
    cx: &mut TestAppContext,
) {
    let (document, items, blocks) = fixture();
    let selection = other_item_range(&document, blocks[1]);
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    let point = checkbox_point(handle, items[0], cx);
    let epoch = handle.update(cx, |view, _, _| view.epoch.get()).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_down(point, MouseButton::Left, Default::default());
    session
        .borrow_mut()
        .apply_intent(&EditIntent::SetTaskChecked {
            item: items[0],
            checked: true,
        })
        .unwrap();
    visual.simulate_mouse_up(point, MouseButton::Left, Default::default());
    assert_eq!(
        checked_value(&session, items[0]),
        Some(AttrValue::Bool(false))
    );
    assert_eq!(session.borrow().history_depths(), (2, 0));
    assert_eq!(session.borrow().selection(), selection);
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(view.epoch.get(), epoch + 1);
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(blocks[1])
            );
        })
        .unwrap();
}

#[gpui::test]
fn successive_native_checkbox_clicks_toggle_live_value_twice(cx: &mut TestAppContext) {
    let (document, items, blocks) = fixture();
    let selection = other_item_range(&document, blocks[1]);
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    let point = checkbox_point(handle, items[0], cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    // Public GPUI event simulation flushes each update and can repaint.
    // This checks actual repeated pointer delivery; the separate stale-frame
    // test changes canonical checked between MouseDown and MouseUp.
    for expected in [true, false] {
        visual.simulate_click(point, Default::default());
        assert_eq!(
            checked_value(&session, items[0]),
            Some(AttrValue::Bool(expected))
        );
    }
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (2, 0));
}

#[gpui::test]
fn clicking_visible_checkbox_does_not_scroll_to_offscreen_existing_caret(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let mut blocks = Vec::new();
    let mut items = Vec::new();
    for _ in 0..80 {
        let block = b
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("task", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        blocks.push(block);
        items.push(
            b.insert(
                NodeKind::TaskItem,
                NodeAttrs::empty(),
                NodeContent::children([block]),
            )
            .unwrap(),
        );
    }
    let list = b
        .insert(
            NodeKind::TaskList,
            NodeAttrs::empty(),
            NodeContent::children(items.clone()),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, b.finish()).unwrap();
    let selection = DocumentSelection::collapsed(TextPoint::at_start_of(blocks[0]));
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    handle
        .update(cx, |view, _, cx| {
            let maximum = view.scroll_handle.max_offset().height;
            assert!(maximum > px(0.));
            view.scroll_handle.set_offset(gpui::point(px(0.), -maximum));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let offset = handle
        .update(cx, |view, _, _| view.scroll_handle.offset())
        .unwrap();
    assert!(offset.y < px(0.));
    let last_item = *items.last().unwrap();
    let point = checkbox_point(handle, last_item, cx);
    VisualTestContext::from_window(handle.into(), cx).simulate_click(point, Default::default());
    assert_eq!(
        checked_value(&session, last_item),
        Some(AttrValue::Bool(true))
    );
    assert_eq!(session.borrow().selection(), selection);
    handle
        .update(cx, |view, _, _| {
            assert_eq!(view.scroll_handle.offset(), offset)
        })
        .unwrap();
}
