//! Conditional v8 links preserve every attribute without weakening strict input.

use serde_json::{Value, json};
use xiaomu_core::document::{
    AttrValue, InlineContent, LinkAttributes, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, StringAttribute, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextRange;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn rich_link() -> LinkMark {
    LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href("https://example.test/路径".into())
            .with_target(StringAttribute::Null)
            .with_rel("noopener noreferrer".into())
            .with_class("".into())
            .with_title("标题🙂".into()),
    )
}

fn run(link: LinkMark) -> TextRun {
    TextRun::new(
        "中🙂",
        MarkSet::new([Mark::Bold, Mark::Link(link)]).unwrap(),
    )
    .unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    TextPoint::new(
        node,
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(raw)
            .unwrap(),
        CursorAffinity::Before,
    )
}

fn document(runs: Vec<TextRun>, attrs: NodeAttrs) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            attrs,
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

fn copy(document: XiaomuDocument, node: NodeId) -> ClipboardSlice {
    let len = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .len_bytes();
    let selection = DocumentSelection::new(point(&document, node, 0), point(&document, node, len));
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn slice(link: LinkMark) -> ClipboardSlice {
    let (doc, node) = document(vec![run(link)], NodeAttrs::empty());
    copy(doc, node)
}

fn wire(slice: &ClipboardSlice) -> Value {
    serde_json::from_str(&encode_metadata(slice).unwrap()).unwrap()
}

fn assert_round_trip(slice: &ClipboardSlice, version: u32) -> Value {
    let wire = wire(slice);
    assert_eq!(wire["version"], version);
    let decoded = decode_metadata(slice.plain_text(), &wire.to_string()).unwrap();
    assert_eq!(&decoded, slice);
    assert_eq!(
        encode_metadata(&decoded).unwrap(),
        encode_metadata(slice).unwrap()
    );
    assert!(decode_metadata("stale", &wire.to_string()).is_none());
    wire
}

#[test]
fn five_independent_tri_state_fields_round_trip_without_bumping_classic_links() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        StringAttribute::Value("值🙂\"\\".into()),
    ];
    for mut code in 0..3usize.pow(5) {
        let values: [StringAttribute; 5] = std::array::from_fn(|_| {
            let value = states[code % 3].clone();
            code /= 3;
            value
        });
        let link = LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href(values[0].clone())
                .with_target(values[1].clone())
                .with_rel(values[2].clone())
                .with_class(values[3].clone())
                .with_title(values[4].clone()),
        );
        let version = if link.classic_parts().is_some() { 4 } else { 8 };
        assert_round_trip(&slice(link), version);
    }
    assert_round_trip(&slice(LinkMark::new("", Some("".into()))), 4);
    assert_round_trip(
        &slice(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href("".into())
                .with_class("".into()),
        )),
        8,
    );
}

#[test]
fn mixed_v8_runs_keep_old_links_and_new_fields_distinct() {
    let (doc, node) = document(
        vec![
            run(LinkMark::new("https://old.test", None)),
            run(rich_link()),
        ],
        NodeAttrs::new([("default".into(), AttrValue::Null)].into()).unwrap(),
    );
    let slice = copy(doc, node);
    let wire = assert_round_trip(&slice, 8);
    assert_eq!(
        wire["roots"][0]["content"]["value"]["runs"][0]["marks"][1]["type"],
        "link"
    );
    let attrs = &wire["roots"][0]["content"]["value"]["runs"][1]["marks"][1]["attrs"];
    assert_eq!(attrs["target"], json!({"type":"null"}));
    assert_eq!(attrs["class"], json!({"type":"string","value":""}));
    assert_eq!(attrs["href"]["value"], "https://example.test/路径");
    for version in [1, 2, 3, 4, 5, 6, 7, 9, 999] {
        let mut changed = wire.clone();
        changed["version"] = version.into();
        assert!(
            decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
            "version {version}"
        );
    }
}

fn table_slice(link: LinkMark, row_attrs: NodeAttrs, cell_attrs: NodeAttrs) -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new([run(link)]).unwrap()),
        )
        .unwrap();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let cell = builder
        .insert(
            NodeKind::TableCell,
            cell_attrs,
            NodeContent::children([quote]),
        )
        .unwrap();
    let row = builder
        .insert(NodeKind::TableRow, row_attrs, NodeContent::children([cell]))
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = DocumentSession::new(
        doc,
        DocumentSelection::collapsed(TextPoint::at_start_of(paragraph)),
    )
    .unwrap();
    session.set_cell_range_selection(cell, cell).unwrap();
    session.clipboard_slice().unwrap().unwrap()
}

#[test]
fn conditional_versions_cover_tables_row_attrs_null_and_deep_links() {
    let plain = || LinkMark::new("https://old.test", None);
    let row_attrs =
        || NodeAttrs::new([("label".into(), AttrValue::String("row".into()))].into()).unwrap();
    let null_attrs = || {
        NodeAttrs::new(
            [(
                "nested".into(),
                AttrValue::Object([("null".into(), AttrValue::Null)].into()),
            )]
            .into(),
        )
        .unwrap()
    };
    assert_round_trip(&slice(plain()), 4);
    assert_round_trip(
        &table_slice(plain(), NodeAttrs::empty(), NodeAttrs::empty()),
        5,
    );
    assert_round_trip(&table_slice(plain(), row_attrs(), NodeAttrs::empty()), 6);
    assert_round_trip(&table_slice(plain(), row_attrs(), null_attrs()), 7);
    assert_round_trip(&table_slice(rich_link(), row_attrs(), null_attrs()), 8);
    let slice = table_slice(rich_link(), NodeAttrs::empty(), NodeAttrs::empty());
    let mut wire = assert_round_trip(&slice, 8);
    for version in 4..8 {
        wire["version"] = version.into();
        assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
    }
}

#[test]
fn legacy_links_keep_v4_through_v7_title_semantics_and_historical_rejections() {
    let slice = slice(LinkMark::new("https://old.test", None));
    let original = wire(&slice);
    for version in 1..=9 {
        for omitted in [false, true] {
            let mut wire = original.clone();
            wire["version"] = version.into();
            if omitted {
                wire["roots"][0]["content"]["value"]["runs"][0]["marks"][1]
                    .as_object_mut()
                    .unwrap()
                    .remove("title");
            }
            let decoded = decode_metadata(slice.plain_text(), &wire.to_string());
            assert_eq!(decoded.is_some(), (4..=8).contains(&version));
            if let Some(decoded) = decoded {
                assert_eq!(decoded, slice);
            }
        }
    }
    for invalid in [json!(null), json!(3), json!([]), json!({})] {
        let mut wire = original.clone();
        wire["roots"][0]["content"]["value"]["runs"][0]["marks"][1]["href"] = invalid;
        assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
    }
    let mut wire = original;
    wire["roots"][0]["content"]["value"]["runs"][0]["marks"][1]["target"] = json!("_blank");
    assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
}

const MARK: &str = "/roots/0/content/value/runs/0/marks/1";

#[test]
fn v8_rejects_unknown_fields_missing_slots_wrong_tags_and_wrong_types() {
    let slice = slice(rich_link());
    let original = wire(&slice);
    for path in [
        MARK.to_owned(),
        format!("{MARK}/attrs"),
        format!("{MARK}/attrs/href"),
        format!("{MARK}/attrs/target"),
    ] {
        let mut changed = original.clone();
        changed.pointer_mut(&path).unwrap()["future"] = json!({"preserve":"me"});
        assert!(
            decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
            "{path}"
        );
    }
    for key in ["href", "target", "rel", "class", "title"] {
        let mut changed = original.clone();
        changed
            .pointer_mut(&format!("{MARK}/attrs"))
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(
            decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
            "missing {key}"
        );
        for invalid in [
            json!(null),
            json!("text"),
            json!(false),
            json!(1.5),
            json!([]),
            json!({"type":"future"}),
            json!({"type":"string"}),
            json!({"type":"string","value":null}),
            json!({"type":"string","value":42}),
            json!({"type":"null","value":null}),
            json!({"type":"missing","value":"hidden"}),
        ] {
            let mut changed = original.clone();
            changed.pointer_mut(&format!("{MARK}/attrs")).unwrap()[key] = invalid;
            assert!(
                decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
                "type at {key}"
            );
        }
    }
}

#[test]
fn duplicate_keys_cannot_overwrite_exact_link_states_or_feature_versions() {
    let slice = slice(rich_link());
    let metadata = encode_metadata(&slice).unwrap();
    for (needle, replacement) in [
        ("\"version\":8", "\"version\":7,\"version\":8"),
        (
            "\"target\":{\"type\":\"null\"}",
            "\"target\":{\"type\":\"null\"},\"target\":{\"type\":\"missing\"}",
        ),
        (
            "\"target\":{\"type\":\"null\"}",
            "\"target\":{\"type\":\"null\"},\"targ\\u0065t\":{\"type\":\"missing\"}",
        ),
        (
            "\"type\":\"null\"",
            "\"type\":\"missing\",\"type\":\"null\"",
        ),
        ("\"value\":\"\"", "\"value\":null,\"value\":\"\""),
    ] {
        assert!(metadata.contains(needle), "{needle}");
        assert!(
            decode_metadata(
                slice.plain_text(),
                &metadata.replacen(needle, replacement, 1)
            )
            .is_none()
        );
    }
}

#[test]
fn pasted_and_edited_links_keep_all_states_through_undo_redo() {
    let rich = rich_link();
    let slice = slice(rich.clone());
    let decoded = decode_metadata(slice.plain_text(), &encode_metadata(&slice).unwrap()).unwrap();
    let (doc, node) = document(
        vec![TextRun::new("tail", MarkSet::empty()).unwrap()],
        NodeAttrs::empty(),
    );
    let original = doc.clone();
    let mut session = DocumentSession::new(
        doc,
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::PasteSlice { slice: decoded })
        .unwrap();
    let pasted = session.document().clone();
    assert_eq!(
        pasted
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks()
            .as_slice(),
        &[Mark::Bold, Mark::Link(rich)]
    );
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), pasted.store());
    let changed = LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(StringAttribute::Null)
            .with_class("changed".into()),
    );
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let range = TextRange::new(
        inline.offset_at(0).unwrap(),
        inline.offset_at("中🙂".len()).unwrap(),
    )
    .unwrap();
    session
        .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::AddMark {
                node,
                range,
                mark: Mark::Link(changed.clone()),
            },
        ))
        .unwrap();
    assert_eq!(
        session
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .marks()
            .as_slice(),
        &[Mark::Bold, Mark::Link(changed)]
    );
    let edited = session.document().clone();
    session
        .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::RemoveMark {
                node,
                range,
                mark_kind: MarkKind::Link,
            },
        ))
        .unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().store(), edited.store());
    session.undo().unwrap();
    assert_eq!(session.document().store(), pasted.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), edited.store());
}
