use super::*;
use crate::transaction::TableTreeTemplate;

fn fixture(four_crossings: bool) -> (XiaomuDocument, NodeId, Vec<NodeId>) {
    let size = if four_crossings { 5 } else { 3 };
    // Independent authored geometry: top, left, right and bottom rich origins.
    let specs = [(0, 2, 2, 1), (2, 0, 1, 2), (2, 3, 1, 2), (3, 2, 2, 1)];
    let mut builder = NodeStoreBuilder::new();
    let mut rows = Vec::new();
    let mut originals = Vec::new();
    for row in 0..size {
        let mut cells = Vec::new();
        for col in 0..size {
            let covering = four_crossings
                .then(|| {
                    specs.iter().position(|&(r, c, h, w)| {
                        r <= row && row < r + h && c <= col && col < c + w
                    })
                })
                .flatten();
            if let Some(index) = covering {
                let (r, c, h, w) = specs[index];
                if row != r || col != c {
                    continue;
                }
                let label = ["TOP", "LEFT", "RIGHT", "BOTTOM"][index];
                let blocks = rich(&mut builder, label);
                let cell = builder
                    .insert(
                        if index % 2 == 0 {
                            NodeKind::TableHeader
                        } else {
                            NodeKind::TableCell
                        },
                        attrs(&[
                            ("rowspan", AttrValue::Integer(h as i64)),
                            ("colspan", AttrValue::Integer(w as i64)),
                            ("tag", AttrValue::String(label.into())),
                        ]),
                        NodeContent::children(blocks),
                    )
                    .unwrap();
                originals.push(cell);
                cells.push(cell);
            } else {
                let paragraph = builder
                    .insert(
                        NodeKind::Paragraph,
                        NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap();
                cells.push(
                    builder
                        .insert(
                            NodeKind::TableCell,
                            NodeAttrs::empty(),
                            NodeContent::children([paragraph]),
                        )
                        .unwrap(),
                );
            }
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs(&[("row", AttrValue::Integer(row as i64))]),
                    NodeContent::children(cells),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let quote = builder
        .insert(
            NodeKind::Quote,
            attrs(&[("outside", AttrValue::Null)]),
            NodeContent::children([table]),
        )
        .unwrap();
    let item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([quote]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let lead = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let tail = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([lead, list, tail]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        originals,
    )
}

#[test]
fn four_distinct_rich_origins_and_nested_outside_nodes_are_stable() {
    let (before, table, originals) = fixture(true);
    assert_eq!(originals.len(), 4);
    let grid = before.table_grid(table).unwrap();
    let rect = TableRect::new(1, 1, 4, 4).unwrap();
    let result = apply(&before, table, rect);
    let after = result.document();
    let next = after.table_grid(table).unwrap();
    assert!(next.is_closed_rect(rect));
    assert_eq!(next.origins().len(), grid.origins().len() + 4);
    for original in &originals {
        assert_eq!(children(after, *original), children(&before, *original));
        let old = grid.placement(*original).unwrap();
        let new = next.placement(*original).unwrap();
        assert_eq!((new.row(), new.column()), (old.row(), old.column()));
    }
    for origin in grid.origins().filter(|p| !originals.contains(&p.cell())) {
        let new = next.placement(origin.cell()).unwrap();
        assert_eq!(
            (new.row(), new.column(), new.rowspan(), new.colspan()),
            (
                origin.row(),
                origin.column(),
                origin.rowspan(),
                origin.colspan()
            )
        );
        assert_eq!(after.node(origin.cell()), before.node(origin.cell()));
        assert!(
            before
                .store()
                .shares_node_payload(after.store(), origin.cell())
        );
    }
    for node in before.store().iter().filter(|node| {
        !originals.contains(&node.id()) && !matches!(node.kind(), NodeKind::TableRow)
    }) {
        assert_eq!(after.node(node.id()), Some(node));
    }
    round_trip(&before, &result);
}

#[test]
fn isolation_then_replacement_keeps_only_top_left_outside_rich_origins() {
    let (before, table, originals) = fixture(true);
    let (source, source_table, _) = fixture(false);
    let source_before = source.clone();
    let tree = TableTreeTemplate::capture(&source, source_table).unwrap();
    let rect = TableRect::new(1, 1, 4, 4).unwrap();
    let result = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::IsolateTableRect { table, rect })
        .with_step(TransactionStep::ReplaceTableRect { table, rect, tree })
        .apply_with_changes(&before)
        .unwrap();
    assert_eq!(
        originals
            .iter()
            .map(|cell| result.document().node(*cell).is_some())
            .collect::<Vec<_>>(),
        [true, true, false, false]
    );
    assert_eq!(
        result
            .document()
            .table_grid(table)
            .unwrap()
            .rect_between(
                result
                    .document()
                    .table_grid(table)
                    .unwrap()
                    .slot(1, 1)
                    .unwrap(),
                result
                    .document()
                    .table_grid(table)
                    .unwrap()
                    .slot(3, 3)
                    .unwrap()
            )
            .unwrap(),
        rect
    );
    for cell in &originals[..2] {
        assert_eq!(children(result.document(), *cell), children(&before, *cell));
    }
    assert_eq!(source.store(), source_before.store());
    assert_eq!(source.root(), source_before.root());
    assert_eq!(source.version(), source_before.version());
    assert_eq!(source.revision(), source_before.revision());
    assert_eq!(source.next_node_id(), source_before.next_node_id());
    round_trip(&before, &result);
}
