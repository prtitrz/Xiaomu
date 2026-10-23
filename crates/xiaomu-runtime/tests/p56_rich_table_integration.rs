//! P5.6 rich table clipboard matrix: Unicode, marks, atoms, containers,
//! nested tables, media, attributes, fresh identity and reversible history.
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection, DocumentSession, EditIntent};

fn attrs(label: &str) -> NodeAttrs {
    NodeAttrs::new([("data-fixture".into(), AttrValue::String(label.into()))].into()).unwrap()
}
fn children(doc: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    doc.node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}
fn container(b: &mut NodeStoreBuilder, kind: NodeKind, label: &str, nodes: &[NodeId]) -> NodeId {
    b.insert(kind, attrs(label), NodeContent::children(nodes.to_vec()))
        .unwrap()
}
fn paragraph(b: &mut NodeStoreBuilder, text: &str) -> NodeId {
    b.insert(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    )
    .unwrap()
}
fn table(b: &mut NodeStoreBuilder, label: &str, blocks: &[NodeId]) -> (NodeId, NodeId) {
    let cell = container(b, NodeKind::TableCell, &format!("{label}-cell"), blocks);
    let row = container(b, NodeKind::TableRow, &format!("{label}-row"), &[cell]);
    (container(b, NodeKind::Table, label, &[row]), cell)
}
fn fixture() -> (XiaomuDocument, NodeId, NodeId, NodeId, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let intro = paragraph(&mut b, "intro");
    let mut atoms = Vec::new();
    for text in ["@一", "@二", "@末"] {
        atoms.push(
            b.insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                attrs(text),
                NodeContent::InlineAtom(InlineAtomContent::new(text).unwrap()),
            )
            .unwrap(),
        );
    }
    let text = "中文👩‍💻é אבג\n第二行\t结束";
    let inline = InlineContent::with_atoms(
        [TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
        [
            InlineAtomPlacement::new(atoms[0], TextOffset::ZERO),
            InlineAtomPlacement::new(atoms[1], TextOffset::ZERO),
            InlineAtomPlacement::new(
                atoms[2],
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()])
                    .unwrap()
                    .offset_at(text.len())
                    .unwrap(),
            ),
        ],
    )
    .unwrap();
    let rich = b
        .insert(
            NodeKind::Paragraph,
            attrs("rich"),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let quote = container(&mut b, NodeKind::Quote, "quote", &[rich]);
    let list_text = paragraph(&mut b, "列表");
    let item = container(&mut b, NodeKind::ListItem, "item", &[list_text]);
    let list = container(&mut b, NodeKind::BulletList, "list", &[item]);
    let code = b
        .insert(
            NodeKind::CodeBlock,
            attrs("code"),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("a\r\nb\rc\td", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let image = b
        .insert(
            NodeKind::Image,
            NodeAttrs::new(
                [
                    (
                        "src".into(),
                        AttrValue::String("https://example.com/p56.png".into()),
                    ),
                    ("alt".into(), AttrValue::String("图片".into())),
                    ("custom".into(), AttrValue::Bool(true)),
                ]
                .into(),
            )
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let rule = b
        .insert(NodeKind::HorizontalRule, attrs("rule"), NodeContent::Atomic)
        .unwrap();
    let nested_text = paragraph(&mut b, "内表");
    let (nested, _) = table(&mut b, "nested", &[nested_text]);
    let (source, source_cell) = table(&mut b, "source", &[quote, list, code, image, rule, nested]);
    let destination_text = paragraph(&mut b, "target");
    let (destination, destination_cell) = table(&mut b, "destination", &[destination_text]);
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, source, destination]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, b.finish()).unwrap(),
        intro,
        source_cell,
        destination_cell,
        destination_text,
    )
}
fn session(doc: &XiaomuDocument, text: NodeId) -> DocumentSession {
    DocumentSession::new(
        doc.clone(),
        DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::at_start_of(text))),
    )
    .unwrap()
}
fn copy(session: &mut DocumentSession, cell: NodeId) -> ClipboardSlice {
    session.set_cell_range_selection(cell, cell).unwrap();
    session.clipboard_slice().unwrap().unwrap()
}
fn node_ids(doc: &XiaomuDocument, node: NodeId, ids: &mut Vec<NodeId>) {
    ids.push(node);
    match doc.node(node).unwrap().content() {
        NodeContent::Children(children) => {
            for child in children {
                node_ids(doc, *child, ids);
            }
        }
        NodeContent::Inline(inline) => {
            for atom in inline.atoms() {
                ids.push(atom.atom());
            }
        }
        _ => {}
    }
}

#[test]
fn rich_cell_wire_and_sibling_table_round_trip_preserves_every_semantic_and_fresh_ids() {
    let (doc, intro, source_cell, _, _) = fixture();
    let mut s = session(&doc, intro);
    let slice = copy(&mut s, source_cell);
    assert_eq!(slice.plain_text().lines().count(), 1);
    assert!(!slice.plain_text().contains(['\t', '\r']));
    assert!(
        slice
            .plain_text()
            .contains("@一@二中文👩‍💻é אבג 第二行 结束@末")
    );
    assert!(slice.plain_text().contains("https://example.com/p56.png"));
    let wire = encode_metadata(&slice).unwrap();
    assert_eq!(decode_metadata(slice.plain_text(), &wire).unwrap(), slice);
    s.set_inline_selection(
        InlinePoint::at_start_of(intro),
        InlinePoint::at_start_of(intro),
    )
    .unwrap();
    let before_selection = s.selection();
    s.apply_intent(&EditIntent::PasteSlice {
        slice: slice.clone(),
    })
    .unwrap();
    let pasted = children(s.document(), s.document().root())[1];
    let pasted_row = children(s.document(), pasted)[0];
    let pasted_cell = children(s.document(), pasted_row)[0];
    assert_eq!(copy(&mut s, pasted_cell), slice);
    let mut source_ids = Vec::new();
    let mut pasted_ids = Vec::new();
    node_ids(&doc, source_cell, &mut source_ids);
    node_ids(s.document(), pasted_cell, &mut pasted_ids);
    assert!(source_ids.iter().all(|id| !pasted_ids.contains(id)));
    assert_eq!(s.history_depths(), (1, 0));
    s.undo().unwrap();
    assert_eq!(s.document().store(), doc.store());
    assert_eq!(s.selection(), before_selection);
    s.redo().unwrap();
    assert_eq!(copy(&mut s, pasted_cell), slice);
}

#[test]
fn rich_range_replacement_preserves_target_cell_attrs_and_exact_undo_redo() {
    let (doc, intro, source_cell, destination_cell, _) = fixture();
    let mut s = session(&doc, intro);
    let slice = copy(&mut s, source_cell);
    s.set_cell_range_selection(destination_cell, destination_cell)
        .unwrap();
    let selection = s.selection();
    s.apply_intent(&EditIntent::PasteSlice {
        slice: slice.clone(),
    })
    .unwrap();
    assert_eq!(
        s.document().node(destination_cell).unwrap().attrs(),
        doc.node(destination_cell).unwrap().attrs()
    );
    let copied = copy(&mut s, destination_cell);
    assert_eq!(copied.plain_text(), slice.plain_text());
    assert_eq!(
        copied.roots()[0].content().as_table().unwrap()[0][0].content(),
        slice.roots()[0].content().as_table().unwrap()[0][0].content()
    );
    s.undo().unwrap();
    assert_eq!(s.document().store(), doc.store());
    assert_eq!(s.selection(), selection);
    s.redo().unwrap();
    assert_eq!(copy(&mut s, destination_cell), copied);
}

#[test]
fn rich_single_cell_paste_and_clear_are_isolated_history_units() {
    let (doc, intro, source_cell, destination_cell, destination_text) = fixture();
    let mut s = session(&doc, intro);
    let slice = copy(&mut s, source_cell);
    s.set_inline_selection(
        InlinePoint::at_start_of(destination_text),
        InlinePoint::at_start_of(destination_text),
    )
    .unwrap();
    s.apply_intent(&EditIntent::PasteSlice { slice }).unwrap();
    let blocks = children(s.document(), destination_cell);
    assert_eq!(blocks.len(), 7);
    assert_eq!(blocks[0], destination_text);
    let pasted_doc = s.document().clone();
    let copied = copy(&mut s, destination_cell);
    s.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(s.history_depths(), (2, 0));
    assert_eq!(copy(&mut s, destination_cell).plain_text(), "");
    s.undo().unwrap();
    assert_eq!(s.document().store(), pasted_doc.store());
    assert_eq!(copy(&mut s, destination_cell), copied);
    s.undo().unwrap();
    assert_eq!(s.document().store(), doc.store());
}
