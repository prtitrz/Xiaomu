//! Shared task clipboard fixtures, with each canonical state kept exact.

use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

pub fn attrs(value: Option<AttrValue>) -> NodeAttrs {
    NodeAttrs::new(
        value
            .map(|value| ("checked".into(), value))
            .into_iter()
            .collect(),
    )
    .unwrap()
}

pub fn text(builder: &mut NodeStoreBuilder, kind: NodeKind, value: &str) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(value, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

pub fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: Vec<NodeId>) -> NodeId {
    builder
        .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

pub fn fixture(value: Option<AttrValue>) -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = text(&mut builder, NodeKind::Paragraph, "task中🙂");
    let code = text(&mut builder, NodeKind::CodeBlock, "code\n");
    let task = builder
        .insert(
            NodeKind::TaskItem,
            attrs(value),
            NodeContent::children([first, code]),
        )
        .unwrap();
    let list = container(&mut builder, NodeKind::TaskList, vec![task]);
    let tail = text(&mut builder, NodeKind::Paragraph, "tail");
    let root = container(&mut builder, NodeKind::Document, vec![list, tail]);
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        tail,
    )
}

pub fn end(document: &XiaomuDocument, node: NodeId) -> InlinePoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    let offset = inline.offset_at(inline.len_bytes()).unwrap();
    let ordinal = inline
        .atoms()
        .iter()
        .filter(|atom| atom.text_offset() == offset)
        .count();
    InlinePoint::new(node, offset, ordinal, CursorAffinity::After)
}

pub fn copy(
    document: &XiaomuDocument,
    first: NodeId,
    tail: NodeId,
    closed: bool,
) -> ClipboardSlice {
    let selection = if closed {
        DocumentSelection::all(document)
    } else {
        DocumentSelection::new(InlinePoint::at_start_of(first), end(document, tail))
    };
    DocumentSession::new(document.clone(), selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

pub fn roundtrip(slice: &ClipboardSlice) -> serde_json::Value {
    let metadata = encode_metadata(slice).unwrap();
    let wire: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(wire["version"], 12);
    assert_eq!(wire["closed"], slice.is_closed());
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(&decoded, slice);
    assert_eq!(encode_metadata(&decoded).unwrap(), metadata);
    assert!(decode_metadata("stale text", &metadata).is_none());
    wire
}
