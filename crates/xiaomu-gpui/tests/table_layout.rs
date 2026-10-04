//! Isolated table-layout prototype checks, not production-span admission.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    AppContext as _, AvailableSpace, EntityInputHandler, ParentElement, Styled,
    TestAppContext, VisualTestContext, div, point, px, size,
};
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_gpui::block_view::{BlockBoundsRegistry, ParagraphView, SharedSession};
use xiaomu_gpui::table_layout::{
    SpanningTableElement, TableCellElement, TableLayoutError, TableLayoutOptions, TableLayoutPlan,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

struct CellSpec {
    columns: i64,
    rows: i64,
    widths: Option<AttrValue>,
    header: bool,
    text: &'static str,
}

fn spec(columns: i64, rows: i64, widths: Option<&[i64]>) -> CellSpec {
    CellSpec {
        columns,
        rows,
        widths: widths.map(|widths| {
            AttrValue::List(widths.iter().copied().map(AttrValue::Integer).collect())
        }),
        header: false,
        text: "cell",
    }
}

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    cells: Vec<NodeId>,
    paragraphs: Vec<NodeId>,
}

fn fixture(rows: Vec<Vec<CellSpec>>) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let mut cells = Vec::new();
    let mut paragraphs = Vec::new();
    let rows = rows
        .into_iter()
        .map(|row| {
            let children = row
                .into_iter()
                .map(|spec| {
                    let paragraph = builder
                        .insert(
                            NodeKind::Paragraph,
                            NodeAttrs::empty(),
                            NodeContent::Inline(
                                InlineContent::new([
                                    TextRun::new(spec.text, MarkSet::empty()).unwrap()
                                ])
                                .unwrap(),
                            ),
                        )
                        .unwrap();
                    let mut attrs = vec![
                        ("colspan".into(), AttrValue::Integer(spec.columns)),
                        ("rowspan".into(), AttrValue::Integer(spec.rows)),
                    ];
                    if let Some(widths) = spec.widths {
                        attrs.push(("colwidth".into(), widths));
                    }
                    let cell = builder
                        .insert(
                            if spec.header {
                                NodeKind::TableHeader
                            } else {
                                NodeKind::TableCell
                            },
                            NodeAttrs::new(attrs.into_iter().collect()).unwrap(),
                            NodeContent::children([paragraph]),
                        )
                        .unwrap();
                    cells.push(cell);
                    paragraphs.push(paragraph);
                    cell
                })
                .collect::<Vec<_>>();
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(children),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        cells,
        paragraphs,
    }
}

fn plan(fixture: &Fixture) -> TableLayoutPlan {
    TableLayoutPlan::from_document(
        &fixture.document,
        fixture.table,
        TableLayoutOptions::default(),
    )
    .unwrap()
}

#[test]
fn missing_null_zero_are_automatic_without_changing_the_document() {
    let mut null = spec(1, 1, None);
    null.widths = Some(AttrValue::Null);
    let fixture = fixture(vec![vec![spec(1, 1, None), null, spec(1, 1, Some(&[0]))]]);
    let before = fixture.document.clone();
    assert_eq!(plan(&fixture).column_widths(300.0).unwrap(), [100.0; 3]);
    assert_eq!(fixture.document.store(), before.store());
}

#[test]
fn positive_hints_are_shared_by_logical_column_across_spans_and_rows() {
    let mut header = spec(2, 1, Some(&[80, 0]));
    header.header = true;
    let fixture = fixture(vec![
        vec![header, spec(1, 1, None)],
        vec![spec(1, 1, None), spec(1, 1, Some(&[120])), spec(1, 1, None)],
    ]);
    let plan = plan(&fixture);
    assert_eq!(plan.column_widths(400.0).unwrap(), [80.0, 120.0, 200.0]);
    let layout = plan.layout(400.0, &[30.0; 5]).unwrap();
    assert!(layout.cells[0].is_header);
    assert_eq!(layout.cells[0].width, 200.0);
    assert_eq!(layout.cells[1].x, 200.0);
    assert_eq!(layout.cells[3].x, 80.0);
    assert_eq!(layout.cells[4].width, 200.0);
}

#[test]
fn fixed_tracks_do_not_grow_or_shrink_and_auto_tracks_overflow_at_the_minimum() {
    let fixed = fixture(vec![vec![spec(2, 1, Some(&[80, 120]))]]);
    assert_eq!(plan(&fixed).column_widths(900.0).unwrap(), [80.0, 120.0]);
    assert_eq!(plan(&fixed).layout(50.0, &[40.0]).unwrap().width, 200.0);
    let mixed = fixture(vec![vec![spec(2, 1, Some(&[100, 0]))]]);
    assert_eq!(plan(&mixed).column_widths(80.0).unwrap(), [100.0, 40.0]);
}

#[test]
fn conflicting_positive_hints_are_refused_without_first_or_last_wins_repair() {
    let fixture = fixture(vec![
        vec![spec(1, 1, Some(&[80]))],
        vec![spec(1, 1, Some(&[120]))],
    ]);
    let before = fixture.document.clone();
    assert!(matches!(
        TableLayoutPlan::from_document(&fixture.document, fixture.table, Default::default()),
        Err(TableLayoutError::ConflictingColumnWidth { column: 0 })
    ));
    assert_eq!(fixture.document.store(), before.store());
}

#[test]
fn real_height_constraints_cover_rowspans_without_duplicate_children() {
    let fixture = fixture(vec![
        vec![spec(2, 2, None), spec(1, 1, None)],
        vec![spec(1, 1, None)],
        vec![spec(1, 1, None), spec(2, 1, None)],
    ]);
    let layout = plan(&fixture)
        .layout(300.0, &[150.0, 30.0, 50.0, 20.0, 60.0])
        .unwrap();
    assert_eq!(layout.row_edges, [0.0, 65.0, 150.0, 210.0]);
    assert_eq!(layout.cells.len(), 5);
    assert_eq!(layout.cells[0].height, 150.0);
    assert_eq!(layout.cells[2].y, 65.0);
    assert_eq!(layout.cells[4].x, 100.0);
    assert_eq!(layout.cells[4].width, 200.0);
    for cell in &layout.cells {
        assert!(cell.height >= cell.measured_height);
    }
}

#[test]
fn completely_covered_empty_rows_still_receive_measured_height() {
    let fixture = fixture(vec![vec![spec(1, 3, None)], vec![], vec![]]);
    let layout = plan(&fixture).layout(90.0, &[120.0]).unwrap();
    assert_eq!(layout.row_edges, [0.0, 40.0, 80.0, 120.0]);
    assert_eq!(layout.cells.len(), 1);
    assert_eq!(layout.cells[0].height, 120.0);
}

#[test]
fn nonfinite_oversized_and_mismatched_measurements_fail_closed() {
    let fixture = fixture(vec![vec![spec(1, 1, None)]]);
    let plan = plan(&fixture);
    for width in [f32::NAN, f32::INFINITY, -1.0, 1_000_001.0] {
        assert_eq!(
            plan.column_widths(width),
            Err(TableLayoutError::InvalidDimension)
        );
    }
    for height in [f32::NAN, f32::INFINITY, -1.0, 1_000_001.0] {
        assert_eq!(
            plan.layout(100.0, &[height]),
            Err(TableLayoutError::InvalidDimension)
        );
    }
    assert_eq!(plan.layout(100.0, &[]), Err(TableLayoutError::CellMismatch));
    let huge = fixture_with_huge_width();
    assert!(matches!(
        TableLayoutPlan::from_document(&huge.document, huge.table, Default::default()),
        Err(TableLayoutError::InvalidDimension)
    ));
}

fn fixture_with_huge_width() -> Fixture {
    fixture(vec![vec![spec(1, 1, Some(&[i64::MAX]))]])
}

#[test]
fn cell_bounds_translate_full_span_and_constructor_checks_identity() {
    let fixture = fixture(vec![vec![spec(1, 2, None), spec(1, 2, None)], vec![]]);
    let plan = plan(&fixture);
    let geometry = plan.layout(200.0, &[80.0, 100.0]).unwrap();
    let bounds = geometry.cells[1].bounds_at(point(px(37.0), px(59.0)));
    assert_eq!(bounds.origin, point(px(137.0), px(59.0)));
    assert_eq!(bounds.size, size(px(100.0), px(100.0)));
    let children = vec![
        TableCellElement::new(fixture.cells[1], gpui::Empty),
        TableCellElement::new(fixture.cells[0], gpui::Empty),
    ];
    assert!(matches!(
        SpanningTableElement::new(plan, px(200.0), children),
        Err(TableLayoutError::CellMismatch)
    ));
}

struct ViewFixture {
    session: SharedSession,
    bounds: BlockBoundsRegistry,
    views: Vec<gpui::Entity<ParagraphView>>,
    visual: VisualTestContext,
}

fn views(fixture: &Fixture, cx: &mut TestAppContext) -> ViewFixture {
    let focus = *fixture.paragraphs.last().unwrap();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            fixture.document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(focus)),
        )
        .unwrap(),
    ));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let views = cx.update(|cx| {
        fixture
            .paragraphs
            .iter()
            .map(|node| {
                cx.new(|cx| {
                    ParagraphView::new(
                        session.clone(),
                        Rc::new(Cell::new(0)),
                        bounds.clone(),
                        *node,
                        cx,
                    )
                })
            })
            .collect()
    });
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| gpui::Empty))
            .unwrap()
    });
    ViewFixture {
        session,
        bounds,
        views,
        visual: VisualTestContext::from_window(window.into(), cx),
    }
}

fn draw(
    fixture: &Fixture,
    views: &mut ViewFixture,
    width: f32,
    origin: gpui::Point<gpui::Pixels>,
) -> (
    xiaomu_gpui::table_layout::TableElementLayoutState,
    xiaomu_gpui::table_layout::TableElementPrepaintState,
) {
    views.bounds.borrow_mut().clear();
    let children = fixture
        .cells
        .iter()
        .zip(&views.views)
        .map(|(cell, view)| TableCellElement::new(*cell, view.clone()))
        .collect();
    let element = SpanningTableElement::new(plan(fixture), px(width), children).unwrap();
    views.visual.draw(
        origin,
        size(
            AvailableSpace::Definite(px(width)),
            AvailableSpace::MaxContent,
        ),
        |_, _| element,
    )
}

#[gpui::test]
fn genuine_child_bounds_follow_spans_and_absolute_prepaint_origin(cx: &mut TestAppContext) {
    let mut header = spec(2, 2, Some(&[70, 110]));
    header.header = true;
    let fixture = fixture(vec![vec![header, spec(1, 1, None)], vec![spec(1, 1, None)]]);
    let mut views = views(&fixture, cx);
    let origin = point(px(31.0), px(47.0));
    let (request, prepaint) = draw(&fixture, &mut views, 300.0, origin);
    let geometry = request.geometry.unwrap();
    assert_eq!(geometry.cells.len(), 3);
    assert!(geometry.cells[0].is_header);
    assert_eq!(prepaint.cells.len(), 3);
    let recorded = views.bounds.borrow().clone();
    assert_eq!(recorded.len(), 3);
    for (index, cell) in geometry.cells.iter().enumerate() {
        assert_eq!(recorded[index].0, fixture.paragraphs[index]);
        assert_eq!(recorded[index].1.origin, cell.bounds_at(origin).origin);
        assert_eq!(recorded[index].1.size.width, px(cell.width));
        assert!(recorded[index].1.size.height <= px(cell.height));
    }
    // The isolated layout does not relax production's protected-span input.
    let view = views.views[2].clone();
    views.visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(
                view.bounds_for_range(0..0, recorded[2].1, window, cx)
                    .is_none()
            );
        });
    });
    assert_eq!(views.session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn width_changes_remeasure_real_wrapped_children_and_keep_native_query_geometry(
    cx: &mut TestAppContext,
) {
    let mut long = spec(1, 1, None);
    long.text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
    let fixture = fixture(vec![vec![spec(1, 1, Some(&[80])), long]]);
    let mut views = views(&fixture, cx);
    let mut heights = Vec::new();
    for width in [400.0, 180.0, 400.0] {
        let origin = point(px(29.0), px(43.0));
        let (request, prepaint) = draw(&fixture, &mut views, width, origin);
        let geometry = request.geometry.unwrap();
        heights.push(geometry.height);
        assert_eq!(geometry.cells[0].width, 80.0);
        assert_eq!(geometry.cells[1].width, width - 80.0);
        let recorded = views.bounds.borrow().clone();
        assert_eq!(recorded[1].1.origin, prepaint.cells[1].1.origin);
        assert_eq!(recorded[1].1.size.width, px(width - 80.0));
        let view = views.views[1].clone();
        views.visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let caret = view
                    .bounds_for_range(0..0, recorded[1].1, window, cx)
                    .unwrap();
                assert_eq!(caret.origin, recorded[1].1.origin);
                assert_eq!(
                    view.character_index_for_point(
                        caret.origin + point(px(0.1), px(1.0)),
                        window,
                        cx
                    ),
                    Some(0)
                );
            });
        });
    }
    assert!(
        heights[1] > heights[0],
        "narrow tracks must rewrap actual child text"
    );
    assert_eq!(heights[0], heights[2]);
    assert_eq!(
        views.session.borrow().document().store(),
        fixture.document.store()
    );
    assert_eq!(views.session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn padded_content_and_preedit_keep_natural_input_bounds_inside_taller_cells(
    cx: &mut TestAppContext,
) {
    let mut tall = spec(1, 1, Some(&[80]));
    tall.text = "one two three four five six seven eight nine ten eleven twelve";
    let fixture = fixture(vec![vec![tall, spec(1, 1, Some(&[160]))]]);
    let mut views = views(&fixture, cx);
    let focused = views.views[1].clone();
    views.visual.update(|window, cx| {
        focused.update(cx, |view, cx| {
            view.replace_and_mark_text_in_range(None, "中🙂中文", Some(4..4), window, cx);
            assert_eq!(
                view.selected_text_range(false, window, cx).unwrap().range,
                4..4
            );
        });
    });
    views.bounds.borrow_mut().clear();
    let children = fixture
        .cells
        .iter()
        .zip(&views.views)
        .map(|(cell, view)| {
            TableCellElement::new(
                *cell,
                div().w_full().px(px(12.0)).py(px(9.0)).child(view.clone()),
            )
        })
        .collect();
    let element = SpanningTableElement::new(plan(&fixture), px(300.0), children).unwrap();
    let origin = point(px(17.0), px(23.0));
    let (request, prepaint) = views.visual.draw(
        origin,
        size(
            AvailableSpace::Definite(px(300.0)),
            AvailableSpace::MaxContent,
        ),
        |_, _| element,
    );
    let geometry = request.geometry.unwrap();
    let recorded = views.bounds.borrow().clone();
    let full = prepaint.cells[1].1;
    let text = recorded[1].1;
    assert_eq!(text.origin, full.origin + point(px(12.0), px(9.0)));
    assert_eq!(text.size.width, full.size.width - px(24.0));
    assert!(geometry.cells[1].height > geometry.cells[1].measured_height);
    assert!(text.bottom() < full.bottom() - px(9.0));
    views.visual.update(|window, cx| {
        focused.update(cx, |view, cx| {
            let caret = view.bounds_for_range(4..4, text, window, cx).unwrap();
            assert!(caret.left() >= text.left() && caret.right() <= text.right());
            assert!(caret.top() >= text.top() && caret.bottom() <= text.bottom());
            assert_eq!(view.marked_text_range(window, cx), Some(0..5));
            view.replace_and_mark_text_in_range(None, "", None, window, cx);
        });
    });
    assert_eq!(
        views.session.borrow().document().store(),
        fixture.document.store()
    );
    assert_eq!(views.session.borrow().history_depths(), (0, 0));
}

/// Deliberately violates the direct Element lifecycle; the ordinary GPUI
/// AnyElement wrapper would reject a second request before reaching this code.
struct RepeatedRequest(SpanningTableElement);

impl gpui::IntoElement for RepeatedRequest {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for RepeatedRequest {
    type RequestLayoutState = xiaomu_gpui::table_layout::TableElementLayoutState;
    type PrepaintState = xiaomu_gpui::table_layout::TableElementPrepaintState;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let (_, first) = self.0.request_layout(id, inspector, window, cx);
        assert!(first.geometry.is_ok());
        self.0.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request: &mut Self::RequestLayoutState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> Self::PrepaintState {
        self.0.prepaint(id, inspector, bounds, request, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&gpui::GlobalElementId>,
        inspector: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        request: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.0
            .paint(id, inspector, bounds, request, prepaint, window, cx);
    }
}

#[gpui::test]
fn repeated_request_is_explicitly_refused_instead_of_becoming_an_empty_table(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(vec![vec![spec(1, 1, None)]]);
    let mut views = views(&fixture, cx);
    let table = SpanningTableElement::new(
        plan(&fixture),
        px(200.0),
        vec![TableCellElement::new(
            fixture.cells[0],
            views.views[0].clone(),
        )],
    )
    .unwrap();
    let (request, prepaint) = views.visual.draw(
        point(px(13.0), px(17.0)),
        size(
            AvailableSpace::Definite(px(200.0)),
            AvailableSpace::MaxContent,
        ),
        |_, _| RepeatedRequest(table),
    );
    assert_eq!(
        request.geometry,
        Err(TableLayoutError::RepeatedLayoutRequest)
    );
    let unavailable = prepaint.unavailable_bounds.expect("visible refusal");
    assert!(unavailable.size.width > px(0.0) && unavailable.size.height >= px(28.0));
    assert!(prepaint.cells.is_empty());
    assert!(
        views.bounds.borrow().is_empty(),
        "failed children must not prepaint"
    );
}

#[gpui::test]
fn measurement_error_is_visible_and_never_prepaints_partially_measured_inputs(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(vec![vec![spec(1, 1, None)]]);
    let mut views = views(&fixture, cx);
    let table = SpanningTableElement::new(
        plan(&fixture),
        px(200.0),
        vec![TableCellElement::new(
            fixture.cells[0],
            div().h(px(1_000_001.0)).child(views.views[0].clone()),
        )],
    )
    .unwrap();
    let (request, prepaint) = views.visual.draw(
        point(px(13.0), px(17.0)),
        size(
            AvailableSpace::Definite(px(200.0)),
            AvailableSpace::MaxContent,
        ),
        |_, _| table,
    );
    assert_eq!(request.geometry, Err(TableLayoutError::InvalidDimension));
    let unavailable = prepaint.unavailable_bounds.expect("visible refusal");
    assert_eq!(unavailable.origin, point(px(13.0), px(17.0)));
    assert!(unavailable.size.width > px(0.0) && unavailable.size.height >= px(28.0));
    assert!(prepaint.cells.is_empty());
    assert!(
        views.bounds.borrow().is_empty(),
        "failed children must not prepaint"
    );
    assert_eq!(views.session.borrow().history_depths(), (0, 0));
}
