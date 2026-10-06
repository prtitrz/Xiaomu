//! Interrupted gestures, host replacement and existing pointer surfaces.
use super::mounted::{active, down, edge, movement, open, up};
use super::*;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, MouseButton, ParentElement, Render, Styled,
    TestAppContext, VisualTestContext, Window, div,
};
use std::cell::Cell;

struct Host {
    view: Option<Entity<DocumentView>>,
    width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(400.0))
            .children(self.view.clone())
    }
}

#[gpui::test]
fn column_resize_changed_viewport_failed_layout_and_disposal_drop_preview(cx: &mut TestAppContext) {
    for mode in 0..3 {
        let f = fixture(false);
        let session = session(&f);
        let commits = Rc::new(Cell::new(0));
        let count = commits.clone();
        let handle = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                let view = cx.new(|_| {
                    let mut view = DocumentView::new(session.clone());
                    view.set_measured_table_layout(true);
                    view.set_table_column_resize(Some(TableColumnResize::new(
                        config(),
                        |_, _| true,
                        move |_, _, _, _| count.set(count.get() + 1),
                    )));
                    view
                });
                cx.new(|_| Host {
                    view: Some(view),
                    width: 400.0,
                })
            })
            .unwrap()
        });
        handle
            .update(cx, |host, window, cx| {
                window.activate_window();
                host.view
                    .as_ref()
                    .unwrap()
                    .update(cx, |view, cx| view.focus_selection(window, cx));
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let (p, weak) = handle
            .update(cx, |host, _, cx| {
                let view = host.view.as_ref().unwrap();
                let bounds = view.read(cx).cell_registry.borrow()[0].1;
                (
                    point(bounds.right(), bounds.top() + px(20.0)),
                    view.downgrade(),
                )
            })
            .unwrap();
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
            p,
            MouseButton::Left,
            Default::default(),
        );
        handle
            .update(cx, |host, _, cx| {
                assert!(
                    host.view
                        .as_ref()
                        .unwrap()
                        .read(cx)
                        .column_resize
                        .drag
                        .borrow()
                        .is_some()
                );
                match mode {
                    0 => host.width = 450.0,
                    1 => host.width = 1_000_100.0,
                    _ => host.view = None,
                };
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |host, _, cx| {
                if let Some(view) = &host.view {
                    assert!(view.read(cx).column_resize.drag.borrow().is_none());
                } else {
                    assert!(
                        weak.upgrade().is_none(),
                        "old frame handlers hold only weak entities"
                    );
                }
            })
            .unwrap();
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
            p,
            MouseButton::Left,
            Default::default(),
        );
        assert_eq!(commits.get(), 0);
        assert_eq!(session.borrow().document().store(), f.document.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn column_resize_readonly_start_and_existing_cell_handle_do_not_capture(cx: &mut TestAppContext) {
    let f = fixture(false);
    let session = session(&f);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let allowed = Rc::new(Cell::new(false));
    let handle = open(session.clone(), true, allowed.clone(), commits.clone(), cx);
    let p = edge(handle, cx);
    down(handle, p, cx);
    assert!(!active(handle, cx));
    up(handle, p, cx);
    allowed.set(true);
    let handle_position = handle
        .update(cx, |view, _, _| {
            view.cell_registry.borrow()[0].1.origin + point(px(2.0), px(2.0))
        })
        .unwrap();
    down(handle, handle_position, cx);
    assert!(!active(handle, cx));
    up(handle, handle_position, cx);
    assert_eq!(
        session
            .borrow()
            .selection()
            .active_cell_range()
            .unwrap()
            .anchor(),
        f.cells[0]
    );
    assert!(commits.borrow().is_empty());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn column_resize_duplicate_down_and_unrelated_button_preserve_one_gesture(cx: &mut TestAppContext) {
    let f = fixture(false);
    let commits = Rc::new(RefCell::new(Vec::new()));
    let handle = open(
        session(&f),
        true,
        Rc::new(Cell::new(true)),
        commits.clone(),
        cx,
    );
    let p = edge(handle, cx);
    down(handle, p, cx);
    movement(handle, p + point(px(20.0), px(0.0)), true, cx);
    down(handle, p + point(px(20.0), px(0.0)), cx);
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
        p,
        MouseButton::Right,
        Default::default(),
    );
    assert!(active(handle, cx));
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_move(
        p + point(px(30.0), px(0.0)),
        Some(MouseButton::Right),
        Default::default(),
    );
    assert!(active(handle, cx), "only button-free movement can finish");
    assert!(commits.borrow().is_empty());
    up(handle, p + point(px(40.0), px(0.0)), cx);
    assert_eq!(commits.borrow().len(), 1);
    assert_eq!(commits.borrow()[0].initial_width, 80);
    assert_eq!(commits.borrow()[0].width, 120);
}

#[gpui::test]
fn column_resize_release_only_position_is_measured_before_single_callback(cx: &mut TestAppContext) {
    let f = fixture(false);
    let session = session(&f);
    let widths = Rc::new(RefCell::new(Vec::new()));
    let handle = open(
        session.clone(),
        false,
        Rc::new(Cell::new(true)),
        Default::default(),
        cx,
    );
    let measured = widths.clone();
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(Some(TableColumnResize::new(
                config(),
                |_, _| true,
                move |intent, view, _, _| {
                    assert!(view.column_resize.drag.borrow().is_none());
                    measured.borrow_mut().push((
                        intent.width,
                        f32::from(view.cell_registry.borrow()[0].1.size.width),
                    ));
                },
            )));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let p = edge(handle, cx);
    down(handle, p, cx);
    up(handle, p + point(px(50.0), px(0.0)), cx);
    assert_eq!(&*widths.borrow(), &[(130, 130.0)]);
    assert_eq!(session.borrow().document().store(), f.document.store());
    down(handle, p, cx);
    up(handle, p + point(px(999_900.0), px(0.0)), cx);
    assert_eq!(
        widths.borrow().len(),
        1,
        "oversized table sum is never committed"
    );
}

#[gpui::test]
fn column_resize_guard_revoked_after_release_before_measurement_cancels(cx: &mut TestAppContext) {
    let f = fixture(false);
    let allowed = Rc::new(Cell::new(true));
    let commits = Rc::new(RefCell::new(Vec::new()));
    let handle = open(session(&f), true, allowed.clone(), commits.clone(), cx);
    let p = edge(handle, cx);
    down(handle, p, cx);
    handle
        .update(cx, |view, window, cx| {
            view.end_column_resize(p + point(px(30.0), px(0.0)), window, cx);
            assert!(view.column_resize.drag.borrow().as_ref().unwrap().released);
            allowed.set(false);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(!active(handle, cx));
    assert!(commits.borrow().is_empty());
}

#[gpui::test]
fn column_resize_hover_invalidation_tracks_guard_changes_without_pointer_motion(
    cx: &mut TestAppContext,
) {
    let f = fixture(false);
    let allowed = Rc::new(Cell::new(true));
    let handle = open(session(&f), true, allowed.clone(), Default::default(), cx);
    let p = edge(handle, cx);
    movement(handle, p, false, cx);
    handle
        .update(cx, |view, _, _| assert!(view.column_resize.hovered))
        .unwrap();
    allowed.set(false);
    handle.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| assert!(!view.column_resize.hovered))
        .unwrap();
    allowed.set(true);
    movement(handle, p, false, cx);
    handle
        .update(cx, |view, _, _| assert!(view.column_resize.hovered))
        .unwrap();
    handle
        .update(cx, |view, _, cx| {
            view.set_table_column_resize(None);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert!(view.column_resize.measurements.borrow().is_empty())
        })
        .unwrap();
}
