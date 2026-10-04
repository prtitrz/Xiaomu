//! Exact TextStyle uses conditional v9 without changing historical mark formats.

use serde_json::{Value, json};
use xiaomu_core::document::{
    AttrValue, InlineContent, LinkAttributes, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeKind, NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes, TextStyleMark,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

fn style(values: [StringAttribute; 3]) -> Mark {
    let [color, family, size] = values;
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(color)
            .with_font_family(family)
            .with_font_size(size),
    ))
}

fn sample() -> Mark {
    style(["red".into(), StringAttribute::Null, "".into()])
}

fn slice(
    marks: Vec<Mark>,
    table: bool,
    row_attrs: NodeAttrs,
    leaf_attrs: NodeAttrs,
) -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let leaf = builder
        .insert(
            NodeKind::Paragraph,
            leaf_attrs,
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("中🙂", MarkSet::new(marks).unwrap()).unwrap(),
                    TextRun::new("plain", MarkSet::empty()).unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let (child, cell) = if table {
        let quote = builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([leaf]),
            )
            .unwrap();
        let cell = builder
            .insert(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([quote]),
            )
            .unwrap();
        let row = builder
            .insert(NodeKind::TableRow, row_attrs, NodeContent::children([cell]))
            .unwrap();
        (
            builder
                .insert(
                    NodeKind::Table,
                    NodeAttrs::empty(),
                    NodeContent::children([row]),
                )
                .unwrap(),
            Some(cell),
        )
    } else {
        (leaf, None)
    };
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([child]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, builder.finish()).unwrap();
    let end = doc
        .node(leaf)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at("中🙂plain".len())
        .unwrap();
    let mut session = DocumentSession::new(
        doc,
        DocumentSelection::new(
            TextPoint::at_start_of(leaf),
            TextPoint::new(leaf, end, CursorAffinity::Before),
        ),
    )
    .unwrap();
    if let Some(cell) = cell {
        session.set_cell_range_selection(cell, cell).unwrap();
    }
    session.clipboard_slice().unwrap().unwrap()
}

fn plain_slice(mark: Mark) -> ClipboardSlice {
    slice(vec![mark], false, NodeAttrs::empty(), NodeAttrs::empty())
}

fn roundtrip(slice: &ClipboardSlice, version: u32) -> Value {
    let metadata = encode_metadata(slice).unwrap();
    let value: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(value["version"], version);
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(&decoded, slice);
    assert_eq!(encode_metadata(&decoded).unwrap(), metadata);
    assert!(decode_metadata("stale", &metadata).is_none());
    value
}

#[test]
fn all_states_and_unrecognized_strings_roundtrip_without_css_interpretation() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        "".into(),
        "calc(2em + 1px); 未解析🙂".into(),
    ];
    for mut code in 0..4usize.pow(3) {
        let values = std::array::from_fn(|_| {
            let value = states[code % 4].clone();
            code /= 4;
            value
        });
        let wire = roundtrip(&plain_slice(style(values)), 9);
        let mark = &wire["roots"][0]["content"]["value"]["runs"][0]["marks"][0];
        assert_eq!(mark["type"], "text_style");
        assert_eq!(mark["attrs"].as_object().unwrap().len(), 3);
    }
}

#[test]
fn conditional_v9_has_priority_only_when_text_style_is_present() {
    let row = || NodeAttrs::new([("row".into(), AttrValue::String("r".into()))].into()).unwrap();
    let null = || NodeAttrs::new([("nullable".into(), AttrValue::Null)].into()).unwrap();
    let classic = || Mark::Link(LinkMark::new("https://classic.test", None));
    let rich = || {
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default().with_href(StringAttribute::Null),
        ))
    };
    roundtrip(&plain_slice(classic()), 4);
    roundtrip(
        &slice(
            vec![classic()],
            true,
            NodeAttrs::empty(),
            NodeAttrs::empty(),
        ),
        5,
    );
    roundtrip(&slice(vec![classic()], true, row(), NodeAttrs::empty()), 6);
    roundtrip(&slice(vec![classic()], true, row(), null()), 7);
    roundtrip(&slice(vec![rich()], true, row(), null()), 8);
    for marks in [
        vec![sample()],
        vec![Mark::Bold, classic(), sample()],
        vec![rich(), sample()],
    ] {
        roundtrip(&slice(marks, true, row(), null()), 9);
    }
}

#[test]
fn new_mark_requires_v9_but_legacy_payloads_keep_their_meaning() {
    let slice = plain_slice(sample());
    let original = roundtrip(&slice, 9);
    for version in [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 999] {
        let mut changed = original.clone();
        changed["version"] = version.into();
        assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_none());
    }
    let plain = plain_slice(Mark::Link(LinkMark::new("old", None)));
    let original = roundtrip(&plain, 4);
    for version in 4..=9 {
        let mut changed = original.clone();
        changed["version"] = version.into();
        assert_eq!(
            decode_metadata(plain.plain_text(), &changed.to_string()),
            Some(plain.clone())
        );
    }
}

const MARK: &str = "/roots/0/content/value/runs/0/marks/0";

#[test]
fn unknown_fields_missing_slots_bad_types_and_conflicting_marks_reject_whole_slice() {
    let slice = plain_slice(sample());
    let original = roundtrip(&slice, 9);
    for path in [
        MARK.to_owned(),
        format!("{MARK}/attrs"),
        format!("{MARK}/attrs/font_family"),
    ] {
        let mut wire = original.clone();
        wire.pointer_mut(&path).unwrap()["future"] = json!("keep-me");
        assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
    }
    for field in ["color", "font_family", "font_size"] {
        let mut wire = original.clone();
        wire.pointer_mut(&format!("{MARK}/attrs"))
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
        for invalid in [
            json!(null),
            json!(false),
            json!(3),
            json!([]),
            json!("red"),
            json!({"type":"future"}),
            json!({"type":"string"}),
            json!({"type":"string","value":null}),
            json!({"type":"string","value":23}),
            json!({"type":"null","value":"red"}),
            json!({"type":"missing","value":"hidden"}),
        ] {
            let mut wire = original.clone();
            wire.pointer_mut(&format!("{MARK}/attrs")).unwrap()[field] = invalid;
            assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
        }
    }
    let mut wire = original.clone();
    let mut other = original.pointer(MARK).unwrap().clone();
    other["attrs"]["color"] = json!({"type":"null"});
    wire["roots"][0]["content"]["value"]["runs"][0]["marks"]
        .as_array_mut()
        .unwrap()
        .push(other);
    assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
}

#[test]
fn duplicate_keys_cannot_hide_versions_or_exact_attribute_states() {
    let slice = plain_slice(sample());
    let metadata = encode_metadata(&slice).unwrap();
    for (from, to) in [
        ("\"version\":9", "\"version\":8,\"version\":9"),
        (
            "\"color\":",
            "\"color\":{\"type\":\"missing\"},\"col\\u006fr\":",
        ),
        (
            "\"type\":\"null\"",
            "\"type\":\"missing\",\"type\":\"null\"",
        ),
        ("\"value\":\"\"", "\"value\":null,\"value\":\"\""),
    ] {
        assert!(metadata.contains(from));
        assert!(decode_metadata(slice.plain_text(), &metadata.replacen(from, to, 1)).is_none());
    }
}
