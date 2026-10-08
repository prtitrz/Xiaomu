//! Nested horizontal reveal with the real measured table hierarchy.
use super::*;
use xiaomu_core::document::AttrValue;

fn cell(builder: &mut NodeStoreBuilder, width: i64, children: Vec<NodeId>) -> NodeId {
    builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::new(
                [(
                    "colwidth".into(),
                    AttrValue::List(vec![AttrValue::Integer(width)]),
                )]
                .into(),
            )
            .unwrap(),
            NodeContent::children(children),
        )
        .unwrap()
}
fn table(builder: &mut NodeStoreBuilder, cells: Vec<NodeId>) -> NodeId {
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap();
    builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap()
}
fn nested() -> (XiaomuDocument, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let mut children: Vec<_> = (0..12).map(|_| paragraph(&mut builder, "before")).collect();
    let first = children[0];
    let inner_a = paragraph(&mut builder, "inner A");
    let target = paragraph(&mut builder, "inner B TARGET");
    let inner_cells = vec![
        cell(&mut builder, 150, vec![inner_a]),
        cell(&mut builder, 150, vec![target]),
    ];
    let inner = table(&mut builder, inner_cells);
    let outer_a = paragraph(&mut builder, "outer A");
    let outer_cells = vec![
        cell(&mut builder, 300, vec![outer_a]),
        cell(&mut builder, 200, vec![inner]),
    ];
    let outer = table(&mut builder, outer_cells);
    children.push(outer);
    children.extend((0..12).map(|_| paragraph(&mut builder, "after")));
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        target,
        inner,
        outer,
    )
}

#[gpui::test]
fn passive_reading_reveals_inner_and_outer_horizontal_then_vertical(cx: &mut TestAppContext) {
    let (document, first, target, inner, outer) = nested();
    let session = make_session(document.clone(), first);
    let selection = session.borrow().selection();
    let handle = open(session.clone(), true, cx);
    let range = ReadingRange::new(at(&session, target, 8), at(&session, target, 14));
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                let stamp = view.reading_snapshot();
                assert!(
                    view.reading_visible_bounds(
                        target,
                        view.reading_target_bounds(&stamp, range, cx).unwrap()
                    )
                    .is_none()
                );
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
            assert!(view.scroll_handle.offset().y < px(0.0));
            for table in [inner, outer] {
                assert!(
                    view.reading.borrow().tables[&table].offset().x < px(0.0),
                    "table {table:?} must participate"
                );
            }
            let rect = view
                .reading_target_bounds(&view.reading_snapshot(), range, cx)
                .unwrap();
            let visible = view.reading_visible_bounds(target, rect).unwrap();
            assert!(visible.size.width >= rect.size.width.min(px(10.0)));
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
    // An inner caret that is inside its own viewport but hidden by the outer
    // viewport cannot be used as the visible starting point.
    session
        .borrow_mut()
        .set_inline_selection(range.start(), range.start())
        .unwrap();
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.reading.borrow().tables[&outer].set_offset(point(px(0.0), px(0.0)));
                assert_eq!(
                    view.reading_start(window, cx),
                    None,
                    "nested offset changed before repaint"
                );
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            let view = host.editor.read(cx);
            assert_ne!(view.reading_start(window, cx), Some(range.start()));
            let rect = view
                .reading_target_bounds(&view.reading_snapshot(), range, cx)
                .unwrap();
            assert!(view.reading_visible_bounds(target, rect).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn hidden_table_target_is_refused_without_claiming_reveal(cx: &mut TestAppContext) {
    let (document, first, target, _, outer) = nested();
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: outer,
            attrs: NodeAttrs::new([("unsupported".into(), AttrValue::Bool(true))].into()).unwrap(),
        })
        .apply(&document)
        .unwrap();
    let session = make_session(document, first);
    let handle = open(session.clone(), true, cx);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                assert_eq!(
                    view.reveal_position(
                        &view.reading_snapshot(),
                        at(&session, target, 0),
                        window,
                        cx
                    ),
                    Err(ReadingViewError::Unavailable)
                );
                assert_eq!(view.reading_reveal_status(), None);
            })
        })
        .unwrap();
}

#[gpui::test]
fn queued_reveal_observes_even_unchanged_inner_scroll_handles(cx: &mut TestAppContext) {
    let (document, first, target, inner, outer) = nested();
    let session = make_session(document, first);
    let handle = open(session.clone(), true, cx);
    // First inner column requires no inner scroll, but its ancestor/document
    // need movement. A user's new inner scroll must still cancel the plan.
    let inner_first = {
        let s = session.borrow();
        let row = s
            .document()
            .node(inner)
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0];
        let cell = s
            .document()
            .node(row)
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0];
        s.document()
            .node(cell)
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0]
    };
    assert_ne!(target, inner_first);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.reveal_position(
                    &view.reading_snapshot(),
                    at(&session, inner_first, 0),
                    window,
                    cx,
                )
                .unwrap();
                view.finish_reading_frame(window, cx);
                assert!(view.reading.borrow().scheduled);
                assert_eq!(view.reading.borrow().tables[&inner].offset().x, px(0.0));
                view.reading.borrow().tables[&inner].set_offset(point(px(-10.0), px(0.0)));
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, _window, cx| {
            let view = host.editor.read(cx);
            assert_eq!(
                view.reading_reveal_status(),
                Some(ReadingRevealStatus::Stale)
            );
            assert_eq!(view.scroll_handle.offset().y, px(0.0));
            assert_eq!(view.reading.borrow().tables[&outer].offset().x, px(0.0));
            assert_eq!(view.reading.borrow().tables[&inner].offset().x, px(-10.0));
        })
        .unwrap();
}
