//! Real measured subtrees and dispatched wheels on GPUI's virtual platform.
use super::*;
use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase};

#[path = "measured_table_scroll_policy_tests.rs"]
mod policy;
#[path = "measured_table_scroll_resize_tests.rs"]
mod resize;
#[path = "measured_table_scroll_routing_tests.rs"]
mod routing;

struct Nested {
    fixture: Fixture,
    inner: NodeId,
    inner_cells: Vec<NodeId>,
    inner_blocks: Vec<NodeId>,
}

fn nested() -> Nested {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "before");
    let mut inner_cells = Vec::new();
    let mut inner_blocks = Vec::new();
    for (text, width) in [("INNER-A", 150), ("INNER-B", 130)] {
        let block = paragraph(&mut builder, text);
        inner_blocks.push(block);
        inner_cells.push(cell(&mut builder, width, vec![block]));
    }
    let inner = table(&mut builder, inner_cells.clone());
    let other = paragraph(&mut builder, "OUTER-B");
    let cells = vec![
        cell(&mut builder, 250, vec![inner]),
        cell(&mut builder, 120, vec![other]),
    ];
    let outer = table(&mut builder, cells.clone());
    let mut children = vec![before, outer];
    for _ in 0..80 {
        children.push(paragraph(&mut builder, "after"));
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    Nested {
        fixture: Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            table: outer,
            cells,
            blocks: vec![other],
            before,
        },
        inner,
        inner_cells,
        inner_blocks,
    }
}

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

fn wheel(
    handle: WindowHandle<DocumentView>,
    position: gpui::Point<gpui::Pixels>,
    x: f32,
    y: f32,
    cx: &mut TestAppContext,
) {
    VisualTestContext::from_window(handle.into(), cx).simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(x), px(y))),
        modifiers: Default::default(),
        touch_phase: TouchPhase::Moved,
    });
}

#[gpui::test]
fn measured_nested_table_horizontal_wheel_reveals_overflow_without_edits(cx: &mut TestAppContext) {
    let f = nested();
    let session = session(&f.fixture, f.inner_blocks[0]);
    let selection = session.borrow().selection();
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
        .update(cx, |view, _, _| {
            let after = view.block_bounds(f.inner_blocks[1]).unwrap();
            assert_eq!(
                after.left(),
                before.left() - px(54.0),
                "280px table inside 226px padded viewport must scroll"
            );
            assert_eq!(view.scroll_handle.offset(), point(px(0.0), px(0.0)));
            let registry = view.cell_registry.borrow();
            assert_eq!(
                registry
                    .iter()
                    .find(|(id, _)| *id == f.inner_cells[0])
                    .unwrap()
                    .1
                    .size
                    .width,
                px(150.0)
            );
            assert_eq!(
                registry
                    .iter()
                    .find(|(id, _)| *id == f.inner_cells[1])
                    .unwrap()
                    .1
                    .size
                    .width,
                px(130.0)
            );
            assert!(
                !view.column_resize.enabled(),
                "scrolling does not depend on resize opt-in"
            );
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
