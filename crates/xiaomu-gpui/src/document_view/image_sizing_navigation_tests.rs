//! Flow navigation after a tall decoded image remains on the document scroller.
use super::*;
use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase, point};

#[gpui::test]
fn tall_image_keeps_following_text_reveal_wheel_click_and_edit_reachable(cx: &mut TestAppContext) {
    let f = fixture(attrs(None, None));
    let selection = f.session.borrow().selection();
    let resolver = Resolver::new(Sample::Tall, false);
    let handle = open(&f, resolver, 432.0, cx);
    cx.background_executor.run_until_parked();
    assert_loaded(geometry(handle, &f, cx), Sample::Tall, None);
    let focus = handle
        .update(cx, |host, window, cx| {
            let focus = window.focused(cx);
            host.editor.update(cx, |view, cx| {
                let snapshot = view.reading_snapshot();
                view.reveal_position(&snapshot, InlinePoint::at_start_of(f.after), window, cx)
                    .unwrap();
            });
            focus
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let (_bounds, offset) = handle
        .update(cx, |host, window, cx| {
            let view = host.editor.read(cx);
            let bounds = view.block_bounds(f.after).unwrap();
            assert!(bounds.top() >= view.scroll_handle.bounds().top());
            assert!(bounds.bottom() <= view.scroll_handle.bounds().bottom());
            assert_eq!(
                window.focused(cx),
                focus,
                "passive reveal never steals focus"
            );
            (bounds, view.scroll_handle.offset())
        })
        .unwrap();
    assert!(offset.y < px(0.0));
    assert_untouched(&f, selection);
    // A vertical wheel above the image returns toward the document beginning.
    VisualTestContext::from_window(handle.into(), cx).simulate_event(ScrollWheelEvent {
        position: point(px(100.0), px(100.0)),
        delta: ScrollDelta::Pixels(point(px(0.0), px(2000.0))),
        modifiers: Default::default(),
        touch_phase: TouchPhase::Moved,
    });
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, _, cx| {
            assert!(host.editor.read(cx).scroll_handle.offset().y > offset.y);
        })
        .unwrap();
    // Reveal again using current geometry, then click the actual following text.
    handle
        .update(cx, |host, window, cx| {
            window.activate_window();
            host.editor.update(cx, |view, cx| {
                let snapshot = view.reading_snapshot();
                view.reveal_position(&snapshot, InlinePoint::at_start_of(f.after), window, cx)
                    .unwrap();
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let bounds = handle
        .update(cx, |host, _, cx| {
            host.editor.read(cx).block_bounds(f.after).unwrap()
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_click(
        point(bounds.left() + px(2.0), bounds.top() + px(10.0)),
        Default::default(),
    );
    VisualTestContext::from_window(handle.into(), cx).simulate_input("z");
    let session = f.session.borrow();
    let text: String = session
        .document()
        .node(f.after)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    assert!(text.contains('z'));
    assert_eq!(
        session.document().node(f.image).unwrap().attrs(),
        f.document.node(f.image).unwrap().attrs()
    );
    assert_eq!(session.history_depths(), (1, 0));
}
