//! Aligned coordinates reuse full measured-cell geometry and existing clips.
use super::*;
use crate::block_alignment::{BlockAlignment, BlockAlignmentProvider};
use gpui::Bounds;
use xiaomu_core::selection::CursorAffinity;

struct Align(BlockAlignment);
impl BlockAlignmentProvider for Align {
    fn alignment(&self, _: &xiaomu_core::document::Node) -> BlockAlignment {
        self.0
    }
}

fn install(handle: WindowHandle<DocumentView>, alignment: BlockAlignment, cx: &mut TestAppContext) {
    handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(Some(Rc::new(Align(alignment))));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}

fn native(
    handle: WindowHandle<DocumentView>,
    f: &Nested,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    handle
        .update(cx, |view, window, cx| {
            let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
            view.focused_child(window, cx)
                .unwrap()
                .update(cx, |child, cx| {
                    let x = child.visual_caret_x(0, CursorAffinity::Before).unwrap();
                    let native = child.bounds_for_range(0..0, bounds, window, cx).unwrap();
                    assert_eq!(native.left(), bounds.left() + x);
                    assert_eq!(
                        child.character_index_for_point(
                            native.origin + point(px(0.), px(1.)),
                            window,
                            cx
                        ),
                        Some(0)
                    );
                    native
                })
        })
        .unwrap()
}

#[gpui::test]
fn alignment_nested_horizontal_scroll_moves_native_hit_and_caret_in_one_coordinate_space(
    cx: &mut TestAppContext,
) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let f = nested();
        let session = session(&f.fixture, f.inner_blocks[1]);
        let before_selection = session.borrow().selection();
        let handle = open(session.clone(), cx);
        install(handle, alignment, cx);
        let before = native(handle, &f, cx);
        let hidden = handle
            .update(cx, |view, _, cx| {
                let bounds = view.block_bounds(f.inner_blocks[1]).unwrap();
                let visible = view
                    .visible_table_bounds(session.borrow().document(), f.inner_blocks[1], bounds)
                    .unwrap();
                assert!(visible.right() < bounds.right());
                let hidden = point(bounds.right() - px(1.), bounds.top() + px(1.));
                assert!(!visible.contains(&hidden));
                let child = view
                    .children
                    .iter()
                    .find(|(node, _)| *node == f.inner_blocks[1])
                    .unwrap()
                    .1
                    .read(cx);
                let start = child.visual_caret_x(0, CursorAffinity::Before).unwrap();
                let end = child
                    .visual_caret_x("INNER-B".len(), CursorAffinity::Before)
                    .unwrap();
                match alignment {
                    BlockAlignment::Left => assert_eq!(start, px(0.)),
                    BlockAlignment::Center => {
                        assert!((f32::from(start + end - bounds.size.width)).abs() < 0.001)
                    }
                    BlockAlignment::Right => {
                        assert!((f32::from(end - bounds.size.width)).abs() < 0.001)
                    }
                }
                hidden
            })
            .unwrap();
        session
            .borrow_mut()
            .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                f.fixture.before,
            )))
            .unwrap();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
            hidden,
            MouseButton::Left,
            Default::default(),
        );
        assert!(
            session
                .borrow()
                .selection()
                .as_same_node_inline()
                .is_none_or(|(_, focus)| focus.node_id() != f.inner_blocks[1])
        );
        session
            .borrow_mut()
            .set_document_selection(before_selection)
            .unwrap();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        scroll_right(handle, &f, cx);
        let after = native(handle, &f, cx);
        assert_eq!(after.origin, before.origin - point(px(54.), px(0.)));
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
            after.origin + point(px(0.), px(1.)),
            MouseButton::Left,
            Default::default(),
        );
        assert_eq!(session.borrow().selection(), before_selection);
        assert_eq!(
            session.borrow().document().store(),
            f.fixture.document.store()
        );
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn alignment_scrolled_resize_preview_repositions_and_cancel_restores_native_bounds(
    cx: &mut TestAppContext,
) {
    for (alignment, factor) in [(BlockAlignment::Center, 0.5), (BlockAlignment::Right, 1.)] {
        let f = nested();
        let session = session(&f.fixture, f.inner_blocks[1]);
        let handle = open(session.clone(), cx);
        install(handle, alignment, cx);
        let commits = install_resize(handle, cx);
        scroll_right(handle, &f, cx);
        let before = native(handle, &f, cx);
        let pointer = drag_start(handle, &f, cx);
        VisualTestContext::from_window(handle.into(), cx).simulate_mouse_move(
            pointer + point(px(20.), px(0.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        let preview = native(handle, &f, cx);
        assert!((f32::from(preview.left() - before.left()) - 20. * factor).abs() < 0.001);
        handle
            .update(cx, |view, _, cx| {
                view.column_resize.cancel();
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(native(handle, &f, cx), before);
        assert!(commits.borrow().is_empty());
        assert_eq!(
            session.borrow().document().store(),
            f.fixture.document.store()
        );
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}
