//! Mounted deterministic geometry/focus tests, not native font/IME acceptance.
use super::*;
use crate::block_view::SharedSession;
use gpui::{Entity, FocusHandle, TestAppContext, WindowHandle, div, point, px};
use xiaomu_core::{
    document::{InlineContent, Mark, MarkSet, NodeContent, NodeKind, NodeStoreBuilder, TextRun},
    selection::CursorAffinity,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

#[path = "reading_mixed_tests.rs"]
mod mixed;
struct CountChanges(Rc<Cell<usize>>);
impl xiaomu_runtime::session::DocumentChangeListener for CountChanges {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

#[path = "reading_table_tests.rs"]
mod tables;

struct Host {
    editor: Entity<DocumentView>,
    external: FocusHandle,
    width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(220.0))
            .flex()
            .flex_col()
            .child(div().h(px(30.0)).track_focus(&self.external).child("find"))
            .child(div().h(px(190.0)).min_h_0().child(self.editor.clone()))
    }
}
fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}
fn document(count: usize) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let blocks: Vec<_> = (0..count)
        .map(|_| paragraph(&mut builder, "abcdefgh 中🙂 wrap text words"))
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(blocks.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), blocks)
}
fn make_session(document: XiaomuDocument, node: NodeId) -> SharedSession {
    Rc::new(RefCell::new(
        DocumentSession::new(
            document,
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ))
}
fn open(session: SharedSession, measured: bool, cx: &mut TestAppContext) -> WindowHandle<Host> {
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let external = cx.focus_handle();
            let editor = cx.new(|_| {
                let mut view = DocumentView::new(session);
                view.set_measured_table_layout(measured);
                view
            });
            cx.new(|_| Host {
                editor,
                external,
                width: 260.0,
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |host, window, _| {
            window.activate_window();
            window.focus(&host.external);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
}
fn repaint(handle: WindowHandle<Host>, cx: &mut TestAppContext) {
    handle
        .update(cx, |host, _window, cx| {
            host.editor.update(cx, |_, cx| cx.notify());
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}
fn at(session: &SharedSession, node: NodeId, raw: usize) -> InlinePoint {
    let s = session.borrow();
    InlinePoint::new(
        node,
        s.document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap(),
        0,
        CursorAffinity::Before,
    )
}

#[gpui::test]
fn reading_decorations_and_reveal_preserve_session_focus_and_shape(cx: &mut TestAppContext) {
    let (document, blocks) = document(30);
    let session = make_session(document, blocks[0]);
    session
        .borrow_mut()
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    session
        .borrow_mut()
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let depths = session.borrow().history_depths();
    let marks = session
        .borrow()
        .effective_input_marks_at(at(&session, blocks[0], 1))
        .unwrap();
    let changes = Rc::new(Cell::new(0));
    session
        .borrow_mut()
        .add_listener(Box::new(CountChanges(changes.clone())));
    let handle = open(session.clone(), false, cx);
    let range = ReadingRange::new(at(&session, blocks[20], 0), at(&session, blocks[20], 8));
    let key = handle
        .update(cx, |host, window, cx| {
            assert!(host.external.is_focused(window));
            host.editor.update(cx, |view, window_cx| {
                let key = view.children[20].1.read(window_cx).cache_key;
                let epoch = view.epoch.get();
                let stamp = view.reading_snapshot();
                view.set_reading_highlights(&stamp, &[range], Some(0), window_cx)
                    .unwrap();
                assert_eq!(view.epoch.get(), epoch);
                view.reveal_range(&stamp, range, window, window_cx).unwrap();
                key
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            assert!(host.external.is_focused(window));
            host.editor.update(cx, |view, cx| {
                assert_eq!(
                    view.reading_reveal_status(),
                    Some(ReadingRevealStatus::Revealed)
                );
                assert!(view.scroll_handle.offset().y < px(0.0));
                let rect = view
                    .reading_target_bounds(&view.reading_snapshot(), range, cx)
                    .unwrap();
                assert!(
                    view.reading_visible_bounds(range.start().node_id(), rect)
                        .is_some()
                );
                assert_eq!(view.children[20].1.read(cx).cache_key, key);
                let offset = view.scroll_handle.offset();
                view.clear_reading_highlights(cx);
                view.focus_selection_without_scroll(window, cx);
                assert_eq!(view.scroll_handle.offset(), offset);
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().document().revision(), before.revision());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), depths);
    assert_eq!(
        session
            .borrow()
            .effective_input_marks_at(at(&session, blocks[0], 1))
            .unwrap(),
        marks
    );
    assert_eq!(
        changes.get(),
        0,
        "reading does not emit document/selection notifications"
    );
    // Reading did not close the existing timeless typing group.
    session
        .borrow_mut()
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.borrow().history_depths(), depths);
}

#[gpui::test]
fn reading_rejects_other_view_revisions_and_in_place_session_replacement(cx: &mut TestAppContext) {
    let (document, blocks) = document(2);
    let session = make_session(document.clone(), blocks[0]);
    let handle = open(session.clone(), false, cx);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                let stamp = view.reading_snapshot();
                let alien = DocumentView::new(session.clone()).reading_snapshot();
                assert_eq!(
                    view.set_reading_highlights(&alien, &[], None, cx),
                    Err(ReadingViewError::StaleSnapshot)
                );
                let range =
                    ReadingRange::new(at(&session, blocks[1], 0), at(&session, blocks[1], 4));
                view.set_reading_highlights(&stamp, &[range], Some(0), cx)
                    .unwrap();
                assert_eq!(
                    view.set_reading_highlights(&stamp, &[range], Some(1), cx),
                    Err(ReadingViewError::InvalidActiveIndex)
                );
                assert_eq!(
                    view.set_reading_highlights(
                        &stamp,
                        &[ReadingRange::new(range.end(), range.start())],
                        None,
                        cx
                    ),
                    Err(ReadingViewError::InvalidRange)
                );
                assert_eq!(view.reading.borrow().highlights.len(), 1);
                view.reveal_range(&stamp, range, window, cx).unwrap();
                *session.borrow_mut() = DocumentSession::new(
                    document,
                    DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
                )
                .unwrap();
                assert_eq!(
                    view.set_reading_highlights(&stamp, &[], None, cx),
                    Err(ReadingViewError::StaleSnapshot)
                );
                assert_eq!(
                    view.reading_reveal_status(),
                    Some(ReadingRevealStatus::Stale)
                );
                let fresh = view.reading_snapshot();
                session
                    .borrow_mut()
                    .apply_intent(&EditIntent::InsertText { text: "z".into() })
                    .unwrap();
                assert_eq!(
                    view.reveal_range(&fresh, range, window, cx),
                    Err(ReadingViewError::StaleSnapshot)
                );
            })
        })
        .unwrap();
    repaint(handle, cx);
    handle
        .update(cx, |host, _window, cx| {
            host.editor.update(cx, |view, _| {
                assert!(view.reading.borrow().highlights.is_empty());
                assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(0.0)));
            })
        })
        .unwrap();
}

#[gpui::test]
fn reading_start_uses_ordered_selection_then_visible_viewport_geometry(cx: &mut TestAppContext) {
    let (document, blocks) = document(30);
    let session = make_session(document, blocks[0]);
    let handle = open(session.clone(), false, cx);
    for (a, b) in [(2, 6), (6, 2)] {
        let start = at(&session, blocks[0], 2);
        let anchor = at(&session, blocks[0], a);
        let focus = at(&session, blocks[0], b);
        session
            .borrow_mut()
            .set_inline_selection(anchor, focus)
            .unwrap();
        handle
            .update(cx, |host, window, cx| {
                assert_eq!(host.editor.read(cx).reading_start(window, cx), Some(start));
            })
            .unwrap();
    }
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.scroll_handle.set_offset(point(px(0.0), px(-350.0)));
                assert_eq!(
                    view.reading_start(window, cx),
                    None,
                    "offset changed before repaint"
                );
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            let view = host.editor.read(cx);
            let start = view.reading_start(window, cx).unwrap();
            assert_ne!(start.node_id(), blocks[0]);
            let rect = view
                .reading_target_bounds(
                    &view.reading_snapshot(),
                    ReadingRange::new(start, start),
                    cx,
                )
                .unwrap();
            assert!(rect.top() <= view.scroll_handle.bounds().top() + px(28.0));
            assert!(rect.bottom() >= view.scroll_handle.bounds().top());
        })
        .unwrap();
}

#[gpui::test]
fn unmeasured_reveal_is_deferred_and_new_query_cancels_before_scroll(cx: &mut TestAppContext) {
    let (document, blocks) = document(30);
    let session = make_session(document, blocks[0]);
    let handle = open(session.clone(), false, cx);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.registry.borrow_mut().clear();
                let stamp = view.reading_snapshot();
                assert_eq!(
                    view.reveal_position(&stamp, at(&session, blocks[25], 0), window, cx),
                    Ok(ReadingRevealStatus::Deferred)
                );
                view.set_reading_highlights(&stamp, &[], None, cx).unwrap();
            })
        })
        .unwrap();
    repaint(handle, cx);
    handle
        .update(cx, |host, _window, cx| {
            let view = host.editor.read(cx);
            assert_eq!(
                view.reading_reveal_status(),
                Some(ReadingRevealStatus::Stale)
            );
            assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(0.0)));
        })
        .unwrap();
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.registry.borrow_mut().clear();
                view.reveal_position(
                    &view.reading_snapshot(),
                    at(&session, blocks[25], 0),
                    window,
                    cx,
                )
                .unwrap();
            })
        })
        .unwrap();
    repaint(handle, cx);
    handle
        .update(cx, |host, _window, cx| {
            assert_eq!(
                host.editor.read(cx).reading_reveal_status(),
                Some(ReadingRevealStatus::Revealed)
            )
        })
        .unwrap();
}

#[gpui::test]
fn newer_reveal_supersedes_already_scheduled_old_target(cx: &mut TestAppContext) {
    let (document, blocks) = document(30);
    let session = make_session(document, blocks[0]);
    let handle = open(session.clone(), false, cx);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                let stamp = view.reading_snapshot();
                view.reveal_position(&stamp, at(&session, blocks[25], 0), window, cx)
                    .unwrap();
                view.finish_reading_frame(window, cx);
                assert!(view.reading.borrow().scheduled);
                view.reveal_position(&stamp, at(&session, blocks[1], 0), window, cx)
                    .unwrap();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, _window, cx| {
            let view = host.editor.read(cx);
            assert_eq!(
                view.reading_reveal_status(),
                Some(ReadingRevealStatus::Revealed)
            );
            assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(0.0)));
        })
        .unwrap();
}
