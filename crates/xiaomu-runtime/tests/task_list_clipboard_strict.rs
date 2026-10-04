//! Task wire compatibility, strict structural parsing, and shared resource limits.

mod task_list_support;
use task_list_support::*;

use serde_json::{Value, json};
use xiaomu_core::document::{AttrValue, NodeAttrs};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};

const ITEM: &str = "/roots/0/content/value/children/0";
const CHECKED: &str = "/roots/0/content/value/children/0/attrs/checked";

#[test]
fn v1_through_v11_reject_task_tags_even_with_other_valid_legacy_features() {
    let (document, first, tail) = fixture(Some(AttrValue::Bool(true)));
    for closed in [false, true] {
        let slice = copy(&document, first, tail, closed);
        let wire = roundtrip(&slice);
        for version in 1..=11 {
            let mut old = wire.clone();
            old["version"] = json!(version);
            if version < 11 {
                old.as_object_mut().unwrap().remove("closed");
            } else {
                old["closed"] = json!(true);
            }
            assert!(
                decode_metadata(slice.plain_text(), &old.to_string()).is_none(),
                "v{version}"
            );
        }
    }
}

#[test]
fn v12_requires_an_exact_boolean_source_boundary_and_preserves_both_values() {
    let (document, first, tail) = fixture(None);
    let slice = copy(&document, first, tail, false);
    let wire = roundtrip(&slice);
    for boundary in [Value::Null, json!("false"), json!(0), json!({}), json!([])] {
        let mut invalid = wire.clone();
        invalid["closed"] = boundary;
        assert!(decode_metadata(slice.plain_text(), &invalid.to_string()).is_none());
    }
    let mut absent = wire.clone();
    absent.as_object_mut().unwrap().remove("closed");
    assert!(decode_metadata(slice.plain_text(), &absent.to_string()).is_none());
    for boundary in [false, true] {
        let mut valid = wire.clone();
        valid["closed"] = json!(boundary);
        assert_eq!(
            decode_metadata(slice.plain_text(), &valid.to_string())
                .unwrap()
                .is_closed(),
            boundary
        );
    }
}

#[test]
fn malformed_checked_kind_shape_and_structural_fields_reject_the_whole_fragment() {
    let (document, first, tail) = fixture(Some(AttrValue::Bool(true)));
    let slice = copy(&document, first, tail, true);
    let wire = roundtrip(&slice);
    for value in [
        json!({"type":"integer","value":1}),
        json!({"type":"string","value":"false"}),
        json!({"type":"list","value":[]}),
        json!({"type":"object","value":{}}),
        json!({"type":"bool","value":"false"}),
        json!(null),
        json!(true),
    ] {
        let mut invalid = wire.clone();
        *invalid.pointer_mut(CHECKED).unwrap() = value;
        assert!(decode_metadata(slice.plain_text(), &invalid.to_string()).is_none());
    }
    for (path, value) in [
        ("/roots/0/kind", json!({"type":"bullet_list"})),
        (
            "/roots/0/content/value/children/0/kind",
            json!({"type":"list_item"}),
        ),
        (
            "/roots/0/content/value/children/0/content",
            json!({"type":"atomic"}),
        ),
        (
            "/roots/0/content/value/children/0/kind",
            json!({"type":"task_item","extra":true}),
        ),
    ] {
        let mut invalid = wire.clone();
        *invalid.pointer_mut(path).unwrap() = value;
        assert!(decode_metadata(slice.plain_text(), &invalid.to_string()).is_none());
    }
    for path in [
        "",
        "/roots/0",
        ITEM,
        "/roots/0/content",
        "/roots/0/content/value",
    ] {
        let mut invalid = wire.clone();
        invalid
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(decode_metadata(slice.plain_text(), &invalid.to_string()).is_none());
    }
    let mut root_item = wire.clone();
    root_item["roots"][0] = wire.pointer(ITEM).unwrap().clone();
    assert!(decode_metadata(slice.plain_text(), &root_item.to_string()).is_none());
}

#[test]
fn raw_duplicate_keys_cannot_hide_task_kinds_checked_values_or_boundary_flags() {
    let (document, first, tail) = fixture(Some(AttrValue::Bool(true)));
    let slice = copy(&document, first, tail, true);
    let metadata = encode_metadata(&slice).unwrap();
    for (needle, replacement) in [
        (r#""version":12"#, r#""version":11,"version":12"#),
        (r#""closed":true"#, r#""closed":false,"closed":true"#),
        (
            r#""type":"task_list""#,
            r#""type":"bullet_list","type":"task_list""#,
        ),
        (
            r#""type":"task_item""#,
            r#""type":"task_item","type":"list_item""#,
        ),
        (
            r#""checked":{"type":"bool","value":true}"#,
            r#""checked":{"type":"integer","value":1},"checked":{"type":"bool","value":true}"#,
        ),
        (
            r#""checked":{"type":"bool","value":true}"#,
            r#""checked":{"type":"bool","value":true},"chec\u006bed":{"type":"bool","value":false}"#,
        ),
        (r#""value":true"#, r#""value":null,"value":true"#),
    ] {
        let duplicate = metadata.replacen(needle, replacement, 1);
        assert_ne!(duplicate, metadata, "missing test needle: {needle}");
        // No Value round trip: it would erase precisely the ambiguity tested.
        assert!(decode_metadata(slice.plain_text(), &duplicate).is_none());
    }
}

#[test]
fn tasks_obey_shared_byte_value_and_depth_budgets_on_decode_and_encode() {
    let (document, first, tail) = fixture(None);
    let slice = copy(&document, first, tail, true);
    let wire = roundtrip(&slice);
    let metadata = encode_metadata(&slice).unwrap();
    let padded = format!("{metadata}{}", " ".repeat(16 * 1024 * 1024));
    assert!(decode_metadata(slice.plain_text(), &padded).is_none());
    let mut too_many = wire.clone();
    too_many["roots"][0]["attrs"]["large"] =
        json!({"type":"list", "value":vec![json!({"type":"null"}); 100_001]});
    assert!(decode_metadata(slice.plain_text(), &too_many.to_string()).is_none());
    let mut nested = r#"{"type":"null"}"#.to_owned();
    for _ in 0..130 {
        nested = format!(r#"{{"type":"list","value":[{nested}]}}"#);
    }
    let deep = metadata.replacen(
        r#""attrs":{}"#,
        &format!(r#""attrs":{{"deep":{nested}}}"#),
        1,
    );
    assert_ne!(deep, metadata);
    assert!(decode_metadata(slice.plain_text(), &deep).is_none());

    let task_list = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let oversized = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: task_list,
            attrs: NodeAttrs::new(
                [(
                    "large".into(),
                    AttrValue::String("x".repeat(16 * 1024 * 1024)),
                )]
                .into(),
            )
            .unwrap(),
        })
        .apply(&document)
        .unwrap();
    let large_slice = copy(&oversized, first, tail, true);
    assert!(encode_metadata(&large_slice).is_err());
}
