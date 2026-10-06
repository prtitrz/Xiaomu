//! Mounted production element/input coverage with GPUI TestPlatform's
//! NoopTextSystem. These deterministic tests are not native font/IME evidence.

#[path = "text_size_edge_tests.rs"]
mod edge_tests;

use super::*;
use crate::block_alignment::BlockAlignment;
use crate::font_size::FontSizeContext;
use crate::input::utf16;
use crate::text_size::{
    TextSizeCapability, TextSizeCaretContext, TextSizeStyle, TextSizeStyleProvider,
};
use gpui::{Entity, EntityInputHandler, TestAppContext, WindowHandle, font, point, px};
use xiaomu_core::document::{
    Mark, MarkSet, Node, NodeAttrs, NodeContent, NodeStoreBuilder, StringAttribute, TextRun,
    TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::{DocumentSelection, SessionOutcome};

struct FixedStyle {
    sized_caret: bool,
}

impl TextSizeStyleProvider for FixedStyle {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(20.0, 20.0, 20.0).unwrap(),
            1.4,
        )
    }

    fn caret_height(&self, context: TextSizeCaretContext) -> Option<f32> {
        self.sized_caret.then_some(context.effective_size())
    }
}

struct Host {
    input: Entity<ParagraphView>,
    width: Pixels,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(
            div()
                .ml(px(17.0))
                .mt(px(23.0))
                .w(self.width)
                .font_family(".SystemUIFont")
                .text_size(px(20.0))
                .line_height(gpui::relative(1.4))
                .child(self.input.clone()),
        )
    }
}

fn size_mark(value: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value(value.into())),
    ))
}

fn open(
    cx: &mut TestAppContext,
    parts: &[(&str, &str)],
    alignment: BlockAlignment,
    width: f32,
    sized_caret: bool,
) -> (WindowHandle<Host>, SharedSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if parts.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new(parts.iter().map(|(text, size)| {
            TextRun::new(*text, MarkSet::new([size_mark(size)]).unwrap()).unwrap()
        }))
        .unwrap()
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    mount(
        cx,
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        node,
        alignment,
        width,
        sized_caret,
    )
}

fn mount(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    node: NodeId,
    alignment: BlockAlignment,
    width: f32,
    sized_caret: bool,
) -> (WindowHandle<Host>, SharedSession, NodeId) {
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(node));
    let session = Rc::new(RefCell::new(
        DocumentSession::new(document, selection).unwrap(),
    ));
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let capability = Rc::new(TextSizeCapability::new(
                window.text_system().clone(),
                Rc::new(FixedStyle { sized_caret }),
            ));
            capability
                .validate_document(
                    session.borrow().document(),
                    &InlineAtomRendererRegistry::default(),
                )
                .unwrap();
            let input = cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.set_block_alignment(Some(alignment));
                view.attach_text_size_capability(Some(capability));
                window.focus(&view.focus_handle);
                view
            });
            cx.new(|_| Host {
                input,
                width: px(width),
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

fn near(actual: Pixels, expected: Pixels) {
    let delta = f32::from(actual - expected);
    assert!(delta.abs() < 0.02, "{actual:?} != {expected:?}");
}

fn set_caret(session: &SharedSession, node: NodeId, byte: usize, affinity: CursorAffinity) {
    let offset = session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(byte)
        .unwrap();
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(InlinePoint::new(
            node, offset, 0, affinity,
        )))
        .unwrap();
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
    view.bounds_for_range(head..head, view.last_bounds.unwrap(), window, cx)
        .unwrap()
}

#[gpui::test]
fn production_uniform_requested_sizes_keep_the_twenty_pixel_baseline_minimum(
    cx: &mut TestAppContext,
) {
    for (css, size) in [("12px", 12.0_f32), ("24px", 24.0), ("48px", 48.0)] {
        let (handle, session, _) = open(cx, &[("same", css)], BlockAlignment::Left, 400.0, false);
        let before = session.borrow().document().clone();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let layout = view.last_layout.as_ref().unwrap();
                    assert_eq!(layout.lines().len(), 1);
                    assert_eq!(layout.lines()[0].unwrapped_layout.font_size, px(size));
                    near(layout.size().height, px(size.max(20.0) * 1.4));
                    near(
                        native_caret(view, window, cx).size.height,
                        px(size.max(20.0) * 1.4),
                    );
                })
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn production_mixed_rows_share_size_accurate_alignment_hit_selection_and_utf16_bounds(
    cx: &mut TestAppContext,
) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let parts = [
            ("small words small words ", "12px"),
            ("中🙂 medium ", "24px"),
            ("BIG WIDE", "48px"),
        ];
        let (handle, session, node) = open(cx, &parts, alignment, 110.0, false);
        let before = session.borrow().document().clone();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let (text, segments) = view.layout_content();
                    let content = view.sized_content(&segments).unwrap().unwrap();
                    let crate::mixed_size::Layout::Mixed(control) = crate::mixed_size::layout(
                        content.capability.text_system(),
                        content.input(&text, host.width),
                    )
                    .unwrap() else {
                        panic!("mixed production fixture");
                    };
                    assert!(control.rows.len() >= 3);
                    assert!(
                        control
                            .rows
                            .iter()
                            .any(|row| row.height > control.rows[0].height)
                    );
                    let actual = view.last_layout.as_ref().unwrap().clone();
                    assert!(
                        actual.lines().is_empty(),
                        "mixed dispatch never impersonates stock lines"
                    );
                    near(actual.size().height, control.size.height);
                    let element = view.last_bounds.unwrap();
                    let mut sizes = Vec::new();
                    for row in &control.rows {
                        assert!(row.height >= px(28.0));
                        sizes.extend(row.fragments.iter().map(|fragment| fragment.line.font_size));
                        let dx = alignment.offset(element.size.width, row.width);
                        let selected = actual.selection_rects(row.range.clone());
                        assert_eq!(selected.len(), 1);
                        near(selected[0].left(), dx);
                        near(selected[0].top(), row.y);
                        near(selected[0].size.height, row.height);
                        let native = view
                            .bounds_for_range(
                                utf16::utf16_offset(&text, row.range.start)
                                    ..utf16::utf16_offset(&text, row.range.end),
                                element,
                                window,
                                cx,
                            )
                            .unwrap();
                        near(native.top(), element.top() + row.y);
                        near(native.size.height, row.height);
                        for stop in &row.stops {
                            let affinity = if stop.index == row.range.start {
                                CursorAffinity::After
                            } else {
                                CursorAffinity::Before
                            };
                            let expected = point(dx + stop.x, row.y);
                            assert_eq!(
                                actual.position_for_caret(stop.index, affinity),
                                Some(expected)
                            );
                            let pointer =
                                element.origin + expected + point(px(0.0), row.height / 2.0);
                            assert_eq!(
                                view.hit_test_caret_position(pointer),
                                Some((stop.index, affinity))
                            );
                            assert_eq!(
                                view.character_index_for_point(pointer, window, cx),
                                Some(utf16::utf16_offset(&text, stop.index))
                            );
                            set_caret(&session, node, stop.index, affinity);
                            let caret = native_caret(view, window, cx);
                            assert_eq!(caret.origin, element.origin + expected);
                            near(caret.size.height, row.height);
                        }
                    }
                    for requested in [12.0, 24.0, 48.0] {
                        assert!(sizes.contains(&px(requested)));
                    }
                    let all = view
                        .bounds_for_range(0..text.encode_utf16().count(), element, window, cx)
                        .unwrap();
                    near(all.size.height, control.size.height);
                })
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn production_visual_navigation_uses_adjacent_unequal_rows_and_wrap_affinity(
    cx: &mut TestAppContext,
) {
    let (handle, _, _) = open(
        cx,
        &[("one two three four ", "12px"), ("BIG TEXT END", "48px")],
        BlockAlignment::Right,
        90.0,
        false,
    );
    handle
        .update(cx, |host, _, cx| {
            host.input.update(cx, |view, _| {
                let layout = view.last_layout.as_ref().unwrap();
                let mut caret = (0, CursorAffinity::Before);
                let desired_x = px(1000.0);
                let mut visited = 1;
                while let Some(next) =
                    view.visual_vertical_target(caret.0, caret.1, desired_x, true)
                {
                    let current_rect = layout.caret_rect(caret.0, caret.1, px(1.0)).unwrap();
                    let next_rect = layout.caret_rect(next.0, next.1, px(1.0)).unwrap();
                    near(next_rect.top(), current_rect.bottom());
                    let start = view.visual_line_edge_target(next.0, next.1, false).unwrap();
                    assert_eq!(start.1, CursorAffinity::After);
                    assert!(view.visual_is_soft_wrap_boundary(start.0));
                    let back = view
                        .visual_vertical_target(next.0, next.1, desired_x, false)
                        .unwrap();
                    near(
                        layout.caret_rect(back.0, back.1, px(1.0)).unwrap().top(),
                        current_rect.top(),
                    );
                    caret = next;
                    visited += 1;
                }
                assert!(visited >= 3);
                assert_eq!(view.visual_edge_row_target(desired_x, true), Some(caret));
                assert!(
                    view.visual_vertical_target(0, CursorAffinity::Before, desired_x, false)
                        .is_none()
                );
            })
        })
        .unwrap();
}

#[gpui::test]
fn size_only_canonical_changes_invalidate_production_cache_without_an_epoch(
    cx: &mut TestAppContext,
) {
    let (handle, session, node) = open(
        cx,
        &[("same", "12px")],
        BlockAlignment::Center,
        200.0,
        false,
    );
    let mut previous = None;
    let mut previous_width = None;
    for (css, size) in [("12px", 12.0), ("24px", 24.0), ("48px", 48.0)] {
        handle
            .update(cx, |host, _, cx| {
                host.input.update(cx, |view, cx| {
                    let inline = view.inline().unwrap();
                    session
                        .borrow_mut()
                        .set_inline_selection(
                            InlinePoint::at_start_of(node),
                            InlinePoint::new(
                                node,
                                inline.offset_at(4).unwrap(),
                                0,
                                CursorAffinity::Before,
                            ),
                        )
                        .unwrap();
                    session
                        .borrow_mut()
                        .apply_intent(&EditIntent::SetMark {
                            mark: size_mark(css),
                        })
                        .unwrap();
                    cx.notify();
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |host, _, cx| {
                host.input.update(cx, |view, _| {
                    assert_eq!(view.epoch.get(), 0);
                    assert_eq!(view.canonical_text(), "same");
                    let key = view.cache_key.unwrap();
                    let layout = view.last_layout.as_ref().unwrap();
                    assert_eq!(layout.lines()[0].unwrapped_layout.font_size, px(size));
                    if let Some(previous) = previous {
                        assert_ne!(key, previous);
                    }
                    if let Some(width) = previous_width {
                        assert!(layout.size().width > width);
                    }
                    previous = Some(key);
                    previous_width = Some(layout.size().width);
                })
            })
            .unwrap();
    }
}

#[gpui::test]
fn pending_empty_sizes_drive_centered_caret_preedit_cancel_commit_and_undo(
    cx: &mut TestAppContext,
) {
    for (css, size) in [("12px", 12.0_f32), ("24px", 24.0), ("48px", 48.0)] {
        let (handle, session, _) = open(cx, &[], BlockAlignment::Right, 220.0, true);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        handle
            .update(cx, |host, _, cx| {
                host.input.update(cx, |_, cx| {
                    assert!(matches!(
                        session
                            .borrow_mut()
                            .apply_intent(&EditIntent::SetMark {
                                mark: size_mark(css)
                            })
                            .unwrap(),
                        SessionOutcome::NoChange
                    ));
                    cx.notify();
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    let bounds = view.last_bounds.unwrap();
                    // The painted caret centers in the exact text row, not the
                    // element box rounded by GPUI layout (33.6px versus 33.5px).
                    let row_height = view.last_layout.as_ref().unwrap().size().height;
                    near(row_height, px(size.max(20.0) * 1.4));
                    let caret = native_caret(view, window, cx);
                    near(caret.size.height, px(size));
                    near(caret.left(), bounds.right());
                    near(caret.top(), bounds.top() + (row_height - px(size)) / 2.0);
                    view.replace_and_mark_text_in_range(None, "中🙂", Some(3..3), window, cx);
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    assert!(view.is_composing());
                    assert!(view.cache_key.is_none());
                    assert_eq!(
                        view.selected_text_range(true, window, cx).unwrap().range,
                        3..3
                    );
                    assert_eq!(
                        view.last_layout.as_ref().unwrap().lines()[0]
                            .unwrapped_layout
                            .font_size,
                        px(size)
                    );
                    near(native_caret(view, window, cx).size.height, px(size));
                    assert_eq!(session.borrow().document().store(), before.store());
                    assert_eq!(session.borrow().selection(), selection);
                    view.replace_and_mark_text_in_range(None, "", None, window, cx);
                    assert!(!view.is_composing());
                    assert_eq!(session.borrow().history_depths(), (0, 0));
                    assert!(
                        session
                            .borrow()
                            .stored_marks()
                            .unwrap()
                            .as_slice()
                            .contains(&size_mark(css))
                    );
                    view.replace_and_mark_text_in_range(None, "中🙂", None, window, cx);
                    view.replace_text_in_range(None, "中🙂", window, cx);
                    assert!(!view.is_composing());
                    assert_eq!(view.canonical_text(), "中🙂");
                    assert!(
                        view.inline().unwrap().runs()[0]
                            .marks()
                            .as_slice()
                            .contains(&size_mark(css))
                    );
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        assert_eq!(session.borrow().history_depths(), (1, 0));
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
    }
}
