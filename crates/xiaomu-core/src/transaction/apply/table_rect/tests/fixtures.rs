use super::*;
use crate::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeStoreBuilder, TextRun,
};

pub(super) fn attrs(entries: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

pub(super) fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
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

fn finish(mut builder: NodeStoreBuilder, table: NodeId) -> (XiaomuDocument, NodeId) {
    let outside = paragraph(&mut builder, "outside");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table, outside]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), table)
}

pub(super) fn unit_grid(rows: usize, columns: usize) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let mut row_ids = Vec::new();
    for row in 0..rows {
        let mut cells = Vec::new();
        for column in 0..columns {
            let p = paragraph(&mut builder, &format!("cell {row}/{column}"));
            cells.push(
                builder
                    .insert(
                        NodeKind::TableCell,
                        NodeAttrs::empty(),
                        NodeContent::children([p]),
                    )
                    .unwrap(),
            );
        }
        row_ids.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs(&[("target-row", AttrValue::Integer(row as i64))]),
                    NodeContent::children(cells),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("target-table", AttrValue::Null)]),
            NodeContent::children(row_ids),
        )
        .unwrap();
    finish(builder, table)
}

fn mixed(builder: &mut NodeStoreBuilder, text_mark: Mark, atom_mark: Mark) -> NodeId {
    let hard_break = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break().with_marks(MarkSet::new([atom_mark]).unwrap()),
            ),
        )
        .unwrap();
    let mention = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            attrs(&[("opaque", AttrValue::Null)]),
            NodeContent::InlineAtom(InlineAtomContent::new("@X").unwrap()),
        )
        .unwrap();
    builder
        .insert(
            NodeKind::Paragraph,
            attrs(&[("alignment", AttrValue::Null)]),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("A🙂中", MarkSet::new([text_mark]).unwrap()).unwrap()],
                    [
                        InlineAtomPlacement::new(hard_break, TextOffset::ZERO),
                        InlineAtomPlacement::new(mention, TextOffset::ZERO),
                    ],
                )
                .unwrap(),
            ),
        )
        .unwrap()
}

pub(super) fn rich_grid(
    rows: usize,
    columns: usize,
    nested_columns: usize,
) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let p = mixed(&mut builder, Mark::Italic, Mark::Bold);
    let quote_p = paragraph(&mut builder, "quoted");
    let quote = builder
        .insert(
            NodeKind::Quote,
            attrs(&[("quote", AttrValue::Null)]),
            NodeContent::children([quote_p]),
        )
        .unwrap();
    let item_p = paragraph(&mut builder, "listed");
    let item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([item_p]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::OrderedList,
            attrs(&[("start", AttrValue::Integer(7))]),
            NodeContent::children([item]),
        )
        .unwrap();
    let code = builder
        .insert(
            NodeKind::CodeBlock,
            attrs(&[("language", AttrValue::String("rust".into()))]),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("let a = 1;\n", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let image = builder
        .insert(
            NodeKind::Image,
            attrs(&[("asset", AttrValue::String("asset:sample".into()))]),
            NodeContent::Atomic,
        )
        .unwrap();
    let mut blocks = vec![p, quote, list, code, image];
    if nested_columns > 0 {
        let nested_p = mixed(&mut builder, Mark::Bold, Mark::Italic);
        let cell = builder
            .insert(
                NodeKind::TableCell,
                attrs(&[
                    ("colspan", AttrValue::Integer(nested_columns as i64)),
                    ("colwidth", AttrValue::Null),
                ]),
                NodeContent::children([nested_p]),
            )
            .unwrap();
        let row = builder
            .insert(
                NodeKind::TableRow,
                attrs(&[("nested-row", AttrValue::Null)]),
                NodeContent::children([cell]),
            )
            .unwrap();
        blocks.push(
            builder
                .insert(
                    NodeKind::Table,
                    attrs(&[("nested-table", AttrValue::Bool(true))]),
                    NodeContent::children([row]),
                )
                .unwrap(),
        );
    }
    let cell = builder
        .insert(
            NodeKind::TableHeader,
            attrs(&[
                ("rowspan", AttrValue::Integer(rows as i64)),
                ("colspan", AttrValue::Integer(columns as i64)),
                (
                    "colwidth",
                    AttrValue::List(
                        (0..columns)
                            .map(|column| AttrValue::Integer(if column == 0 { 90 } else { 0 }))
                            .collect(),
                    ),
                ),
                (
                    "opaque",
                    AttrValue::Object(BTreeMap::from([("nullable".into(), AttrValue::Null)])),
                ),
            ]),
            NodeContent::children(blocks),
        )
        .unwrap();
    let mut row_ids = Vec::new();
    for row in 0..rows {
        row_ids.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs(&[("source-row", AttrValue::Integer(row as i64))]),
                    NodeContent::children(if row == 0 { vec![cell] } else { vec![] }),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("source-table", AttrValue::String("fixed".into()))]),
            NodeContent::children(row_ids),
        )
        .unwrap();
    finish(builder, table)
}

pub(super) fn context(document: &XiaomuDocument) -> ApplyContext {
    ApplyContext {
        root: document.root(),
        store: document.store().clone(),
        next_node_id: document.next_node_id(),
    }
}

pub(super) fn budget_grid(
    outer_columns: usize,
    nested_columns: usize,
    other_columns: usize,
) -> (XiaomuDocument, NodeId) {
    fn table(builder: &mut NodeStoreBuilder, columns: usize, blocks: Vec<NodeId>) -> NodeId {
        let cell = builder
            .insert(
                NodeKind::TableCell,
                attrs(&[("colspan", AttrValue::Integer(columns as i64))]),
                NodeContent::children(blocks),
            )
            .unwrap();
        let row = builder
            .insert(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                NodeContent::children([cell]),
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
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "outer");
    let mut blocks = vec![p];
    if nested_columns > 0 {
        let p = paragraph(&mut builder, "nested");
        blocks.push(table(&mut builder, nested_columns, vec![p]));
    }
    let outer = table(&mut builder, outer_columns, blocks);
    let mut tables = vec![outer];
    if other_columns > 0 {
        let p = paragraph(&mut builder, "other");
        tables.push(table(&mut builder, other_columns, vec![p]));
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(tables),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), outer)
}

pub(super) fn apply(
    document: &XiaomuDocument,
    table: NodeId,
    rect: TableRect,
    tree: &TableTreeTemplate,
) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceTableRect {
            table,
            rect,
            tree: tree.clone(),
        })
        .apply_with_changes(document)
        .unwrap()
}

pub(super) fn round_trip(before: &XiaomuDocument, applied: &AppliedTransaction) {
    let undo = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undo.document().store(), before.store());
    let redo = undo.inverse().apply_with_changes(undo.document()).unwrap();
    assert_eq!(redo.document().store(), applied.document().store());
}

pub(super) fn compare_copy(
    source: &XiaomuDocument,
    old: NodeId,
    target: &XiaomuDocument,
    new: NodeId,
    pairs: &mut BTreeMap<NodeId, NodeId>,
) {
    assert!(pairs.insert(old, new).is_none());
    let a = source.node(old).unwrap();
    let b = target.node(new).unwrap();
    assert_eq!(a.kind(), b.kind());
    assert_eq!(a.attrs(), b.attrs());
    match (a.content(), b.content()) {
        (NodeContent::Children(a), NodeContent::Children(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                compare_copy(source, *a, target, *b, pairs);
            }
        }
        (NodeContent::Inline(a), NodeContent::Inline(b)) => {
            assert_eq!(a.runs(), b.runs());
            assert_eq!(a.atoms().len(), b.atoms().len());
            for (a, b) in a.atoms().iter().zip(b.atoms()) {
                assert_eq!(a.text_offset(), b.text_offset());
                compare_copy(source, a.atom(), target, b.atom(), pairs);
            }
        }
        (NodeContent::InlineAtom(a), NodeContent::InlineAtom(b)) => assert_eq!(a, b),
        (NodeContent::Atomic, NodeContent::Atomic) => {}
        shape => panic!("shape changed: {shape:?}"),
    }
}
