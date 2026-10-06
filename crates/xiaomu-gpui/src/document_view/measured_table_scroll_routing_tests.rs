//! Real virtual-platform wheel routing, per-view state and clipped pointer hits.
//! Geometry remains canonical-width; scrolling must never edit the session.

use super::*;
use gpui::{Bounds, MouseButton, Pixels, Point, size};
use xiaomu_runtime::session::DocumentPosition;

fn set_widths(fixture: &mut Fixture, widths: &[(NodeId, i64)]) {
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for (node, width) in widths {
        let mut attrs = fixture
            .document
            .node(*node)
            .unwrap()
            .attrs()
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        attrs.insert(
            "colwidth".into(),
            AttrValue::List(vec![AttrValue::Integer(*width)]),
        );
        transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
            node: *node,
            attrs: NodeAttrs::new(attrs).unwrap(),
        });
    }
    fixture.document = transaction.apply(&fixture.document).unwrap();
}

fn oversized_outer() -> Nested {
    let mut f = nested();
    let cell = f.fixture.cells[1];
    set_widths(&mut f.fixture, &[(cell, 800)]);
    f
}

fn open_narrow(session: SharedSession, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    let handle = open(session, cx);
    cx.simulate_window_resize(handle.into(), size(px(400.0), px(400.0)));
    cx.background_executor.run_until_parked();
    handle
}

fn bounds_of_cell(view: &DocumentView, cell: NodeId) -> Bounds<Pixels> {
    view.cell_registry
        .borrow()
        .iter()
        .find(|(id, _)| *id == cell)
        .unwrap()
        .1
}

fn clip_of_table(view: &DocumentView, table: NodeId) -> Bounds<Pixels> {
    view.table_clips.borrow().get(&table).copied().unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NestedPosition {
    outer_left: Pixels,
    inner_relative_left: Pixels,
    document_offset: Point<Pixels>,
}

fn nested_position(view: &DocumentView, f: &Nested) -> NestedPosition {
    let outer_left = bounds_of_cell(view, f.fixture.cells[0]).left();
    NestedPosition {
        outer_left,
        inner_relative_left: bounds_of_cell(view, f.inner_cells[0]).left() - outer_left,
        document_offset: view.scroll_handle.offset(),
    }
}

fn position(
    handle: WindowHandle<DocumentView>,
    f: &Nested,
    cx: &mut TestAppContext,
) -> NestedPosition {
    handle
        .update(cx, |view, _, _| nested_position(view, f))
        .unwrap()
}

fn over_inner(
    handle: WindowHandle<DocumentView>,
    f: &Nested,
    x: f32,
    y: f32,
    cx: &mut TestAppContext,
) {
    let point = handle
        .update(cx, |view, _, _| clip_of_table(view, f.inner).center())
        .unwrap();
    wheel(handle, point, x, y, cx);
}

fn assert_tracks(view: &DocumentView, widths: &[(NodeId, f32)]) {
    for (cell, width) in widths {
        assert_eq!(bounds_of_cell(view, *cell).size.width, px(*width));
    }
}

fn assert_unchanged(session: &SharedSession, fixture: &Fixture, selection: DocumentSelection) {
    let session = session.borrow();
    assert_eq!(session.document().store(), fixture.document.store());
    assert_eq!(session.document().revision(), fixture.document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[gpui::test]
fn measured_inner_horizontal_wheel_has_one_owner_even_with_oversized_outer(
    cx: &mut TestAppContext,
) {
    let f = oversized_outer();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    let before = position(handle, &f, cx);
    over_inner(handle, &f, -20.0, 0.0, cx);
    assert_eq!(
        position(handle, &f, cx),
        NestedPosition {
            inner_relative_left: before.inner_relative_left - px(20.0),
            ..before
        },
        "a changed inner offset consumes the wheel before outer and document handlers"
    );
    handle
        .update(cx, |view, _, _| {
            assert_tracks(
                view,
                &[
                    (f.inner_cells[0], 150.0),
                    (f.inner_cells[1], 130.0),
                    (f.fixture.cells[0], 250.0),
                    (f.fixture.cells[1], 800.0),
                ],
            );
            assert_eq!(clip_of_table(view, f.inner).size.width, px(226.0));
            assert!(clip_of_table(view, f.fixture.table).size.width < px(1050.0));
        })
        .unwrap();
    assert_unchanged(&session, &f.fixture, selection);
}

#[gpui::test]
fn measured_inner_edge_chains_next_horizontal_wheel_to_outer(cx: &mut TestAppContext) {
    let f = oversized_outer();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    let before = position(handle, &f, cx);
    over_inner(handle, &f, -90.0, 0.0, cx);
    let at_edge = position(handle, &f, cx);
    assert_eq!(
        at_edge.inner_relative_left,
        before.inner_relative_left - px(54.0)
    );
    assert_eq!(at_edge.outer_left, before.outer_left);
    assert_eq!(at_edge.document_offset, before.document_offset);

    // The event that reaches the edge still has only one owner. A later event
    // with no possible inner movement is available to the outer viewport.
    over_inner(handle, &f, -35.0, 0.0, cx);
    let outer_scrolled = position(handle, &f, cx);
    assert_eq!(
        outer_scrolled.inner_relative_left,
        at_edge.inner_relative_left
    );
    assert_eq!(outer_scrolled.outer_left, before.outer_left - px(35.0));
    assert_eq!(outer_scrolled.document_offset, before.document_offset);

    // Reversing direction makes the nearest viewport eligible again.
    over_inner(handle, &f, 18.0, 0.0, cx);
    let reversed = position(handle, &f, cx);
    assert_eq!(
        reversed.inner_relative_left,
        at_edge.inner_relative_left + px(18.0)
    );
    assert_eq!(reversed.outer_left, outer_scrolled.outer_left);
    assert_eq!(reversed.document_offset, before.document_offset);
    assert_unchanged(&session, &f.fixture, selection);
}

#[gpui::test]
fn measured_pure_vertical_wheel_over_inner_scrolls_document_only(cx: &mut TestAppContext) {
    let f = oversized_outer();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    over_inner(handle, &f, -20.0, 0.0, cx);
    let before = position(handle, &f, cx);
    over_inner(handle, &f, 0.0, -25.0, cx);
    let after = position(handle, &f, cx);
    assert!(after.document_offset.y < before.document_offset.y);
    assert_eq!(after.document_offset.x, before.document_offset.x);
    assert_eq!(after.inner_relative_left, before.inner_relative_left);
    assert_eq!(after.outer_left, before.outer_left);
    assert_unchanged(&session, &f.fixture, selection);
}

#[gpui::test]
fn measured_zero_range_inner_passes_horizontal_wheel_to_outer(cx: &mut TestAppContext) {
    let mut f = oversized_outer();
    let widths = [(f.inner_cells[0], 80), (f.inner_cells[1], 90)];
    set_widths(&mut f.fixture, &widths);
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    let before = position(handle, &f, cx);
    handle
        .update(cx, |view, _, _| {
            assert_eq!(clip_of_table(view, f.inner).size.width, px(170.0));
            assert_tracks(view, &[(f.inner_cells[0], 80.0), (f.inner_cells[1], 90.0)]);
        })
        .unwrap();
    over_inner(handle, &f, -30.0, 0.0, cx);
    let after = position(handle, &f, cx);
    assert_eq!(after.inner_relative_left, before.inner_relative_left);
    assert_eq!(after.outer_left, before.outer_left - px(30.0));
    assert_eq!(after.document_offset, before.document_offset);
    assert_unchanged(&session, &f.fixture, selection);
}

#[gpui::test]
fn measured_table_offset_survives_repeated_render_and_next_wheel(cx: &mut TestAppContext) {
    let f = oversized_outer();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    let before = position(handle, &f, cx);
    over_inner(handle, &f, -25.0, 0.0, cx);
    let retained = position(handle, &f, cx);
    assert_eq!(
        retained.inner_relative_left,
        before.inner_relative_left - px(25.0)
    );
    for _ in 0..3 {
        repaint(handle, cx);
        assert_eq!(position(handle, &f, cx), retained);
    }
    over_inner(handle, &f, -10.0, 0.0, cx);
    assert_eq!(
        position(handle, &f, cx),
        NestedPosition {
            inner_relative_left: before.inner_relative_left - px(35.0),
            ..before
        }
    );
    assert_unchanged(&session, &f.fixture, selection);
}

fn sibling_tables() -> (Fixture, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "before");
    let mut cells = Vec::new();
    let mut blocks = Vec::new();
    let mut tables = Vec::new();
    for names in [["FIRST-A", "FIRST-B"], ["SECOND-A", "SECOND-B"]] {
        let mut row = Vec::new();
        for (name, width) in names.into_iter().zip([500, 300]) {
            let block = paragraph(&mut builder, name);
            let cell = cell(&mut builder, width, vec![block]);
            blocks.push(block);
            cells.push(cell);
            row.push(cell);
        }
        tables.push(table(&mut builder, row));
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([before, tables[0], tables[1]]),
        )
        .unwrap();
    (
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            table: tables[0],
            cells,
            blocks,
            before,
        },
        tables[1],
    )
}

#[gpui::test]
fn measured_sibling_tables_keep_independent_horizontal_offsets(cx: &mut TestAppContext) {
    let (f, second_table) = sibling_tables();
    let session = session(&f, f.before);
    let selection = session.borrow().selection();
    let handle = open_narrow(session.clone(), cx);
    let before = handle
        .update(cx, |view, _, _| {
            [
                bounds_of_cell(view, f.cells[0]),
                bounds_of_cell(view, f.cells[2]),
            ]
        })
        .unwrap();
    let point = handle
        .update(cx, |view, _, _| clip_of_table(view, f.table).center())
        .unwrap();
    wheel(handle, point, -40.0, 0.0, cx);
    handle
        .update(cx, |view, _, _| {
            assert_eq!(
                bounds_of_cell(view, f.cells[0]).left(),
                before[0].left() - px(40.0)
            );
            assert_eq!(bounds_of_cell(view, f.cells[2]), before[1]);
        })
        .unwrap();
    let point = handle
        .update(cx, |view, _, _| clip_of_table(view, second_table).center())
        .unwrap();
    wheel(handle, point, -70.0, 0.0, cx);
    repaint(handle, cx);
    handle
        .update(cx, |view, _, _| {
            assert_eq!(
                bounds_of_cell(view, f.cells[0]).left(),
                before[0].left() - px(40.0)
            );
            assert_eq!(
                bounds_of_cell(view, f.cells[2]).left(),
                before[1].left() - px(70.0)
            );
            assert_tracks(
                view,
                &[
                    (f.cells[0], 500.0),
                    (f.cells[1], 300.0),
                    (f.cells[2], 500.0),
                    (f.cells[3], 300.0),
                ],
            );
            assert_eq!(view.scroll_handle.offset(), gpui::point(px(0.0), px(0.0)));
        })
        .unwrap();
    assert_unchanged(&session, &f, selection);
}

struct MeasuredPanes {
    a: Entity<DocumentView>,
    b: Entity<DocumentView>,
}

impl Render for MeasuredPanes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(320.0))
                    .flex_shrink_0()
                    .h_full()
                    .child(self.a.clone()),
            )
            .child(
                div()
                    .w(px(320.0))
                    .flex_shrink_0()
                    .h_full()
                    .child(self.b.clone()),
            )
    }
}

#[gpui::test]
fn measured_views_of_same_table_do_not_share_horizontal_offset(cx: &mut TestAppContext) {
    let f = oversized_outer();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let mut make_view = || {
                cx.new(|_| {
                    let mut view = DocumentView::new(session.clone());
                    view.set_measured_table_layout(true);
                    view
                })
            };
            let a = make_view();
            let b = make_view();
            cx.new(|_| MeasuredPanes { a, b })
        })
        .unwrap()
    });
    cx.simulate_window_resize(handle.into(), size(px(640.0), px(400.0)));
    handle
        .update(cx, |panes, window, cx| {
            window.activate_window();
            panes
                .a
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let before = handle
        .update(cx, |panes, _, cx| {
            [
                nested_position(panes.a.read(cx), &f),
                nested_position(panes.b.read(cx), &f),
            ]
        })
        .unwrap();
    for (index, delta) in [(0, -20.0), (1, -35.0)] {
        let point = handle
            .update(cx, |panes, _, cx| {
                let view = if index == 0 { &panes.a } else { &panes.b };
                clip_of_table(view.read(cx), f.inner).center()
            })
            .unwrap();
        VisualTestContext::from_window(handle.into(), cx).simulate_event(ScrollWheelEvent {
            position: point,
            delta: ScrollDelta::Pixels(gpui::point(px(delta), px(0.0))),
            modifiers: Default::default(),
            touch_phase: TouchPhase::Moved,
        });
        handle
            .update(cx, |panes, _, cx| {
                assert_eq!(
                    nested_position(panes.a.read(cx), &f),
                    NestedPosition {
                        inner_relative_left: before[0].inner_relative_left - px(20.0),
                        ..before[0]
                    }
                );
                assert_eq!(
                    nested_position(panes.b.read(cx), &f),
                    NestedPosition {
                        inner_relative_left: before[1].inner_relative_left
                            - px(if index == 0 { 0.0 } else { 35.0 }),
                        ..before[1]
                    }
                );
            })
            .unwrap();
    }
    assert_unchanged(&session, &f.fixture, selection);
}

#[gpui::test]
fn measured_ordinary_pointer_selection_ignores_clipped_inner_cell_overflow(
    cx: &mut TestAppContext,
) {
    let f = nested();
    let session = session(&f.fixture, f.fixture.before);
    let selection = session.borrow().selection();
    let handle = open(session.clone(), cx);
    let obscured = handle
        .update(cx, |view, _, _| {
            let inner = bounds_of_cell(view, f.inner_cells[1]);
            let outer = bounds_of_cell(view, f.fixture.cells[1]);
            let point = point(outer.left() + px(20.0), inner.center().y);
            assert!(
                inner.contains(&point),
                "the uncut inner track overlaps this point"
            );
            assert!(!clip_of_table(view, f.inner).contains(&point));
            assert_eq!(view.cell_at_position(point), Some(f.fixture.cells[1]));
            point
        })
        .unwrap();
    assert_unchanged(&session, &f.fixture, selection);
    {
        let cx = &mut VisualTestContext::from_window(handle.into(), cx);
        cx.simulate_mouse_down(obscured, MouseButton::Left, Default::default());
        cx.simulate_mouse_up(obscured, MouseButton::Left, Default::default());
    }
    let selected = session.borrow().selection();
    assert!(selected.is_collapsed());
    assert!(selected.active_cell_range().is_none());
    assert!(
        matches!(selected.focus(), DocumentPosition::Inline(point) if point.node_id() == f.fixture.blocks[0])
    );

    over_inner(handle, &f, -90.0, 0.0, cx);
    assert_unchanged(&session, &f.fixture, selected);
    let visible = handle
        .update(cx, |view, _, _| {
            let block = view.block_bounds(f.inner_blocks[1]).unwrap();
            let point = block.origin + point(px(5.0), px(5.0));
            assert!(clip_of_table(view, f.inner).contains(&point));
            assert_eq!(view.cell_at_position(point), Some(f.inner_cells[1]));
            point
        })
        .unwrap();
    {
        let cx = &mut VisualTestContext::from_window(handle.into(), cx);
        cx.simulate_mouse_down(visible, MouseButton::Left, Default::default());
        cx.simulate_mouse_up(visible, MouseButton::Left, Default::default());
    }
    let selected = session.borrow().selection();
    assert!(selected.is_collapsed());
    assert!(selected.active_cell_range().is_none());
    assert!(
        matches!(selected.focus(), DocumentPosition::Inline(point) if point.node_id() == f.inner_blocks[1])
    );
    assert_unchanged(&session, &f.fixture, selected);
}
