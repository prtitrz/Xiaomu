//! Wrapped, mixed-size and aligned reading share production row geometry.
use super::*;
use crate::{
    block_alignment::{BlockAlignment, BlockAlignmentProvider},
    font_size::FontSizeContext,
    text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider},
};
use xiaomu_core::document::{Node, StringAttribute, TextStyleAttributes, TextStyleMark};
struct Fixed;
impl TextSizeStyleProvider for Fixed {
    fn caret_height(&self, context: crate::text_size::TextSizeCaretContext) -> Option<f32> {
        Some(context.effective_size())
    }
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            gpui::font(".SystemUIFont"),
            FontSizeContext::new(20.0, 20.0, 20.0).unwrap(),
            1.4,
        )
    }
}
struct Align;
impl BlockAlignmentProvider for Align {
    fn alignment(&self, _: &Node) -> BlockAlignment {
        BlockAlignment::Right
    }
}
fn size_mark(value: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value(value.into())),
    ))
}

#[gpui::test]
fn reading_uses_measured_mixed_size_wrapped_aligned_rows(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let text = "large words wrap across several actual rows ";
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("small prefix ", MarkSet::new([size_mark("12px")]).unwrap())
                        .unwrap(),
                    TextRun::new(text.repeat(6), MarkSet::new([size_mark("48px")]).unwrap())
                        .unwrap(),
                    TextRun::new("END", MarkSet::new([size_mark("16px")]).unwrap()).unwrap(),
                ])
                .unwrap(),
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
    let session = make_session(XiaomuDocument::new(root, builder.finish()).unwrap(), node);
    let handle = open(session.clone(), false, cx);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.attach_text_size_capability(Some(Rc::new(TextSizeCapability::new(
                    window.text_system().clone(),
                    Rc::new(Fixed),
                ))));
                view.set_block_alignment_provider(Some(Rc::new(Align)));
                cx.notify();
            })
        })
        .unwrap();
    repaint(handle, cx);
    let short_caret = at(&session, node, 2);
    session
        .borrow_mut()
        .set_inline_selection(short_caret, short_caret)
        .unwrap();
    handle
        .update(cx, |host, _window, cx| {
            host.editor.update(cx, |view, cx| {
                let row = view
                    .reading_target_bounds(
                        &view.reading_snapshot(),
                        ReadingRange::new(short_caret, short_caret),
                        cx,
                    )
                    .unwrap();
                let scroll = view.scroll_handle.offset();
                view.scroll_handle.set_offset(point(
                    scroll.x,
                    scroll.y + view.scroll_handle.bounds().top() + px(8.0) - row.bottom(),
                ));
                cx.notify();
            })
        })
        .unwrap();
    repaint(handle, cx);
    handle
        .update(cx, |host, window, cx| {
            let view = host.editor.read(cx);
            let stamp = view.reading_snapshot();
            let row = view
                .reading_target_bounds(&stamp, ReadingRange::new(short_caret, short_caret), cx)
                .unwrap();
            let caret = view.children[0]
                .1
                .read(cx)
                .reading_caret_bounds(&stamp, short_caret)
                .unwrap();
            assert!(
                view.reading_visible_bounds(node, row).is_some(),
                "the tall row still intersects"
            );
            assert!(
                view.reading_visible_bounds(node, caret).is_none(),
                "the actual short caret is offscreen"
            );
            assert_ne!(view.reading_start(window, cx), Some(short_caret));
        })
        .unwrap();
    let len = session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .len_bytes();
    let range = ReadingRange::new(at(&session, node, len - 3), at(&session, node, len));
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                let stamp = view.reading_snapshot();
                let target = view.reading_target_bounds(&stamp, range, cx).unwrap();
                assert!(target.top() > view.scroll_handle.bounds().bottom());
                assert!(target.left() > view.block_bounds(node).unwrap().left());
                view.set_reading_highlights(&stamp, &[range], Some(0), cx)
                    .unwrap();
                view.reveal_range(&stamp, range, window, cx).unwrap();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            assert!(host.external.is_focused(window));
            let view = host.editor.read(cx);
            assert_eq!(
                view.reading_reveal_status(),
                Some(ReadingRevealStatus::Revealed)
            );
            let target = view
                .reading_target_bounds(&view.reading_snapshot(), range, cx)
                .unwrap();
            assert!(view.reading_visible_bounds(node, target).is_some());
            // Canonical caret remains at the start above the viewport; actual top
            // row hit is far into the mixed-size block, not an estimated line count.
            let top = view.reading_start(window, cx).unwrap();
            assert_eq!(top.node_id(), node);
            assert!(top.text_offset().as_usize() > 0);
            let top_rect = view
                .reading_target_bounds(&view.reading_snapshot(), ReadingRange::new(top, top), cx)
                .unwrap();
            assert!(top_rect.top() <= view.scroll_handle.bounds().top() + px(1.0));
            assert!(top_rect.bottom() >= view.scroll_handle.bounds().top());
        })
        .unwrap();
}
