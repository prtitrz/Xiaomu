use super::*;

#[gpui::test]
fn measured_table_diagonal_wheel_has_one_dominant_axis(cx: &mut TestAppContext) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open(session.clone(), cx);
    let before = handle
        .update(cx, |view, _, _| {
            view.block_bounds(f.inner_blocks[1]).unwrap()
        })
        .unwrap();
    let over = before.origin + point(px(5.0), px(5.0));
    wheel(handle, over, -20.0, -5.0, cx);
    let after_x = handle
        .update(cx, |view, _, _| {
            assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(0.0)));
            view.block_bounds(f.inner_blocks[1]).unwrap()
        })
        .unwrap();
    assert_eq!(after_x.origin, before.origin - point(px(20.0), px(0.0)));
    wheel(
        handle,
        after_x.origin + point(px(5.0), px(5.0)),
        -5.0,
        -20.0,
        cx,
    );
    handle
        .update(cx, |view, _, _| {
            assert_eq!(
                view.block_bounds(f.inner_blocks[1]).unwrap().left(),
                after_x.left()
            );
            assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(-20.0)));
        })
        .unwrap();
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
    assert_eq!(
        session.borrow().document().revision(),
        f.fixture.document.revision()
    );
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn measured_table_viewport_state_is_removed_with_disabled_element(cx: &mut TestAppContext) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let handle = open(session.clone(), cx);
    let before = handle
        .update(cx, |view, _, _| {
            view.block_bounds(f.inner_blocks[1]).unwrap()
        })
        .unwrap();
    wheel(
        handle,
        before.origin + point(px(5.0), px(5.0)),
        -90.0,
        0.0,
        cx,
    );
    handle
        .update(cx, |view, _, cx| {
            view.set_measured_table_layout(false);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, cx| {
            assert!(view.table_clips.borrow().is_empty());
            view.set_measured_table_layout(true);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert_eq!(
                view.block_bounds(f.inner_blocks[1]).unwrap().left(),
                before.left()
            );
        })
        .unwrap();
    assert_eq!(
        session.borrow().document().store(),
        f.fixture.document.store()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
