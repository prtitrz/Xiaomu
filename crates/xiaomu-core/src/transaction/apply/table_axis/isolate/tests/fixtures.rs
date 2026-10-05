use super::*;
use crate::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeStoreBuilder, TextRun,
};
use crate::text::TextOffset;

pub(super) fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    )
    .unwrap()
}

fn insert(builder: &mut NodeStoreBuilder, kind: NodeKind, content: NodeContent) -> NodeId {
    builder.insert(kind, NodeAttrs::empty(), content).unwrap()
}

pub(super) fn rich(builder: &mut NodeStoreBuilder, label: &str) -> Vec<NodeId> {
    let atom = insert(
        builder,
        NodeKind::InlineAtom(AtomKind::hard_break()),
        NodeContent::InlineAtom(
            InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Italic]).unwrap()),
        ),
    );
    let p = insert(
        builder,
        NodeKind::Paragraph,
        NodeContent::Inline(
            InlineContent::with_atoms(
                [TextRun::new(label, MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
            )
            .unwrap(),
        ),
    );
    let heading = insert(
        builder,
        NodeKind::Heading(HeadingLevel::new(2).unwrap()),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(label, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    );
    let code = insert(
        builder,
        NodeKind::CodeBlock,
        NodeContent::Inline(
            InlineContent::new([TextRun::new("code🙂", MarkSet::empty()).unwrap()]).unwrap(),
        ),
    );
    let quote = insert(builder, NodeKind::Quote, NodeContent::children([code]));
    let list_p = insert(builder, NodeKind::Paragraph, NodeContent::empty_inline());
    let item = insert(builder, NodeKind::ListItem, NodeContent::children([list_p]));
    let list = insert(builder, NodeKind::BulletList, NodeContent::children([item]));
    let nested_p = insert(builder, NodeKind::Paragraph, NodeContent::empty_inline());
    let nested_c = insert(
        builder,
        NodeKind::TableCell,
        NodeContent::children([nested_p]),
    );
    let nested_r = insert(
        builder,
        NodeKind::TableRow,
        NodeContent::children([nested_c]),
    );
    let nested = insert(builder, NodeKind::Table, NodeContent::children([nested_r]));
    vec![p, heading, quote, list, nested]
}

pub(super) fn span_fixture(
    rows: usize,
    columns: usize,
    kind: NodeKind,
    extra: NodeAttrs,
) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let blocks = rich(&mut builder, "original 中🙂");
    let mut values: BTreeMap<_, _> = extra
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    if rows != 1 {
        values.insert("rowspan".into(), AttrValue::Integer(rows as i64));
    }
    if columns != 1 {
        values.insert("colspan".into(), AttrValue::Integer(columns as i64));
    }
    let cell = builder
        .insert(
            kind,
            NodeAttrs::new(values).unwrap(),
            NodeContent::children(blocks),
        )
        .unwrap();
    let mut row_ids = Vec::new();
    for index in 0..rows {
        let row_attrs = if index % 2 == 0 {
            NodeAttrs::empty()
        } else {
            attrs(&[
                ("preserve_empty_content", AttrValue::Bool(true)),
                ("raw", AttrValue::List(vec![])),
            ])
        };
        row_ids.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    row_attrs,
                    NodeContent::children(if index == 0 { vec![cell] } else { vec![] }),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            attrs(&[("table-null", AttrValue::Null)]),
            NodeContent::children(row_ids),
        )
        .unwrap();
    let root = insert(
        &mut builder,
        NodeKind::Document,
        NodeContent::children([table]),
    );
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        cell,
    )
}

pub(super) fn context(doc: &XiaomuDocument) -> ApplyContext {
    ApplyContext {
        root: doc.root(),
        store: doc.store().clone(),
        next_node_id: doc.next_node_id(),
    }
}

pub(super) fn apply(doc: &XiaomuDocument, table: NodeId, rect: TableRect) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::IsolateTableRect { table, rect })
        .apply_with_changes(doc)
        .unwrap()
}

pub(super) fn children(doc: &XiaomuDocument, id: NodeId) -> &[NodeId] {
    doc.node(id).unwrap().content().as_children().unwrap()
}

pub(super) fn round_trip(doc: &XiaomuDocument, applied: &AppliedTransaction) {
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), doc.store());
    assert_eq!(undone.document().root(), doc.root());
    assert_eq!(
        undone.document().next_node_id(),
        applied.document().next_node_id()
    );
    let redone = undone
        .inverse()
        .apply_with_changes(undone.document())
        .unwrap();
    assert_eq!(redone.document().store(), applied.document().store());
    assert_eq!(
        redone.document().next_node_id(),
        applied.document().next_node_id()
    );
}

pub(super) fn expected_restore(applied: &AppliedTransaction) -> &TableCellRestore {
    let TransactionStep::RestoreTableCells { restore } = &applied.inverse().steps()[0] else {
        panic!("exact inverse")
    };
    restore
}
