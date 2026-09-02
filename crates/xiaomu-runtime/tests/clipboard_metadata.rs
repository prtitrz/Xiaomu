//! P3.3 versioned structured clipboard metadata regressions.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    AttrValue, HeadingLevel, InlineContent, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs,
    NodeContent, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::{ClipboardNodeContent, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

fn point(document: &XiaomuDocument, node: xiaomu_core::document::NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

#[test]
fn metadata_round_trip_preserves_kind_attrs_runs_and_marks() {
    let mut attrs = BTreeMap::new();
    attrs.insert(
        "level-note".to_owned(),
        AttrValue::String("保留".to_owned()),
    );
    attrs.insert("flag".to_owned(), AttrValue::Bool(true));
    let attrs = NodeAttrs::new(attrs).unwrap();

    let mut builder = NodeStoreBuilder::new();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(3).unwrap()),
            attrs,
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("前", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                    TextRun::new(
                        "链接",
                        MarkSet::new([Mark::Link(LinkMark::new(
                            "https://example.invalid/x",
                            Some("title".to_owned()),
                        ))])
                        .unwrap(),
                    )
                    .unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let inline = document
        .node(heading)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let selection = DocumentSelection::new(
        TextPoint::new(
            heading,
            inline.offset_at(0).unwrap(),
            CursorAffinity::Before,
        ),
        TextPoint::new(
            heading,
            inline.offset_at(inline.len_bytes()).unwrap(),
            CursorAffinity::Before,
        ),
    );
    let session = DocumentSession::new(document, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();

    let metadata = encode_metadata(&slice).unwrap();
    let decoded = decode_metadata(slice.plain_text(), &metadata).expect("xiaomu metadata");

    assert_eq!(decoded, slice);
    assert!(matches!(decoded.blocks()[0].kind(), NodeKind::Heading(level) if level.as_u8() == 3));
    assert_eq!(
        decoded.blocks()[0].attrs().get("level-note"),
        Some(&AttrValue::String("保留".to_owned()))
    );
    let runs = decoded.blocks()[0].inline().runs();
    assert!(runs[0].marks().contains(MarkKind::Bold));
    assert!(runs[1].marks().contains(MarkKind::Link));
}

#[test]
fn metadata_round_trip_preserves_minimal_list_fragment_tree() {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("甲乙", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let first_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("尾巴", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let second_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([second]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([first_item, second_item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(
        point(&document, first, "甲".len()),
        point(&document, second, "尾".len()),
    );
    let session = DocumentSession::new(document, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();

    assert_eq!(slice.plain_text(), "乙\n尾");
    let metadata = encode_metadata(&slice).unwrap();
    let decoded = decode_metadata(slice.plain_text(), &metadata).expect("hierarchical metadata");
    assert_eq!(decoded, slice);

    assert_eq!(decoded.roots().len(), 1);
    assert!(matches!(decoded.roots()[0].kind(), NodeKind::BulletList));
    let ClipboardNodeContent::Children(items) = decoded.roots()[0].content() else {
        panic!("list fragment must keep item children");
    };
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| matches!(item.kind(), NodeKind::ListItem))
    );
}

#[test]
fn foreign_stale_and_unknown_metadata_fall_back_instead_of_parsing() {
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
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let inline = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let selection = DocumentSelection::new(
        TextPoint::new(
            paragraph,
            inline.offset_at(0).unwrap(),
            CursorAffinity::Before,
        ),
        TextPoint::new(
            paragraph,
            inline.offset_at(3).unwrap(),
            CursorAffinity::Before,
        ),
    );
    let session = DocumentSession::new(document, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    let metadata = encode_metadata(&slice).unwrap();

    assert!(decode_metadata("changed", &metadata).is_none());
    assert!(decode_metadata("abc", "{\"format\":\"other\"}").is_none());
    assert!(decode_metadata("abc", "not-json").is_none());

    let unknown_version = metadata.replacen("\"version\":3", "\"version\":999", 1);
    assert!(decode_metadata("abc", &unknown_version).is_none());
}
