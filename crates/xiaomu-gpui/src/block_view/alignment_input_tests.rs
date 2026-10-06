//! Mounted input/layout coverage uses TestPlatform, not native font/IME proof.
use super::*;
use crate::{block_alignment::BlockAlignment, input::utf16};
use gpui::{Entity, EntityInputHandler, TestAppContext, WindowHandle, point, px};
use xiaomu_core::{
    document::{Mark, MarkSet, NodeAttrs, NodeContent, NodeStoreBuilder, TextRun, XiaomuDocument},
    selection::{CursorAffinity, InlinePoint},
};
use xiaomu_runtime::session::DocumentSelection;

struct Host {
    input: Entity<ParagraphView>,
    width: gpui::Pixels,
    family: &'static str,
    font_size: gpui::Pixels,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(
            div()
                .ml(px(17.))
                .mt(px(23.))
                .w(self.width)
                .font_family(self.family)
                .text_size(self.font_size)
                .child(self.input.clone()),
        )
    }
}

fn open(
    cx: &mut TestAppContext,
    text: &str,
    alignment: Option<BlockAlignment>,
) -> (WindowHandle<Host>, SharedSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let content = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(
            text,
            MarkSet::new([Mark::Underline, Mark::Strike]).unwrap(),
        )
        .unwrap()])
        .unwrap()
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(content),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ));
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let input = cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.set_block_alignment(alignment);
                window.focus(&view.focus_handle);
                view
            });
            cx.new(|_| Host {
                input,
                width: px(130.),
                family: ".SystemUIFont",
                font_size: px(16.),
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, session, node)
}

fn native_caret(
    view: &mut ParagraphView,
    window: &mut Window,
    cx: &mut Context<ParagraphView>,
) -> Bounds<Pixels> {
    let selected = view.selected_text_range(true, window, cx).unwrap();
    let head = if selected.reversed {
        selected.range.start
    } else {
        selected.range.end
    };
    let bounds = view.last_bounds.unwrap();
    view.bounds_for_range(head..head, bounds, window, cx)
        .unwrap()
}

#[gpui::test]
fn opt_in_native_wrap_affinity_matches_caret_and_keeps_legacy_none(cx: &mut TestAppContext) {
    for alignment in [
        None,
        Some(BlockAlignment::Left),
        Some(BlockAlignment::Center),
        Some(BlockAlignment::Right),
    ] {
        let (handle, session, node) = open(
            cx,
            "one two three four five six seven eight nine",
            alignment,
        );
        let before = session.borrow().document().clone();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let layout = view.last_layout.as_ref().unwrap();
                    let boundary = layout
                        .visual_line_edge(0, CursorAffinity::Before, true)
                        .unwrap()
                        .0;
                    assert!(layout.is_soft_wrap_boundary(boundary));
                    let inline = view.inline().unwrap();
                    let at = InlinePoint::new(
                        node,
                        inline.offset_at(boundary).unwrap(),
                        0,
                        CursorAffinity::After,
                    );
                    session
                        .borrow_mut()
                        .set_document_selection(DocumentSelection::collapsed(at))
                        .unwrap();
                    let expected = if alignment.is_some() {
                        layout.position_for_caret(boundary, CursorAffinity::After)
                    } else {
                        layout.position_for_index(boundary)
                    }
                    .unwrap();
                    let bounds = view.last_bounds.unwrap();
                    let native = native_caret(view, window, cx);
                    assert_eq!(native.origin, bounds.origin + expected);
                    assert_eq!(
                        view.character_index_for_point(
                            native.origin + point(px(0.), px(1.)),
                            window,
                            cx
                        ),
                        Some(utf16::utf16_offset(&view.display_content().0, boundary))
                    );
                })
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn alignment_width_and_font_changes_refresh_cache_and_native_coordinates_without_edits(
    cx: &mut TestAppContext,
) {
    let (handle, session, _) = open(cx, "中🙂e\u{301} short", Some(BlockAlignment::Center));
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let mut previous = None;
    for (alignment, width, family, font_size) in [
        (BlockAlignment::Center, 130., ".SystemUIFont", 16.),
        (BlockAlignment::Right, 130., ".SystemUIFont", 16.),
        (BlockAlignment::Right, 170., ".SystemUIFont", 16.),
        (BlockAlignment::Right, 170., "monospace", 16.),
        (BlockAlignment::Right, 170., "monospace", 19.),
        (BlockAlignment::Left, 170., "monospace", 19.),
    ] {
        handle
            .update(cx, |host, _, cx| {
                host.width = px(width);
                host.family = family;
                host.font_size = px(font_size);
                host.input
                    .update(cx, |view, _| view.set_block_alignment(Some(alignment)));
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let key = view.cache_key.unwrap();
                    if let Some(previous) = previous {
                        assert_ne!(key, previous);
                    }
                    previous = Some(key);
                    assert_eq!(view.epoch.get(), 0);
                    let layout = view.last_layout.as_ref().unwrap();
                    let expected = layout
                        .position_for_caret(0, CursorAffinity::Before)
                        .unwrap();
                    let bounds = view.last_bounds.unwrap();
                    assert_eq!(
                        native_caret(view, window, cx).origin,
                        bounds.origin + expected
                    );
                })
            })
            .unwrap();
    }
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn aligned_preedit_cancel_commit_and_undo_keep_native_geometry_and_canonical_semantics(
    cx: &mut TestAppContext,
) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let (handle, session, _) = open(cx, "base 中🙂", Some(alignment));
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        for commit in [false, true] {
            handle
                .update(cx, |host, window, cx| {
                    host.input.update(cx, |view, cx| {
                        view.replace_and_mark_text_in_range(
                            None,
                            "中文🙂e\u{301}",
                            Some(6..6),
                            window,
                            cx,
                        )
                    })
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            handle
                .update(cx, |host, window, cx| {
                    host.input.update(cx, |view, cx| {
                        assert!(view.is_composing());
                        assert!(view.cache_key.is_none());
                        let byte = view.composing_caret_byte().unwrap();
                        let layout = view.last_layout.as_ref().unwrap();
                        let expected = layout
                            .position_for_caret(byte, CursorAffinity::Before)
                            .unwrap();
                        let bounds = view.last_bounds.unwrap();
                        assert_eq!(
                            native_caret(view, window, cx).origin,
                            bounds.origin + expected
                        );
                        assert_eq!(session.borrow().document().store(), before.store());
                        if commit {
                            view.replace_text_in_range(None, "中文🙂e\u{301}", window, cx);
                        } else {
                            view.replace_and_mark_text_in_range(None, "", None, window, cx);
                        }
                    })
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            if commit {
                assert_eq!(session.borrow().history_depths(), (1, 0));
                session.borrow_mut().undo().unwrap();
            }
            assert_eq!(session.borrow().document().store(), before.store());
            assert_eq!(session.borrow().selection(), selection);
        }
    }
}

#[gpui::test]
fn aligned_empty_paragraph_and_reversed_selection_native_head_are_consistent(
    cx: &mut TestAppContext,
) {
    for (text, alignment, fraction) in [
        ("", BlockAlignment::Center, 0.5),
        ("", BlockAlignment::Right, 1.),
        ("中🙂 tail", BlockAlignment::Center, 0.5),
    ] {
        let (handle, session, node) = open(cx, text, Some(alignment));
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    if !text.is_empty() {
                        let inline = view.inline().unwrap();
                        session
                            .borrow_mut()
                            .set_document_selection(DocumentSelection::new(
                                InlinePoint::new(
                                    node,
                                    inline.offset_at(text.len()).unwrap(),
                                    0,
                                    CursorAffinity::Before,
                                ),
                                InlinePoint::at_start_of(node),
                            ))
                            .unwrap();
                    }
                    let bounds = view.last_bounds.unwrap();
                    let native = native_caret(view, window, cx);
                    let expected = view
                        .last_layout
                        .as_ref()
                        .unwrap()
                        .position_for_index(0)
                        .unwrap();
                    assert_eq!(native.origin, bounds.origin + expected);
                    if text.is_empty() {
                        assert_eq!(native.left(), bounds.left() + bounds.size.width * fraction);
                    }
                })
            })
            .unwrap();
    }
}

#[gpui::test]
fn alignment_native_nonempty_ranges_cover_selected_rows_without_adjacent_endpoint_rows(
    cx: &mut TestAppContext,
) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let (handle, _, _) = open(cx, "a\nlong middle\nz", Some(alignment));
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let element = view.last_bounds.unwrap();
                    let layout = view.last_layout.as_ref().unwrap();
                    let first = layout.position_for_index(0).unwrap();
                    let last = layout.position_for_index(15).unwrap();
                    let middle_start = layout.position_for_index(2).unwrap();
                    let middle_end = layout.position_for_index(13).unwrap();
                    let selected = view.bounds_for_range(0..15, element, window, cx).unwrap();
                    assert!(selected.left() <= element.left() + middle_start.x);
                    assert!(selected.right() >= element.left() + middle_end.x);
                    assert!(selected.size.width > (last.x - first.x).abs());
                })
            })
            .unwrap();
        let (handle, _, _) = open(cx, "one two three four five six", Some(alignment));
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let element = view.last_bounds.unwrap();
                    let layout = view.last_layout.as_ref().unwrap();
                    let boundary = layout
                        .visual_line_edge(0, CursorAffinity::Before, true)
                        .unwrap()
                        .0;
                    let height = layout.line_height();
                    let selected = view
                        .bounds_for_range(boundary..boundary + 1, element, window, cx)
                        .unwrap();
                    assert_eq!(selected.top(), element.top() + height);
                    assert_eq!(selected.size.height, height);
                })
            })
            .unwrap();
        let (handle, _, _) = open(cx, "\n\n", Some(alignment));
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let element = view.last_bounds.unwrap();
                    let height = view.last_layout.as_ref().unwrap().line_height();
                    let selected = view.bounds_for_range(0..1, element, window, cx).unwrap();
                    assert_eq!(selected.top(), element.top());
                    assert_eq!(selected.size.height, height);
                    assert_eq!(selected.size.width, px(4.));
                })
            })
            .unwrap();
    }
}
