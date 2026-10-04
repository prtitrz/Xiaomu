//! Public-layout-pipeline proof, using real wrapped text and nested tables.
//! These synthetic scroll roots do not replace the guarded DocumentView.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::{
    AppContext as _, Context, InteractiveElement as _, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, div, point,
};
use xiaomu_core::document::{
    AttrValue, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, XiaomuDocument,
};

use super::*;
use crate::table_layout::{
    SpanningTableElement, TableCellElement, TableElementLayoutState, TableElementPrepaintState,
    TableGeometry, TableLayoutPlan,
};

const VIEWPORT_PADDING: f32 = 12.0;
const CELL_PADDING: f32 = 8.0;
const BEFORE_HEIGHT: f32 = 20.0;
const AFTER_HEIGHT: f32 = 30.0;
const WRAPPED_TEXT: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";

struct Model {
    outer: TableLayoutPlan,
    inner: TableLayoutPlan,
}

fn paragraph(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap()
}

fn cell(builder: &mut NodeStoreBuilder, content: NodeId, rowspan: i64) -> NodeId {
    builder
        .insert(
            if rowspan > 1 {
                NodeKind::TableHeader
            } else {
                NodeKind::TableCell
            },
            NodeAttrs::new([("rowspan".into(), AttrValue::Integer(rowspan))].into()).unwrap(),
            NodeContent::children([content]),
        )
        .unwrap()
}

fn table(builder: &mut NodeStoreBuilder, rows: Vec<Vec<NodeId>>) -> NodeId {
    let rows: Vec<_> = rows
        .into_iter()
        .map(|cells| {
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells),
                )
                .unwrap()
        })
        .collect();
    builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap()
}

fn model() -> Rc<Model> {
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder);
    let b = paragraph(&mut builder);
    let a = cell(&mut builder, a, 1);
    let b = cell(&mut builder, b, 1);
    let inner = table(&mut builder, vec![vec![a, b]]);
    let spanning = cell(&mut builder, inner, 2);
    let top = paragraph(&mut builder);
    let bottom = paragraph(&mut builder);
    let top = cell(&mut builder, top, 1);
    let bottom = cell(&mut builder, bottom, 1);
    let outer = table(&mut builder, vec![vec![spanning, top], vec![bottom]]);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([outer]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    Rc::new(Model {
        outer: TableLayoutPlan::from_document(&document, outer, Default::default()).unwrap(),
        inner: TableLayoutPlan::from_document(&document, inner, Default::default()).unwrap(),
    })
}

#[derive(Default)]
struct Observations {
    host_renders: usize,
    builder_sizes: Vec<Size<Pixels>>,
    builder_view_ids: Vec<gpui::EntityId>,
    flow: Vec<Bounds<Pixels>>,
    tables: BTreeMap<&'static str, (Bounds<Pixels>, TableGeometry)>,
}

type SharedObservations = Rc<RefCell<Observations>>;

struct ObservedTable {
    table: SpanningTableElement,
    name: &'static str,
    observed: SharedObservations,
}

impl IntoElement for ObservedTable {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ObservedTable {
    type RequestLayoutState = TableElementLayoutState;
    type PrepaintState = TableElementPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.table.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let prepaint = self
            .table
            .prepaint(id, inspector, bounds, request, window, cx);
        self.observed.borrow_mut().tables.insert(
            self.name,
            (
                bounds,
                request.geometry.as_ref().expect("measured table").clone(),
            ),
        );
        prepaint
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.table
            .paint(id, inspector, bounds, request, prepaint, window, cx);
    }
}

fn viewport(
    model: Rc<Model>,
    scroll: ScrollHandle,
    observed: SharedObservations,
) -> SameFrameTableViewport {
    SameFrameTableViewport::new("same-frame-table-viewport", move |viewport, window, _| {
        {
            let mut observed = observed.borrow_mut();
            observed.builder_sizes.push(viewport);
            observed.builder_view_ids.push(window.current_view());
        }
        let width = f32::from(viewport.width) - VIEWPORT_PADDING * 2.0;
        let outer_widths = model.outer.layout(width, &[0.0; 3]).unwrap();
        let inner_width = outer_widths.cells[0].width - CELL_PADDING * 2.0;
        let inner = SpanningTableElement::new(
            model.inner.clone(),
            px(inner_width),
            model
                .inner
                .cells()
                .iter()
                .map(|cell| {
                    TableCellElement::new(
                        cell.placement.cell(),
                        div().w_full().whitespace_normal().child(WRAPPED_TEXT),
                    )
                })
                .collect(),
        )
        .unwrap();
        let inner = ObservedTable {
            table: inner,
            name: "inner",
            observed: observed.clone(),
        };
        let outer = SpanningTableElement::new(
            model.outer.clone(),
            px(width),
            vec![
                TableCellElement::new(
                    model.outer.cells()[0].placement.cell(),
                    div().w_full().p(px(CELL_PADDING)).child(inner),
                ),
                TableCellElement::new(
                    model.outer.cells()[1].placement.cell(),
                    div().w_full().h(px(32.0)).child("top"),
                ),
                TableCellElement::new(
                    model.outer.cells()[2].placement.cell(),
                    div().w_full().h(px(40.0)).child("bottom"),
                ),
            ],
        )
        .unwrap();
        let outer = ObservedTable {
            table: outer,
            name: "outer",
            observed: observed.clone(),
        };
        let flow = observed.clone();
        let document = div()
            .w_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .on_children_prepainted(move |bounds, _, _| flow.borrow_mut().flow = bounds)
            .child(div().w_full().h(px(BEFORE_HEIGHT)).flex_shrink_0())
            .child(outer)
            .child(div().w_full().h(px(AFTER_HEIGHT)).flex_shrink_0());
        div()
            .id("whole-document-scroll")
            .size_full()
            .flex()
            .flex_col()
            .p(px(VIEWPORT_PADDING))
            .text_size(px(12.0))
            .line_height(px(16.0))
            .track_scroll(&scroll)
            .overflow_y_scroll()
            .child(document)
    })
}

#[derive(Clone, Copy)]
enum HostLayout {
    FillFrom(gpui::Point<Pixels>),
    Fixed {
        origin: gpui::Point<Pixels>,
        size: Size<Pixels>,
        padding: Pixels,
    },
}

struct Host {
    model: Rc<Model>,
    scroll: ScrollHandle,
    observed: SharedObservations,
    layout: HostLayout,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.observed.borrow_mut().host_renders += 1;
        let viewport = viewport(
            self.model.clone(),
            self.scroll.clone(),
            self.observed.clone(),
        );
        let slot = match self.layout {
            HostLayout::FillFrom(origin) => div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .right_0()
                .bottom_0(),
            HostLayout::Fixed {
                origin,
                size,
                padding,
            } => div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(size.width)
                .h(size.height)
                .p(padding)
                .flex()
                .flex_col(),
        };
        div().relative().size_full().child(slot.child(viewport))
    }
}

fn open(
    model: Rc<Model>,
    scroll: ScrollHandle,
    observed: SharedObservations,
    layout: HostLayout,
    window_size: Size<Pixels>,
    cx: &mut TestAppContext,
) -> WindowHandle<Host> {
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.0), px(0.0)),
                    window_size,
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Host {
                    model,
                    scroll,
                    observed,
                    layout,
                })
            },
        )
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    window
}

fn assert_owned_and_settled(
    window: WindowHandle<Host>,
    observed: &SharedObservations,
    cx: &mut TestAppContext,
) {
    let entity_id = window
        .update(cx, |_, _, cx| cx.entity().entity_id())
        .unwrap();
    let before = observed.borrow().host_renders;
    assert!(before > 0, "the actual mounted host must have rendered");
    {
        let observed = observed.borrow();
        assert_eq!(
            observed.builder_sizes.len(),
            before,
            "one builder per host render"
        );
        assert!(observed.builder_view_ids.iter().all(|id| *id == entity_id));
    }
    cx.background_executor.run_until_parked();
    assert_eq!(
        observed.borrow().host_renders,
        before,
        "no adapter notify/refresh loop"
    );
}

fn close(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() < 0.1,
        "{actual:?} != {expected}"
    );
}

fn check_frame(
    observed: &Observations,
    scroll: &ScrollHandle,
    origin: gpui::Point<Pixels>,
    viewport_size: Size<Pixels>,
) {
    let (outer_bounds, outer) = &observed.tables["outer"];
    let (inner_bounds, inner) = &observed.tables["inner"];
    assert_eq!(observed.flow.len(), 3);
    assert_eq!(observed.flow[1], *outer_bounds);
    close(observed.flow[0].size.height, BEFORE_HEIGHT);
    close(observed.flow[2].size.height, AFTER_HEIGHT);
    close(observed.flow[1].top() - observed.flow[0].bottom(), 0.0);
    close(observed.flow[2].top() - observed.flow[1].bottom(), 0.0);
    close(
        outer_bounds.size.width,
        f32::from(viewport_size.width) - 2.0 * VIEWPORT_PADDING,
    );
    close(
        inner_bounds.size.width,
        outer.cells[0].width - 2.0 * CELL_PADDING,
    );
    close(inner_bounds.left() - outer_bounds.left(), CELL_PADDING);
    close(inner_bounds.top() - outer_bounds.top(), CELL_PADDING);
    close(outer_bounds.size.height, outer.height);
    close(inner_bounds.size.height, inner.height);
    assert!(outer.cells[0].height >= inner.height + 2.0 * CELL_PADDING);
    assert_eq!(scroll.bounds(), Bounds::new(origin, viewport_size));
    let content_height = BEFORE_HEIGHT + outer.height + AFTER_HEIGHT + 2.0 * VIEWPORT_PADDING;
    close(
        scroll.max_offset().height,
        (content_height - f32::from(viewport_size.height)).max(0.0),
    );
    close(
        observed.flow[0].top() - origin.y - scroll.offset().y,
        VIEWPORT_PADDING,
    );
}

#[gpui::test]
fn first_frame_has_current_width_sibling_flow_and_complete_scroll_extent(cx: &mut TestAppContext) {
    let observed = Rc::new(RefCell::new(Observations::default()));
    let scroll = ScrollHandle::new();
    let origin = point(px(37.0), px(51.0));
    let viewport_size = size(px(500.0), px(120.0));
    let window = open(
        model(),
        scroll.clone(),
        observed.clone(),
        HostLayout::FillFrom(origin),
        size(
            viewport_size.width + origin.x,
            viewport_size.height + origin.y,
        ),
        cx,
    );
    assert_owned_and_settled(window, &observed, cx);
    let observed = observed.borrow();
    assert!(
        observed
            .builder_sizes
            .iter()
            .all(|size| *size == viewport_size)
    );
    check_frame(&observed, &scroll, origin, viewport_size);
    assert!(scroll.max_offset().height > px(0.0));
}

#[gpui::test]
fn resize_and_scrolling_rebuild_once_with_no_stale_nested_width_or_height(cx: &mut TestAppContext) {
    let model = model();
    let observed = Rc::new(RefCell::new(Observations::default()));
    let scroll = ScrollHandle::new();
    let origin = point(px(37.0), px(51.0));
    let window = open(
        model,
        scroll.clone(),
        observed.clone(),
        HostLayout::FillFrom(origin),
        size(px(500.0) + origin.x, px(120.0) + origin.y),
        cx,
    );
    let mut heights = Vec::new();
    for (frame, width) in [500.0, 300.0, 500.0].into_iter().enumerate() {
        let viewport_size = size(px(width), px(120.0));
        if frame > 0 {
            scroll.set_offset(point(px(0.0), px(-60.0)));
            cx.simulate_window_resize(
                window.into(),
                size(
                    viewport_size.width + origin.x,
                    viewport_size.height + origin.y,
                ),
            );
            cx.background_executor.run_until_parked();
        }
        assert_owned_and_settled(window, &observed, cx);
        let observed = observed.borrow();
        assert_eq!(observed.builder_sizes.last(), Some(&viewport_size));
        check_frame(&observed, &scroll, origin, viewport_size);
        heights.push(observed.tables["outer"].1.height);
        if frame > 0 {
            assert_eq!(scroll.offset().y, px(-60.0));
        }
    }
    assert!(
        heights[1] > heights[0],
        "real nested text must rewrap in narrow columns"
    );
    assert_eq!(heights[0], heights[2]);
}

#[gpui::test]
fn constrained_parent_bounds_win_over_window_space_and_include_parent_insets(
    cx: &mut TestAppContext,
) {
    let observed = Rc::new(RefCell::new(Observations::default()));
    let scroll = ScrollHandle::new();
    let origin = point(px(19.0), px(29.0));
    let window = open(
        model(),
        scroll.clone(),
        observed.clone(),
        HostLayout::Fixed {
            origin,
            size: size(px(320.0), px(200.0)),
            padding: px(10.0),
        },
        size(px(900.0), px(700.0)),
        cx,
    );
    assert_owned_and_settled(window, &observed, cx);
    let actual = size(px(300.0), px(180.0));
    let observed = observed.borrow();
    assert!(observed.builder_sizes.iter().all(|size| *size == actual));
    check_frame(
        &observed,
        &scroll,
        origin + point(px(10.0), px(10.0)),
        actual,
    );
}
