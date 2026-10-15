//! Exact leaf descriptors are recomputed, versioned and fail-closed on the wire.

use super::*;
use crate::clipboard::{ClipboardAtom, ClipboardSourceBoundary, ClipboardTextProjection};
use serde_json::{Value, json};
use xiaomu_core::document::{AtomKind, InlineAtomContent, Mark, MarkSet, TextRun};
use xiaomu_core::text::TextOffset;

const PREFIX: &str = "xiaomu.clipboard.v15\n";

fn projection(keys: &[&str]) -> ClipboardTextProjection {
    ClipboardTextProjection::text_between_lf_with_atoms(
        keys.iter().map(|key| AtomKind::new(*key).unwrap()),
    )
    .unwrap()
}

fn leaf() -> ClipboardNode {
    ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(
            ClipboardInline::new(
                [TextRun::new("中🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                [ClipboardAtom::new(
                    TextOffset::ZERO,
                    AtomKind::new("example.reference.v1").unwrap(),
                    NodeAttrs::new(
                        [("name".into(), AttrValue::String("Synthetic label".into()))].into(),
                    )
                    .unwrap(),
                    InlineAtomContent::new("visible fallback")
                        .unwrap()
                        .with_marks(MarkSet::new([Mark::Italic]).unwrap()),
                )],
            )
            .unwrap(),
        ),
    )
}

fn slice() -> ClipboardSlice {
    ClipboardSlice::from_export_roots(
        vec![leaf()],
        ClipboardSourceBoundary::WholeRoots,
        Some(projection(&["example.reference.v1"])),
    )
    .unwrap()
}

fn wire() -> Value {
    let metadata = encode_metadata(&slice()).unwrap();
    serde_json::from_str(metadata.strip_prefix(PREFIX).unwrap()).unwrap()
}

fn reject(value: Value) {
    assert_eq!(
        decode_metadata_checked("\n中🙂", &format!("{PREFIX}{value}")),
        ClipboardMetadataDecode::RejectedNative,
        "{value}"
    );
}

#[test]
fn leaf_descriptor_and_complete_rich_payload_round_trip() {
    let source = slice();
    let metadata = encode_metadata(&source).unwrap();
    assert_eq!(source.plain_text(), "\n中🙂");
    assert_eq!(
        wire()["text_projection"],
        json!({"kind":"text_between_lf_with_atoms_v1","atom_kinds":["example.reference.v1"]})
    );
    assert_eq!(
        decode_metadata(source.plain_text(), &metadata),
        Some(source)
    );
    for text in ["visible fallback中🙂", "\u{fffc}中🙂", "中🙂", "\r\n中🙂"] {
        assert_eq!(
            decode_metadata_checked(text, &metadata),
            ClipboardMetadataDecode::RejectedNative
        );
    }
}

#[test]
fn leaf_descriptor_unknown_missing_null_wrong_type_and_extra_fields_reject() {
    for descriptor in [
        Value::Null,
        json!("text_between_lf_v1"),
        json!("text_between_lf_with_atoms_v1"),
        json!([]),
        json!({}),
        json!({"kind":"future","atom_kinds":["example.reference.v1"]}),
        json!({"kind":{"text_between_lf_with_atoms_v1":null},"atom_kinds":["example.reference.v1"]}),
        json!({"kind":"text_between_lf_with_atoms_v1"}),
        json!({"atom_kinds":["example.reference.v1"]}),
        json!({"kind":"text_between_lf_with_atoms_v1","atom_kinds":null}),
        json!({"kind":"text_between_lf_with_atoms_v1","atom_kinds":"example.reference.v1"}),
        json!({"kind":"text_between_lf_with_atoms_v1","atom_kinds":["example.reference.v1"],"fallback":"\n"}),
    ] {
        let mut value = wire();
        value["text_projection"] = descriptor;
        reject(value);
    }
    let mut missing = wire();
    missing.as_object_mut().unwrap().remove("text_projection");
    reject(missing);
    let value = wire();
    reject(json!([
        value["format"],
        value["version"],
        value["roots"],
        value["closed"],
        value["source_boundary"],
        value["text_projection"]
    ]));
}

#[test]
fn maximum_descriptor_roundtrips_with_all_keys_at_the_byte_limit() {
    let kinds = (0..32)
        .map(|index| AtomKind::new(format!("{index:02}{}", "\u{1}".repeat(126))).unwrap())
        .collect::<Vec<_>>();
    let node = ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(
            ClipboardInline::new(
                [],
                [ClipboardAtom::new(
                    TextOffset::ZERO,
                    kinds[31].clone(),
                    NodeAttrs::empty(),
                    InlineAtomContent::new("label").unwrap(),
                )],
            )
            .unwrap(),
        ),
    );
    let projection = ClipboardTextProjection::text_between_lf_with_atoms(kinds).unwrap();
    let slice = ClipboardSlice::from_export_roots(
        vec![node],
        ClipboardSourceBoundary::Open,
        Some(projection),
    )
    .unwrap();
    let metadata = encode_metadata(&slice).unwrap();
    assert_eq!(slice.plain_text(), "\n");
    assert_eq!(decode_metadata("\n", &metadata), Some(slice));
}

#[test]
fn leaf_kinds_are_finite_canonical_exact_extension_keys() {
    for keys in [
        json!([]),
        json!([""]),
        json!([" \n\t"]),
        json!([null]),
        json!([42]),
        json!([{"type":"hard_break"}]),
        json!(["example.reference.v1", "example.reference.v1"]),
        json!(["z", "example.reference.v1"]),
        json!(["Example.reference.v1"]),
        json!(["example.reference.v"]),
        json!(["example.reference.v1 "]),
        json!(["x".repeat(129)]),
        json!(["é".repeat(65)]),
        json!((0..33).map(|i| format!("kind-{i:02}")).collect::<Vec<_>>()),
    ] {
        let mut value = wire();
        value["text_projection"]["atom_kinds"] = keys;
        reject(value);
    }
    // Escaped strings decode to the same key before uniqueness validation.
    let metadata = encode_metadata(&slice()).unwrap().replace(
        "\"atom_kinds\":[\"example.reference.v1\"]",
        "\"atom_kinds\":[\"example.reference.v1\",\"\\u0065xample.reference.v1\"]",
    );
    assert_eq!(
        decode_metadata_checked("\n中🙂", &metadata),
        ClipboardMetadataDecode::RejectedNative
    );
}

#[test]
fn escaped_exact_keys_round_trip_without_normalizing_unicode_or_whitespace() {
    for key in [" quote\"\\\t", "é", "e\u{301}", "hardBreak", "a"].map(str::to_owned) {
        let node = ClipboardNode::new(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            ClipboardNodeContent::Inline(
                ClipboardInline::new(
                    [],
                    [ClipboardAtom::new(
                        TextOffset::ZERO,
                        AtomKind::new(&key).unwrap(),
                        NodeAttrs::empty(),
                        InlineAtomContent::new("not-LF").unwrap(),
                    )],
                )
                .unwrap(),
            ),
        );
        let source = ClipboardSlice::from_export_roots(
            vec![node],
            ClipboardSourceBoundary::Open,
            Some(projection(&[&key])),
        )
        .unwrap();
        let encoded = encode_metadata(&source).unwrap();
        assert_eq!(source.plain_text(), "\n");
        assert_eq!(decode_metadata("\n", &encoded), Some(source));
    }
}

#[test]
fn v15_prefix_body_version_duplicates_and_downgrades_fail_closed() {
    let metadata = encode_metadata(&slice()).unwrap();
    for malformed in [
        metadata.replacen("\"version\":15", "\"version\":14", 1),
        metadata.replacen("v15\n", "v14\n", 1),
        metadata.replacen("v15\n", "v999\n", 1),
        metadata.replacen("\"version\":15", "\"version\":15,\"\\u0076ersion\":15", 1),
        metadata.replacen("\"kind\":\"text_between_lf_with_atoms_v1\"", "\"kind\":\"text_between_lf_with_atoms_v1\",\"\\u006bind\":\"text_between_lf_with_atoms_v1\"", 1),
        metadata.replacen("\"atom_kinds\":", "\"atom_kinds\":[],\"atom_kinds\":", 1),
        format!("{metadata}{{}}"),
        format!("{PREFIX}{{"),
        format!("{metadata}{}", " ".repeat(16 * 1024 * 1024)),
    ] {
        assert_eq!(
            decode_metadata_checked("\n中🙂", &malformed),
            ClipboardMetadataDecode::RejectedNative
        );
    }
    // A valid object descriptor cannot be smuggled into the older framed schema.
    let old = metadata
        .replace("v15\n", "v14\n")
        .replace("\"version\":15", "\"version\":14");
    assert_eq!(
        decode_metadata_checked("\n中🙂", &old),
        ClipboardMetadataDecode::RejectedNative
    );
    let unframed = metadata.strip_prefix(PREFIX).unwrap();
    assert_eq!(
        decode_metadata_checked("\n中🙂", unframed),
        ClipboardMetadataDecode::ForeignOrLegacyFallback
    );
}

#[test]
fn unlisted_atom_or_invalid_source_cannot_be_hidden_by_a_forged_descriptor() {
    let mut unknown = wire();
    unknown["roots"][0]["content"]["value"]["atoms"][0]["kind"]["value"] = json!("unknown");
    reject(unknown);
    let mut boundary = wire();
    boundary["closed"] = json!(false);
    reject(boundary);
    // Rich labels/attrs need not equal LF. Descriptors prove consistency, not
    // authenticity: a coherent third-party rewrite is still untrusted content.
    let mut label = wire();
    label["roots"][0]["content"]["value"]["atoms"][0]["fallback"] = json!("another label");
    let decoded = decode_metadata("\n中🙂", &format!("{PREFIX}{label}")).unwrap();
    assert_eq!(
        decoded.blocks()[0].inline().atoms()[0]
            .content()
            .fallback_text(),
        "another label"
    );
}

#[test]
fn v14_text_only_encoding_stays_byte_exact() {
    let node = ClipboardNode::new(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        ClipboardNodeContent::Inline(
            ClipboardInline::text_only([TextRun::new("old", MarkSet::empty()).unwrap()]).unwrap(),
        ),
    );
    let slice = ClipboardSlice::from_export_roots(
        vec![node],
        ClipboardSourceBoundary::Open,
        Some(ClipboardTextProjection::TextBetweenLfV1),
    )
    .unwrap();
    let expected = concat!(
        "xiaomu.clipboard.v14\n",
        r#"{"format":"xiaomu.clipboard","version":14,"roots":[{"kind":{"type":"paragraph"},"attrs":{},"content":{"type":"inline","value":{"runs":[{"text":"old","marks":[]}]}}}],"closed":false,"source_boundary":{"kind":"open"},"text_projection":"text_between_lf_v1"}"#
    );
    assert_eq!(encode_metadata(&slice).unwrap(), expected);
    assert_eq!(decode_metadata("old", expected), Some(slice));
}
