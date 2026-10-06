//! Pure measured hit/preview tests; mounted dispatch lives beside these tests.

use super::geometry::{drag_width, integral_width};
use super::*;
use crate::table_capability::TableCapability;
use crate::table_column_resize::TableColumnResizeConfig;
use gpui::{Bounds, point, px, size};
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

#[path = "lifecycle_tests.rs"]
mod lifecycle;
#[path = "mounted_tests.rs"]
mod mounted;
#[path = "pointer_quantization_tests.rs"]
mod pointer_quantization;
#[path = "timing_tests.rs"]
mod timing;

fn config() -> TableColumnResizeConfig {
    TableColumnResizeConfig {
        handle_width: 5.0,
        min_column_width: 25,
        last_column_resizable: true,
    }
}

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    cells: Vec<NodeId>,
    blocks: Vec<NodeId>,
}

fn fixture(span: bool) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let mut cells = Vec::new();
    let mut blocks = Vec::new();
    for index in 0..if span { 3 } else { 4 } {
        let block = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("wrapping cell content", MarkSet::empty()).unwrap()
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let widths = if span && index == 0 {
            vec![80, 120]
        } else if (!span && index % 2 == 0) || (span && index == 1) {
            vec![80]
        } else {
            vec![120]
        };
        let mut attrs = vec![(
            "colwidth".into(),
            AttrValue::List(widths.into_iter().map(AttrValue::Integer).collect()),
        )];
        if span && index == 0 {
            attrs.push(("colspan".into(), AttrValue::Integer(2)));
        }
        cells.push(
            builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::new(attrs.into_iter().collect()).unwrap(),
                    NodeContent::children([block]),
                )
                .unwrap(),
        );
        blocks.push(block);
    }
    let split = if span { 1 } else { 2 };
    let rows: Vec<_> = [&cells[..split], &cells[split..]]
        .into_iter()
        .map(|cells| {
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells.iter().copied()),
                )
                .unwrap()
        })
        .collect();
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
        blocks,
    }
}

fn session(f: &Fixture) -> SharedSession {
    Rc::new(RefCell::new(
        DocumentSession::new(
            f.document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(f.blocks[0])),
        )
        .unwrap(),
    ))
}

fn measured(f: &Fixture, width: f32) -> ResizeMeasurement {
    let plan = TableLayoutPlan::from_document(&f.document, f.table, Default::default()).unwrap();
    let mut capability = TableCapability::default();
    capability.set_enabled(true);
    ResizeMeasurement {
        table: f.table,
        revision: f.document.revision(),
        document: f.document.clone(),
        key: capability.key(&f.document, f.table).unwrap(),
        origin: point(px(0.0), px(0.0)),
        viewport: None,
        available: width,
        clip: Bounds::new(point(px(-100.0), px(-100.0)), size(px(2000.0), px(2000.0))),
        geometry: plan.layout(width, &vec![50.0; plan.cells().len()]).unwrap(),
        placements: plan.cells().iter().map(|cell| cell.placement).collect(),
    }
}

#[test]
fn edges_respect_spans_cell_handles_clip_and_last_column_policy() {
    let f = fixture(true);
    let measured = measured(&f, 400.0);
    let hit = |x, y, config| {
        measured
            .hit(point(px(x), px(y)), config)
            .map(|intent| intent.column)
    };
    assert_eq!(
        hit(80.0, 25.0, config()),
        None,
        "no imaginary edge inside spanning cell"
    );
    assert_eq!(hit(80.0, 75.0, config()), Some(0));
    assert_eq!(hit(200.0, 25.0, config()), Some(1));
    assert_eq!(
        hit(80.0, 52.0, config()),
        None,
        "cell handle retains priority"
    );
    assert_eq!(hit(201.0, 75.0, config()), Some(1));
    assert_eq!(hit(206.0, 75.0, config()), None);
    assert_eq!(
        hit(0.0, 25.0, config()),
        None,
        "outside left edge has no preceding track"
    );
    assert_eq!(
        hit(
            200.0,
            25.0,
            TableColumnResizeConfig {
                last_column_resizable: false,
                ..config()
            }
        ),
        None
    );
    assert_eq!(
        hit(
            80.0,
            75.0,
            TableColumnResizeConfig {
                last_column_resizable: false,
                ..config()
            }
        ),
        Some(0)
    );
    let mut clipped = measured;
    clipped.clip.size.width = px(150.0);
    assert!(clipped.hit(point(px(80.0), px(75.0)), config()).is_none());
}

#[test]
fn measured_width_contract_refuses_fractional_starts_and_invalid_configuration() {
    for unsupported in [0.0, 80.5, f32::NAN, f32::INFINITY, 1_000_001.0] {
        assert_eq!(integral_width(unsupported), None);
    }
    assert_eq!(integral_width(80.0), Some(80));
    assert_eq!(drag_width(80, 100.5, 140.5, 25), Some(120));
    assert_eq!(drag_width(80, 100.5, 140.0, 25), Some(120));
    assert_eq!(drag_width(80, 100.0, -500.0, 25), Some(25));
    assert_eq!(drag_width(80, 100.0, f32::NAN, 25), None);
    assert!(
        !TableColumnResizeConfig {
            handle_width: 0.0,
            ..config()
        }
        .valid()
    );
    assert!(
        !TableColumnResizeConfig {
            min_column_width: 0,
            ..config()
        }
        .valid()
    );
}

#[test]
fn exact_preview_track_is_below_automatic_minimum_without_document_changes() {
    let f = fixture(true);
    let mut plan =
        TableLayoutPlan::from_document(&f.document, f.table, Default::default()).unwrap();
    plan.override_column_width(0, 25).unwrap();
    let geometry = plan.layout(400.0, &[60.0, 90.0, 40.0]).unwrap();
    assert_eq!(geometry.column_edges, vec![0.0, 25.0, 145.0]);
    assert_eq!(geometry.cells[0].width, 145.0);
    assert_eq!(geometry.cells.len(), 3);
    assert_eq!(
        TableLayoutPlan::from_document(&f.document, f.table, Default::default())
            .unwrap()
            .column_widths(400.0)
            .unwrap(),
        vec![80.0, 120.0]
    );
    assert!(plan.override_column_width(2, 25).is_err());
    assert!(plan.override_column_width(0, 0).is_err());
}

#[test]
fn covered_rows_share_origin_edges_and_do_not_duplicate_cells() {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let mut f = fixture(false);
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for cell in &f.cells[..2] {
        let mut attrs = f
            .document
            .node(*cell)
            .unwrap()
            .attrs()
            .iter()
            .map(|(k, v)| (k.to_owned(), v.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        attrs.insert("rowspan".into(), AttrValue::Integer(2));
        transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
            node: *cell,
            attrs: NodeAttrs::new(attrs).unwrap(),
        });
    }
    for cell in &f.cells[2..] {
        transaction = transaction.with_step(TransactionStep::RemoveNode { node: *cell });
    }
    f.document = transaction.apply(&f.document).unwrap();
    let measured = measured(&f, 400.0);
    assert_eq!(measured.geometry.cells.len(), 2);
    assert_eq!(measured.geometry.row_edges.len(), 3);
    assert_eq!(
        measured
            .hit(point(px(80.0), px(45.0)), config())
            .unwrap()
            .column,
        0
    );
}

fn nested_fixture() -> (Fixture, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let mut blocks = Vec::new();
    let mut cells = Vec::new();
    for width in [80, 120] {
        let block = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([
                        TextRun::new("nested wrapping content", MarkSet::empty()).unwrap()
                    ])
                    .unwrap(),
                ),
            )
            .unwrap();
        let cell = builder
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
                NodeContent::children([block]),
            )
            .unwrap();
        blocks.push(block);
        cells.push(cell);
    }
    let inner_row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells.iter().copied()),
        )
        .unwrap();
    let inner = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([inner_row]),
        )
        .unwrap();
    let outer_cell = builder
        .insert(
            NodeKind::TableCell,
            NodeAttrs::new(
                [(
                    "colwidth".into(),
                    AttrValue::List(vec![AttrValue::Integer(260)]),
                )]
                .into(),
            )
            .unwrap(),
            NodeContent::children([inner]),
        )
        .unwrap();
    let outer_row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children([outer_cell]),
        )
        .unwrap();
    let outer = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([outer_row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([outer]),
        )
        .unwrap();
    (
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            table: inner,
            cells,
            blocks,
        },
        outer,
    )
}

#[test]
fn fractional_automatic_start_is_refused_and_other_automatic_tracks_reflow() {
    use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
    let mut f = fixture(false);
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for cell in &f.cells {
        transaction = transaction.with_step(TransactionStep::SetNodeAttrs {
            node: *cell,
            attrs: NodeAttrs::empty(),
        });
    }
    f.document = transaction.apply(&f.document).unwrap();
    let fractional = measured(&f, 161.0);
    assert_eq!(fractional.geometry.column_edges, vec![0.0, 80.5, 161.0]);
    assert!(
        fractional
            .hit(point(px(80.5), px(25.0)), config())
            .is_none()
    );
    assert!(
        measured(&f, 160.0)
            .hit(point(px(80.0), px(25.0)), config())
            .is_some()
    );
    let mut plan =
        TableLayoutPlan::from_document(&f.document, f.table, Default::default()).unwrap();
    plan.override_column_width(0, 25).unwrap();
    assert_eq!(plan.column_widths(160.0).unwrap(), vec![25.0, 135.0]);
    assert_eq!(plan.column_widths(50.0).unwrap(), vec![25.0, 40.0]);
}
