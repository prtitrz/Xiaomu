//! Framed-source validation and byte-exact deterministic projection regression.

use super::*;
use crate::clipboard::{
    ClipboardAtom, ClipboardCellRangeRoot, ClipboardSourceBoundary, ClipboardTextProjection,
};
use serde_json::{Value, json};
use xiaomu_core::document::{AtomKind, InlineAtomContent, Mark, MarkSet, TextRun};
use xiaomu_core::text::TextOffset;

const PREFIX: &str = "xiaomu.clipboard.v14\n";

fn paragraph(text: &str) -> ClipboardNode {
    let runs = if text.is_empty() {
        vec![]
    } else {
        vec![TextRun::new(text, MarkSet::empty()).unwrap()]
    };
    ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(ClipboardInline::text_only(runs).unwrap()),
    )
}

fn table() -> ClipboardNode {
    let cell = ClipboardNode::new(
        NodeKind::TableHeader,
        NodeAttrs::new(
            [
                ("rowspan".into(), AttrValue::Integer(2)),
                ("colspan".into(), AttrValue::Integer(2)),
                (
                    "colwidth".into(),
                    AttrValue::List(vec![AttrValue::Integer(80), AttrValue::Integer(120)]),
                ),
            ]
            .into(),
        )
        .unwrap(),
        ClipboardNodeContent::Children(vec![paragraph("inner")]),
    );
    ClipboardNode::new(
        NodeKind::Table,
        NodeAttrs::empty(),
        ClipboardNodeContent::Table {
            rows: vec![vec![cell], vec![]],
            row_attrs: vec![
                NodeAttrs::empty(),
                NodeAttrs::new([("emptyContent".into(), AttrValue::Bool(true))].into()).unwrap(),
            ],
        },
    )
}

fn projected(roots: Vec<ClipboardNode>, source: ClipboardSourceBoundary) -> ClipboardSlice {
    ClipboardSlice::from_export_roots(
        roots,
        source,
        Some(ClipboardTextProjection::TextBetweenLfV1),
    )
    .unwrap()
}

fn wire(slice: &ClipboardSlice) -> Value {
    let metadata = encode_metadata(slice).unwrap();
    serde_json::from_str(metadata.strip_prefix(PREFIX).unwrap()).unwrap()
}

fn reject(plain: &str, value: &Value) {
    let metadata = format!("{PREFIX}{value}");
    assert_eq!(
        decode_metadata_checked(plain, &metadata),
        ClipboardMetadataDecode::RejectedNative
    );
    assert_eq!(decode_metadata(plain, &metadata), None);
}

#[test]
fn framed_text_between_keeps_empty_blocks_break_marks_images_nested_tables_and_raw_text() {
    let hard_break = ClipboardAtom::new(
        TextOffset::ZERO,
        AtomKind::hard_break(),
        NodeAttrs::empty(),
        InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Italic]).unwrap()),
    );
    let breaks = ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(
            ClipboardInline::new(
                [TextRun::new("甲\r\n乙\t", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                [hard_break],
            )
            .unwrap(),
        ),
    );
    let image = ClipboardNode::new(
        NodeKind::Image,
        NodeAttrs::new(
            [
                (
                    "src".into(),
                    AttrValue::String("https://example.test/do-not-project".into()),
                ),
                ("alt".into(), AttrValue::String("do-not-project-alt".into())),
            ]
            .into(),
        )
        .unwrap(),
        ClipboardNodeContent::Atomic,
    );
    let quote = ClipboardNode::new(
        NodeKind::Quote,
        NodeAttrs::empty(),
        ClipboardNodeContent::Children(vec![paragraph("Q"), table()]),
    );
    let slice = projected(
        vec![
            paragraph(""),
            breaks,
            ClipboardNode::new(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                ClipboardNodeContent::Atomic,
            ),
            image,
            quote,
        ],
        ClipboardSourceBoundary::WholeRoots,
    );
    assert_eq!(
        slice.plain_text(),
        concat!("\n", "\n甲\r\n乙\t", "\n\n", "\n\n", "\nQ", "\ninner")
    );
    let inline = slice.roots()[1].content().as_inline().unwrap();
    assert_eq!(
        inline.atoms()[0].content().marks(),
        &MarkSet::new([Mark::Italic]).unwrap()
    );
    assert_eq!(
        inline.runs()[0].marks(),
        &MarkSet::new([Mark::Bold]).unwrap()
    );
    let metadata = encode_metadata(&slice).unwrap();
    assert_eq!(
        decode_metadata(slice.plain_text(), &metadata).as_ref(),
        Some(&slice)
    );
    assert_eq!(
        decode_metadata_checked(&slice.plain_text().replace('\r', ""), &metadata),
        ClipboardMetadataDecode::RejectedNative
    );
}

#[test]
fn unknown_custom_blocks_and_extension_atoms_are_never_silently_omitted() {
    let unknown_block = ClipboardNode::new(
        NodeKind::custom("host-block").unwrap(),
        NodeAttrs::empty(),
        ClipboardNodeContent::Atomic,
    );
    assert!(
        ClipboardSlice::from_export_roots(
            vec![unknown_block],
            ClipboardSourceBoundary::WholeRoots,
            Some(ClipboardTextProjection::TextBetweenLfV1)
        )
        .is_err()
    );
    let unknown_atom = ClipboardAtom::new(
        TextOffset::ZERO,
        AtomKind::new("hardBreak").unwrap(),
        NodeAttrs::empty(),
        InlineAtomContent::new("\n").unwrap(),
    );
    let node = ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(ClipboardInline::new([], [unknown_atom]).unwrap()),
    );
    assert!(
        ClipboardSlice::from_export_roots(
            vec![node],
            ClipboardSourceBoundary::WholeRoots,
            Some(ClipboardTextProjection::TextBetweenLfV1)
        )
        .is_err()
    );
}

#[test]
fn cell_source_requires_one_valid_table_carrier_open_one_one_and_explicit_root_form() {
    for root_form in [ClipboardCellRangeRoot::Rows, ClipboardCellRangeRoot::Table] {
        let source = ClipboardSourceBoundary::CellRange { root_form };
        let slice = projected(vec![table()], source);
        let original = wire(&slice);
        assert_eq!(original["source_boundary"]["open_start"], 1);
        assert_eq!(original["source_boundary"]["open_end"], 1);
        assert_eq!(original["closed"], false);
        for (path, bad) in [
            (vec!["closed"], json!(true)),
            (vec!["source_boundary", "open_start"], json!(0)),
            (vec!["source_boundary", "open_end"], json!(2)),
            (vec!["source_boundary", "root_form"], json!("guessed")),
            (vec!["text_projection"], json!(null)),
            (vec!["text_projection"], json!("arbitrary-text")),
        ] {
            let mut value = original.clone();
            let mut at = &mut value;
            for key in path {
                at = &mut at[key];
            }
            *at = bad;
            reject(slice.plain_text(), &value);
        }
        let mut extra = original.clone();
        let duplicate = extra["roots"][0].clone();
        extra["roots"].as_array_mut().unwrap().push(duplicate);
        reject(slice.plain_text(), &extra);
        let mut missing = original.clone();
        missing["source_boundary"]
            .as_object_mut()
            .unwrap()
            .remove("root_form");
        reject(slice.plain_text(), &missing);
        let mut invalid_grid = original;
        let rowspan = invalid_grid
            .pointer_mut("/roots/0/content/value/rows/0/0/attrs/rowspan/value")
            .expect("actual encoded Table DTO has tagged content/value/rows");
        assert_eq!(*rowspan, json!(2));
        *rowspan = json!(3);
        reject(slice.plain_text(), &invalid_grid);
    }
    let roots = vec![paragraph("not a table")];
    assert!(
        ClipboardSlice::from_export_roots(
            roots,
            ClipboardSourceBoundary::CellRange {
                root_form: ClipboardCellRangeRoot::Rows
            },
            Some(ClipboardTextProjection::TextBetweenLfV1)
        )
        .is_err()
    );
}

#[test]
fn version_keys_cannot_overwrite_native_recognition_or_enable_fallback() {
    let slice = projected(vec![paragraph("safe")], ClipboardSourceBoundary::WholeRoots);
    let metadata = encode_metadata(&slice).unwrap();
    for bad in [
        metadata.replace("\"version\":14", "\"version\":14,\"version\":4"),
        metadata.replace("\"version\":14", "\"version\":4,\"version\":14"),
        metadata.replace("\"version\":14", "\"version\":14,\"vers\\u0069on\":4"),
        metadata.replace("\"version\":14", "\"version\":999"),
        metadata.replace("\"version\":14", "\"version\":null"),
        metadata.replace(
            "\"format\":\"xiaomu.clipboard\"",
            "\"format\":\"xiaomu.clipboard\",\"format\":\"foreign\"",
        ),
        format!("{PREFIX}{{"),
        "xiaomu.clipboard.v999\n{}".into(),
        format!("{PREFIX}{}", " ".repeat(16 * 1024 * 1024 + 1)),
    ] {
        assert_eq!(
            decode_metadata_checked(slice.plain_text(), &bad),
            ClipboardMetadataDecode::RejectedNative
        );
    }
    assert_eq!(
        decode_metadata_checked("stale", &metadata),
        ClipboardMetadataDecode::RejectedNative
    );
    for foreign in ["", "plain", "{}", r#"{"format":"foreign","version":14}"#] {
        assert_eq!(
            decode_metadata_checked("plain", foreign),
            ClipboardMetadataDecode::ForeignOrLegacyFallback
        );
    }
}

#[test]
fn frame_prefix_counts_toward_the_total_metadata_budget() {
    let slice = projected(vec![paragraph("safe")], ClipboardSourceBoundary::WholeRoots);
    let mut metadata = encode_metadata(&slice).unwrap();
    metadata.push_str(&" ".repeat(16 * 1024 * 1024 - metadata.len() + 1));
    // The JSON body is still below 16 MiB and syntactically valid, but the
    // complete metadata transport exceeds the documented limit by one byte.
    assert_eq!(metadata.len(), 16 * 1024 * 1024 + 1);
    assert_eq!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::RejectedNative
    );
}

#[test]
fn projection_and_provenance_are_independent_but_cannot_be_downgraded_in_a_frame() {
    let slice = projected(vec![paragraph("a")], ClipboardSourceBoundary::WholeRoots);
    let original = wire(&slice);
    for version in [4, 11, 13, 15] {
        let mut value = original.clone();
        value["version"] = json!(version);
        reject("a", &value);
    }
    for boundary in [
        json!({"kind":"whole_roots","open_start":1,"open_end":1}),
        json!({"kind":"open"}),
        json!({"kind":"cell_range","root_form":"rows","open_start":1,"open_end":1}),
    ] {
        let mut value = original.clone();
        value["source_boundary"] = boundary;
        reject("a", &value);
    }
    let mut no_projection = original.clone();
    no_projection
        .as_object_mut()
        .unwrap()
        .remove("text_projection");
    let metadata = format!("{PREFIX}{no_projection}");
    let decoded = decode_metadata("a", &metadata).unwrap();
    assert_eq!(
        decoded.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert_eq!(decoded.text_projection(), None);
    // An old reader must reject the new fields, never quietly drop descriptors.
    let mut old = original;
    old["version"] = json!(13);
    assert_eq!(
        decode_metadata_checked("a", &old.to_string()),
        ClipboardMetadataDecode::ForeignOrLegacyFallback
    );
}
