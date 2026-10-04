//! Node selections project one closed subtree without flattening its shell.

use xiaomu_core::document::{
    AtomKind, AttrValue, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark,
    MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::{
    ClipboardNode, ClipboardNodeContent, decode_metadata, encode_metadata,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, SessionError};

fn rich_attrs() -> NodeAttrs {
    NodeAttrs::new(
        [
            ("null".into(), AttrValue::Null),
            ("flag".into(), AttrValue::Bool(false)),
            ("start".into(), AttrValue::Integer(4)),
            (
                "src".into(),
                AttrValue::String("https://example.invalid/image.png".into()),
            ),
            (
                "detail".into(),
                AttrValue::Object(
                    [(
                        "values".into(),
                        AttrValue::List(vec![AttrValue::Null, AttrValue::String("中🙂".into())]),
                    )]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
    .unwrap()
}

fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: &[NodeId]) -> NodeId {
    builder
        .insert(
            kind,
            rich_attrs(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap()
}

fn text(builder: &mut NodeStoreBuilder, kind: NodeKind, value: &str) -> NodeId {
    let content = if value.is_empty() {
        NodeContent::empty_inline()
    } else {
        NodeContent::Inline(
            InlineContent::new([TextRun::new(value, MarkSet::new([Mark::Bold]).unwrap()).unwrap()])
                .unwrap(),
        )
    };
    builder.insert(kind, rich_attrs(), content).unwrap()
}

fn fixture(with_tasks: bool) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let atom_marks = MarkSet::new([Mark::Italic, Mark::Underline]).unwrap();
    let hard_break = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break().with_marks(atom_marks.clone())),
        )
        .unwrap();
    let mention = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            rich_attrs(),
            NodeContent::InlineAtom(
                InlineAtomContent::new("@完整")
                    .unwrap()
                    .with_marks(atom_marks),
            ),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            rich_attrs(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("中🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [
                        InlineAtomPlacement::new(hard_break, TextOffset::ZERO),
                        InlineAtomPlacement::new(mention, TextOffset::ZERO),
                    ],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let code = text(&mut builder, NodeKind::CodeBlock, "let a = 1;\n尾");
    let image = builder
        .insert(NodeKind::Image, rich_attrs(), NodeContent::Atomic)
        .unwrap();
    let rule = builder
        .insert(NodeKind::HorizontalRule, rich_attrs(), NodeContent::Atomic)
        .unwrap();
    let heading = text(
        &mut builder,
        NodeKind::Heading(HeadingLevel::new(3).unwrap()),
        "heading",
    );
    let nested_quote = container(&mut builder, NodeKind::Quote, &[code, image, rule]);
    let nested_text = text(&mut builder, NodeKind::Paragraph, "nested");
    let mut selected = vec![
        paragraph,
        code,
        image,
        rule,
        heading,
        nested_quote,
        nested_text,
    ];
    let nested = if with_tasks {
        let item = builder
            .insert(
                NodeKind::TaskItem,
                NodeAttrs::new([("checked".into(), AttrValue::Null)].into()).unwrap(),
                NodeContent::children([nested_text]),
            )
            .unwrap();
        let list = container(&mut builder, NodeKind::TaskList, &[item]);
        selected.push(list);
        list
    } else {
        nested_text
    };
    let ordered_item = container(&mut builder, NodeKind::ListItem, &[nested]);
    let ordered = container(&mut builder, NodeKind::OrderedList, &[ordered_item]);
    let bullet_item = container(&mut builder, NodeKind::ListItem, &[ordered]);
    let bullet = container(&mut builder, NodeKind::BulletList, &[bullet_item]);
    let empty_quote = container(&mut builder, NodeKind::Quote, &[]);
    let outer = container(
        &mut builder,
        NodeKind::Quote,
        &[paragraph, heading, nested_quote, bullet, empty_quote],
    );
    selected.extend([ordered, bullet, empty_quote, outer]);
    let root = container(&mut builder, NodeKind::Document, &[outer]);
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        selected,
    )
}

fn assert_fragment(document: &XiaomuDocument, id: NodeId, fragment: &ClipboardNode) {
    let node = document.node(id).unwrap();
    assert_eq!(fragment.kind(), node.kind());
    assert_eq!(fragment.attrs(), node.attrs());
    match (node.content(), fragment.content()) {
        (NodeContent::Inline(source), ClipboardNodeContent::Inline(copied)) => {
            assert_eq!(copied.runs(), source.runs());
            assert_eq!(copied.atoms().len(), source.atoms().len());
            for (atom, placement) in copied.atoms().iter().zip(source.atoms()) {
                let payload = document.node(placement.atom()).unwrap();
                let NodeKind::InlineAtom(kind) = payload.kind() else {
                    panic!("inline atom")
                };
                assert_eq!(atom.kind(), kind);
                assert_eq!(atom.anchor(), placement.text_offset());
                assert_eq!(atom.attrs(), payload.attrs());
                assert_eq!(atom.content(), payload.content().as_inline_atom().unwrap());
            }
        }
        (NodeContent::Children(source), ClipboardNodeContent::Children(copied)) => {
            assert_eq!(copied.len(), source.len());
            for (child, fragment) in source.iter().zip(copied) {
                assert_fragment(document, *child, fragment);
            }
        }
        (NodeContent::Children(source), ClipboardNodeContent::Table { rows, row_attrs }) => {
            assert_eq!(rows.len(), source.len());
            assert_eq!(row_attrs.len(), source.len());
            for ((row, copied_cells), attrs) in source.iter().zip(rows).zip(row_attrs) {
                let row = document.node(*row).unwrap();
                assert_eq!(attrs, row.attrs());
                let cells = row.content().as_children().unwrap();
                assert_eq!(cells.len(), copied_cells.len());
                for (cell, copied) in cells.iter().zip(copied_cells) {
                    assert_fragment(document, *cell, copied);
                }
            }
        }
        (NodeContent::Atomic, ClipboardNodeContent::Atomic) => {}
        _ => panic!("whole-subtree projection changed the content shape"),
    }
}

#[test]
fn every_supported_nested_block_copies_exactly_one_closed_subtree() {
    for with_tasks in [false, true] {
        let (document, nodes) = fixture(with_tasks);
        for selected in nodes {
            let selection = DocumentSelection::node(&document, selected).unwrap();
            let session = DocumentSession::new(document.clone(), selection).unwrap();
            let slice = session.clipboard_slice().unwrap().unwrap();
            assert!(slice.is_closed());
            assert_eq!(slice.roots().len(), 1);
            assert_fragment(&document, selected, &slice.roots()[0]);
            let metadata = encode_metadata(&slice).unwrap();
            let wire: serde_json::Value = serde_json::from_str(&metadata).unwrap();
            let carries_tasks = metadata.contains("\"type\":\"task_list\"");
            assert_eq!(wire["version"], if carries_tasks { 12 } else { 11 });
            assert_eq!(wire["closed"], true);
            assert_eq!(
                decode_metadata(slice.plain_text(), &metadata),
                Some(slice.clone())
            );
            assert!(!metadata.contains("node_id"));
            assert!(!metadata.contains("node_selection"));
            assert_eq!(session.selection(), selection);
            assert_eq!(session.document().store(), document.store());
            assert_eq!(session.document().revision(), document.revision());
            assert_eq!(session.history_depths(), (0, 0));
            assert_eq!(session.stored_marks(), None);
        }
    }
}

#[test]
fn selecting_a_whole_table_preserves_rows_cells_and_all_nested_blocks() {
    let mut builder = NodeStoreBuilder::new();
    let code = text(&mut builder, NodeKind::CodeBlock, "a\nb");
    let empty = text(&mut builder, NodeKind::Paragraph, "");
    let quote = container(&mut builder, NodeKind::Quote, &[code, empty]);
    let image = builder
        .insert(NodeKind::Image, rich_attrs(), NodeContent::Atomic)
        .unwrap();
    let first = container(&mut builder, NodeKind::TableCell, &[quote]);
    let second = container(&mut builder, NodeKind::TableCell, &[image]);
    let row = container(&mut builder, NodeKind::TableRow, &[first, second]);
    let table = container(&mut builder, NodeKind::Table, &[row]);
    let root = container(&mut builder, NodeKind::Document, &[table]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let session = DocumentSession::new(
        document.clone(),
        DocumentSelection::node(&document, table).unwrap(),
    )
    .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert!(slice.is_closed());
    assert_eq!(slice.roots().len(), 1);
    assert_fragment(&document, table, &slice.roots()[0]);
    let metadata = encode_metadata(&slice).unwrap();
    assert!(metadata.contains("\"version\":11"));
    assert_eq!(decode_metadata(slice.plain_text(), &metadata), Some(slice));
}

#[test]
fn ordinary_text_ranges_keep_open_fragment_semantics_and_existing_wire_versions() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("abc", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let quote = container(&mut builder, NodeKind::Quote, &[paragraph]);
    let root = container(&mut builder, NodeKind::Document, &[quote]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let end = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(3)
        .unwrap();
    let text = DocumentSelection::new(
        InlinePoint::at_start_of(paragraph),
        InlinePoint::new(
            paragraph,
            end,
            0,
            xiaomu_core::selection::CursorAffinity::Before,
        ),
    );
    let session = DocumentSession::new(document, text).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert!(!slice.is_closed());
    assert_eq!(slice.roots()[0].kind(), &NodeKind::Paragraph);
    let metadata = encode_metadata(&slice).unwrap();
    assert!(metadata.contains("\"version\":4"));
    assert!(!metadata.contains("\"closed\""));
    assert_eq!(decode_metadata("abc", &metadata), Some(slice));
}

#[test]
fn unsupported_leaf_inside_selected_container_rejects_the_whole_copy() {
    let mut builder = NodeStoreBuilder::new();
    let unknown = builder
        .insert(
            NodeKind::custom("unknown-leaf").unwrap(),
            rich_attrs(),
            NodeContent::InlineAtom(InlineAtomContent::new("opaque").unwrap()),
        )
        .unwrap();
    let quote = container(&mut builder, NodeKind::Quote, &[unknown]);
    let root = container(&mut builder, NodeKind::Document, &[quote]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::node(&document, quote).unwrap();
    let session = DocumentSession::new(document.clone(), selection).unwrap();
    assert_eq!(
        session.clipboard_slice(),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.history_depths(), (0, 0));
}
