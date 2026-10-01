//! Table clipboard round trips must preserve every legal cell subtree.
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn attrs(tag: &str) -> NodeAttrs {
    NodeAttrs::new([("extension-tag".into(), AttrValue::String(tag.into()))].into()).unwrap()
}

fn children(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn block(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    tag: &str,
    content: NodeContent,
) -> NodeId {
    builder.insert(kind, attrs(tag), content).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    block(
        builder,
        NodeKind::Paragraph,
        text,
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()])
                .unwrap(),
        ),
    )
}

fn one_cell_table(
    builder: &mut NodeStoreBuilder,
    blocks: &[NodeId],
    tag: &str,
) -> (NodeId, NodeId) {
    let cell = block(
        builder,
        NodeKind::TableCell,
        &format!("{tag}-cell"),
        NodeContent::children(blocks.to_vec()),
    );
    let row = block(
        builder,
        NodeKind::TableRow,
        &format!("{tag}-row"),
        NodeContent::children([cell]),
    );
    (
        block(builder, NodeKind::Table, tag, NodeContent::children([row])),
        cell,
    )
}

fn rich_fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let quoted_text = paragraph(&mut builder, "中🙂\t文\nnext");
    let quote = block(
        &mut builder,
        NodeKind::Quote,
        "quote",
        NodeContent::children([quoted_text]),
    );
    let list_text = paragraph(&mut builder, "list");
    let item = block(
        &mut builder,
        NodeKind::ListItem,
        "item",
        NodeContent::children([list_text]),
    );
    let list = block(
        &mut builder,
        NodeKind::BulletList,
        "list",
        NodeContent::children([item]),
    );
    let hr = block(
        &mut builder,
        NodeKind::HorizontalRule,
        "rule",
        NodeContent::Atomic,
    );
    let inner_text = paragraph(&mut builder, "nested");
    let (nested, _) = one_cell_table(&mut builder, &[inner_text], "nested");
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new(
                [
                    (
                        "src".into(),
                        AttrValue::String("https://example.test/table.png".into()),
                    ),
                    ("alt".into(), AttrValue::String("图🙂".into())),
                    (
                        "extension-tag".into(),
                        AttrValue::String("image-extension".into()),
                    ),
                ]
                .into(),
            )
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let (table, cell) = one_cell_table(&mut builder, &[quote, list, hr, nested, image], "outer");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let zero = document
        .node(quoted_text)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(0)
        .unwrap();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(quoted_text, zero, 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: attrs("atom"),
            content: InlineAtomContent::new("@first").unwrap(),
        })
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(quoted_text, zero, 1, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: attrs("atom-two"),
            content: InlineAtomContent::new("@second").unwrap(),
        })
        .apply(&document)
        .unwrap();
    (document, intro, cell)
}

fn session(document: &XiaomuDocument, node: NodeId) -> DocumentSession {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(0)
        .unwrap();
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::new(node, offset, 0, CursorAffinity::Before)),
    )
    .unwrap()
}

fn copy_cell(session: &mut DocumentSession, cell: NodeId) -> ClipboardSlice {
    session.set_cell_range_selection(cell, cell).unwrap();
    session.clipboard_slice().unwrap().unwrap()
}

#[test]
fn rich_table_wire_and_sibling_paste_preserve_all_levels_with_exact_undo() {
    let (document, intro, cell) = rich_fixture();
    let slice = copy_cell(&mut session(&document, intro), cell);
    // Embedded tabs/newlines must not create extra TSV rows or columns.
    assert!(!slice.plain_text().contains(['\t', '\n', '\r']));
    assert!(
        slice
            .plain_text()
            .contains("https://example.test/table.png")
    );
    let metadata = encode_metadata(&slice).unwrap();
    let wire: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(wire["version"], 6);
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(decoded, slice);
    let mut lower_version = wire.clone();
    lower_version["version"] = 5.into();
    assert!(decode_metadata(slice.plain_text(), &lower_version.to_string()).is_none());

    let mut target = session(&document, intro);
    let selection_before = target.selection();
    target
        .apply_intent(&EditIntent::PasteSlice { slice: decoded })
        .unwrap();
    let pasted = target.document().clone();
    let table = children(&pasted, pasted.root())[1];
    let row = children(&pasted, table)[0];
    let pasted_cell = children(&pasted, row)[0];
    assert_ne!(pasted_cell, cell);
    assert_eq!(target.history_depths(), (1, 0));
    assert_eq!(copy_cell(&mut target, pasted_cell), slice);
    target.undo().unwrap();
    assert_eq!(target.document().store(), document.store());
    assert_eq!(target.selection(), selection_before);
    target.redo().unwrap();
    assert_eq!(target.document().store(), pasted.store());
}

#[test]
fn rich_range_replacement_keeps_target_row_and_table_attrs() {
    let (document, intro, source_cell) = rich_fixture();
    let slice = copy_cell(&mut session(&document, intro), source_cell);
    let inserted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTable {
            parent: document.root(),
            index: 2,
            rows: 1,
            columns: 1,
        })
        .apply(&document)
        .unwrap();
    let target_table = children(&inserted, inserted.root())[2];
    let target_row = children(&inserted, target_table)[0];
    let target_cell = children(&inserted, target_row)[0];
    let mut target = session(&inserted, intro);
    target
        .set_cell_range_selection(target_cell, target_cell)
        .unwrap();
    let before_selection = target.selection();
    target
        .apply_intent(&EditIntent::PasteSlice {
            slice: slice.clone(),
        })
        .unwrap();
    assert_eq!(target.selection(), before_selection);
    assert!(
        target
            .document()
            .node(target_table)
            .unwrap()
            .attrs()
            .is_empty()
    );
    assert!(
        target
            .document()
            .node(target_row)
            .unwrap()
            .attrs()
            .is_empty()
    );
    let result = target.clipboard_slice().unwrap().unwrap();
    assert_eq!(
        result.roots()[0].content().as_table(),
        slice.roots()[0].content().as_table()
    );
    let after = target.document().clone();
    target.undo().unwrap();
    assert_eq!(target.document().store(), inserted.store());
    assert_eq!(target.selection(), before_selection);
    target.redo().unwrap();
    assert_eq!(target.document().store(), after.store());
}

#[test]
fn single_cell_paste_from_nested_quote_caret_keeps_complete_payload() {
    let (document, intro, cell) = rich_fixture();
    let slice = copy_cell(&mut session(&document, intro), cell);
    let quote = children(&document, cell)[0];
    let text = children(&document, quote)[0];
    let mut target = session(&document, text);
    target
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    let blocks = children(target.document(), cell);
    assert_eq!(blocks.len(), 10);
    assert_eq!(blocks[0], quote);
    assert!(matches!(
        target.document().node(blocks[1]).unwrap().kind(),
        NodeKind::Quote
    ));
    assert!(matches!(
        target.document().node(blocks[4]).unwrap().kind(),
        NodeKind::Table
    ));
    target.undo().unwrap();
    assert_eq!(target.document().store(), document.store());
}

#[test]
fn non_table_fragment_fills_range_without_losing_container_content() {
    let (document, intro, cell) = rich_fixture();
    let mut source = session(&document, intro);
    let start = source.selection().focus();
    let end = InlinePoint::new(
        intro,
        document
            .node(intro)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(5)
            .unwrap(),
        0,
        CursorAffinity::Before,
    );
    source = DocumentSession::new(document.clone(), DocumentSelection::new(start, end)).unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();
    let mut target = session(&document, intro);
    target.set_cell_range_selection(cell, cell).unwrap();
    target
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    assert_eq!(
        target.clipboard_slice().unwrap().unwrap().plain_text(),
        "intro"
    );
    assert_eq!(
        target.document().node(cell).unwrap().attrs(),
        &attrs("outer-cell")
    );
    target.undo().unwrap();
    assert_eq!(target.document().store(), document.store());
}
