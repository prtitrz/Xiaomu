use super::*;
use crate::document::{NodeStoreBuilder, XiaomuDocument};
use crate::mapping::{MapBias, MappedPosition};
use crate::selection::{CursorAffinity, InlinePoint, NodeGap, NodeSelection};
use crate::text::TextOffset;
use crate::transaction::{AppliedTransaction, Transaction, TransactionOrigin};

mod failures;
mod fixtures;
mod rich_boundaries;
use fixtures::*;

#[test]
fn each_edge_and_both_axes_produce_bounded_strips_with_one_rich_survivor() {
    let (before, table, original) = span_fixture(
        3,
        3,
        NodeKind::TableHeader,
        attrs(&[
            ("backgroundColor", AttrValue::String("red".into())),
            ("align", AttrValue::Null),
        ]),
    );
    let cases = [
        ((1, 0, 3, 3), vec![(0, 0, 1, 3), (1, 0, 2, 3)]),
        ((0, 0, 2, 3), vec![(0, 0, 2, 3), (2, 0, 1, 3)]),
        ((0, 1, 3, 3), vec![(0, 0, 3, 1), (0, 1, 3, 2)]),
        ((0, 0, 3, 2), vec![(0, 0, 3, 2), (0, 2, 3, 1)]),
        (
            (1, 1, 2, 2),
            vec![
                (0, 0, 1, 3),
                (1, 0, 1, 1),
                (1, 1, 1, 1),
                (1, 2, 1, 1),
                (2, 0, 1, 3),
            ],
        ),
    ];
    for ((top, left, bottom, right), expected) in cases {
        let rect = TableRect::new(top, left, bottom, right).unwrap();
        let result = apply(&before, table, rect);
        let after = result.document();
        let grid = after.table_grid(table).unwrap();
        assert!(grid.is_closed_rect(rect));
        assert_eq!(
            grid.origins()
                .map(|p| (p.row(), p.column(), p.rowspan(), p.colspan()))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(grid.slot(0, 0), Some(original));
        assert_eq!(children(after, original), children(&before, original));
        assert_eq!(after.node(table), before.node(table));
        for p in grid.origins() {
            let node = after.node(p.cell()).unwrap();
            assert_eq!(node.kind(), &NodeKind::TableHeader);
            assert_eq!(
                node.attrs().get("backgroundColor"),
                before
                    .node(original)
                    .unwrap()
                    .attrs()
                    .get("backgroundColor")
            );
            assert_eq!(node.attrs().get("align"), Some(&AttrValue::Null));
            if p.cell() != original {
                let blocks = children(after, p.cell());
                assert_eq!(blocks.len(), 1);
                let paragraph = after.node(blocks[0]).unwrap();
                assert_eq!(paragraph.kind(), &NodeKind::Paragraph);
                assert!(paragraph.attrs().is_empty());
                assert_eq!(paragraph.content(), &NodeContent::empty_inline());
            }
        }
        for node in before
            .store()
            .iter()
            .filter(|node| node.id() != original && !matches!(node.kind(), NodeKind::TableRow))
        {
            assert_eq!(after.node(node.id()), Some(node));
        }
        for row in children(&before, table) {
            assert_eq!(
                after.node(*row).unwrap().attrs(),
                before.node(*row).unwrap().attrs()
            );
        }
        round_trip(&before, &result);
    }
}

#[test]
fn width_presence_and_only_actual_column_slices_are_normalized() {
    for width in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::List(vec![AttrValue::Integer(0); 3])),
        Some(AttrValue::List(vec![
            AttrValue::Integer(0),
            AttrValue::Integer(80),
            AttrValue::Integer(0),
        ])),
    ] {
        let extras = width
            .clone()
            .map_or_else(NodeAttrs::empty, |w| attrs(&[("colwidth", w)]));
        let (before, table, original) = span_fixture(3, 3, NodeKind::TableCell, extras);
        let horizontal = apply(&before, table, TableRect::new(1, 0, 2, 3).unwrap());
        for p in horizontal.document().table_grid(table).unwrap().origins() {
            assert_eq!(
                horizontal
                    .document()
                    .node(p.cell())
                    .unwrap()
                    .attrs()
                    .get("colwidth"),
                width.as_ref()
            );
        }
        let both = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
        for p in both.document().table_grid(table).unwrap().origins() {
            let actual = both
                .document()
                .node(p.cell())
                .unwrap()
                .attrs()
                .get("colwidth");
            if p.row() != 1 {
                assert_eq!(actual, width.as_ref());
            } else {
                let expected = match &width {
                    Some(AttrValue::List(w)) if w[p.column()] != AttrValue::Integer(0) => {
                        Some(AttrValue::List(vec![w[p.column()].clone()]))
                    }
                    Some(AttrValue::List(_)) => Some(AttrValue::Null),
                    other => other.clone(),
                };
                assert_eq!(actual, expected.as_ref());
            }
        }
        assert_eq!(
            children(both.document(), original),
            children(&before, original)
        );
        round_trip(&before, &both);
    }
    // A column-only edit must not insert an absent or rewrite explicit unit rowspan.
    for span in [None, Some(AttrValue::Integer(1))] {
        let extra = span
            .clone()
            .map_or_else(NodeAttrs::empty, |value| attrs(&[("rowspan", value)]));
        let (before, table, _) = span_fixture(1, 3, NodeKind::TableCell, extra);
        let result = apply(&before, table, TableRect::new(0, 1, 1, 2).unwrap());
        for p in result.document().table_grid(table).unwrap().origins() {
            assert_eq!(
                result
                    .document()
                    .node(p.cell())
                    .unwrap()
                    .attrs()
                    .get("rowspan"),
                span.as_ref()
            );
        }
    }
}

#[test]
fn raw_empty_rows_survive_temporary_population_and_exact_undo() {
    let (before, table, _) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let rows = children(&before, table);
    assert!(children(&before, rows[1]).is_empty());
    assert!(children(&before, rows[2]).is_empty());
    let result = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
    assert_eq!(children(result.document(), rows[1]).len(), 3);
    assert_eq!(children(result.document(), rows[2]).len(), 1);
    for row in rows {
        assert_eq!(
            result.document().node(*row).unwrap().attrs(),
            before.node(*row).unwrap().attrs()
        );
    }
    round_trip(&before, &result);
}

#[test]
fn old_content_cell_selection_and_row_gaps_map_and_redo_reuses_first_ids() {
    let (before, table, original) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let result = apply(&before, table, TableRect::new(1, 1, 2, 2).unwrap());
    let paragraph = children(&before, original)[0];
    let point = InlinePoint::new(paragraph, TextOffset::ZERO, 1, CursorAffinity::After);
    assert_eq!(
        result.changes().map_inline_point(point, MapBias::End),
        MappedPosition::Mapped(point)
    );
    assert_eq!(
        result
            .changes()
            .map_node_selection(NodeSelection::new(original)),
        MappedPosition::Mapped(NodeSelection::new(original))
    );
    let row = children(&before, table)[1];
    for (bias, index) in [(MapBias::Start, 0), (MapBias::End, 3)] {
        assert_eq!(
            result.changes().map_node_gap(NodeGap::new(row, 0), bias),
            MappedPosition::Mapped(NodeGap::new(row, index))
        );
    }
    let new_cell = result
        .document()
        .table_grid(table)
        .unwrap()
        .slot(1, 1)
        .unwrap();
    let new_p = children(result.document(), new_cell)[0];
    let undo = result
        .inverse()
        .apply_with_changes(result.document())
        .unwrap();
    assert_eq!(
        undo.changes().map_node_selection(NodeSelection::new(new_p)),
        MappedPosition::Deleted
    );
    assert_eq!(
        undo.changes().map_inline_point(point, MapBias::Start),
        MappedPosition::Mapped(point)
    );
    round_trip(&before, &result);
}

#[test]
fn closed_repeat_has_no_exchange_maps_inverse_or_allocator_effect() {
    let (before, table, _) = span_fixture(3, 3, NodeKind::TableCell, NodeAttrs::empty());
    let mut ctx = context(&before);
    ctx.next_node_id = u64::MAX;
    let store = ctx.store.clone();
    let (maps, inverse) = ctx
        .apply_isolate_table_rect(table, TableRect::new(0, 0, 3, 3).unwrap())
        .unwrap();
    assert!(maps.is_empty() && inverse.is_empty());
    assert_eq!(ctx.store.map_storage_id(), store.map_storage_id());
    assert_eq!(ctx.next_node_id, u64::MAX);
    let rect = TableRect::new(1, 1, 2, 2).unwrap();
    let result = apply(&before, table, rect);
    let repeated = apply(result.document(), table, rect);
    assert!(repeated.changes().steps().is_empty());
    assert!(repeated.inverse().steps().is_empty());
    assert_eq!(repeated.document().store(), result.document().store());
    assert_eq!(
        repeated.document().next_node_id(),
        result.document().next_node_id()
    );
}

#[test]
fn large_span_uses_four_extra_cells_not_area_expansion() {
    // Leave one aggregate slot for the original rich forest's nested table.
    let (before, table, _) = span_fixture(999, 1000, NodeKind::TableHeader, NodeAttrs::empty());
    let result = apply(&before, table, TableRect::new(100, 100, 900, 900).unwrap());
    assert_eq!(
        result.document().table_grid(table).unwrap().origins().len(),
        5
    );
    assert_eq!(result.document().store().len(), before.store().len() + 8);
    assert_eq!(result.document().next_node_id(), before.next_node_id() + 8);
    round_trip(&before, &result);
}

#[test]
fn all_small_interior_and_boundary_rectangles_partition_without_holes() {
    let (before, table, original) = span_fixture(5, 5, NodeKind::TableCell, NodeAttrs::empty());
    for top in 0..5 {
        for bottom in top + 1..=5 {
            for left in 0..5 {
                for right in left + 1..=5 {
                    let rect = TableRect::new(top, left, bottom, right).unwrap();
                    let result = apply(&before, table, rect);
                    let grid = result.document().table_grid(table).unwrap();
                    assert!(grid.is_closed_rect(rect));
                    assert!(grid.origins().len() <= 5);
                    assert_eq!(grid.slot(0, 0), Some(original));
                    assert_eq!(
                        grid.origins()
                            .map(|p| p.rowspan() * p.colspan())
                            .sum::<usize>(),
                        25
                    );
                    assert_eq!(
                        children(result.document(), original),
                        children(&before, original)
                    );
                }
            }
        }
    }
}
