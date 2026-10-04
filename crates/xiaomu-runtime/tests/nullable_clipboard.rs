//! Null-aware wire versions and lossless reconstruction under fresh IDs.

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn nested_null_attrs() -> NodeAttrs {
    NodeAttrs::new(
        [(
            "extension".into(),
            AttrValue::Object(
                [(
                    "values".into(),
                    AttrValue::List(vec![
                        AttrValue::Bool(false),
                        AttrValue::Integer(0),
                        AttrValue::String("null".into()),
                        AttrValue::Object([("default".into(), AttrValue::Null)].into()),
                        AttrValue::Null,
                    ]),
                )]
                .into(),
            ),
        )]
        .into(),
    )
    .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, attrs: NodeAttrs) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            attrs,
            NodeContent::Inline(
                InlineContent::new([TextRun::new("中🙂", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> InlinePoint {
    let offset = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    InlinePoint::new(node, offset, 0, CursorAffinity::Before)
}

fn session(document: &XiaomuDocument, node: NodeId) -> DocumentSession {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(point(document, node, 0)),
    )
    .unwrap()
}

fn paragraph_slice(attrs: NodeAttrs) -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = paragraph(&mut builder, attrs);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(
        point(&document, paragraph, 0),
        point(&document, paragraph, "中🙂".len()),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn assert_v7_round_trip(slice: &ClipboardSlice) -> serde_json::Value {
    let metadata = encode_metadata(slice).unwrap();
    let wire: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(wire["version"], 7);
    assert_eq!(
        decode_metadata(slice.plain_text(), &metadata).as_ref(),
        Some(slice)
    );
    assert!(decode_metadata("stale text", &metadata).is_none());
    for version in [1, 2, 3, 4, 5, 6, 9, 999] {
        let mut changed = wire.clone();
        changed["version"] = version.into();
        assert!(
            decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
            "version {version}"
        );
    }
    wire
}

#[test]
fn wire_preserves_explicit_null_missing_and_nested_values() {
    let attrs = NodeAttrs::new([("textAlign".into(), AttrValue::Null)].into()).unwrap();
    let slice = paragraph_slice(attrs);
    let wire = assert_v7_round_trip(&slice);
    assert_eq!(
        wire["roots"][0]["attrs"]["textAlign"],
        serde_json::json!({"type":"null"})
    );
    assert!(wire["roots"][0]["attrs"].get("missing").is_none());
    assert_eq!(
        slice.blocks()[0].attrs().get("textAlign"),
        Some(&AttrValue::Null)
    );
    assert_eq!(slice.blocks()[0].attrs().get("missing"), None);
    assert_v7_round_trip(&paragraph_slice(nested_null_attrs()));
}

#[test]
fn unknown_attr_variants_and_floats_fail_closed_without_losing_known_attrs() {
    let slice = paragraph_slice(nested_null_attrs());
    let wire = assert_v7_round_trip(&slice);
    for invalid in [
        serde_json::json!({"type":"future_value","value":"preserve me"}),
        serde_json::json!({"type":"float","value":1.5}),
        serde_json::json!({"type":"integer","value":1.5}),
    ] {
        let mut changed = wire.clone();
        changed["roots"][0]["attrs"]["unknown"] = invalid.clone();
        assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_none());
        let mut changed = wire.clone();
        changed["roots"][0]["attrs"]["extension"]["value"]["values"]["value"][3]["value"]["default"] =
            invalid;
        assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_none());
    }
}

#[test]
fn unknown_structural_fields_fail_soft_at_every_wire_level() {
    let (document, intro, cell) = table_fixture("row");
    let slice = copy_cell(&mut session(&document, intro), cell);
    let wire = assert_v7_round_trip(&slice);
    let table = "/roots/0/content/value";
    let cell = format!("{table}/rows/0/0");
    let quote = format!("{cell}/content/value/children/0");
    let paragraph = format!("{quote}/content/value/children/0");
    let inline = format!("{paragraph}/content/value");
    let attrs = format!("{table}/row_attrs/0/extension");
    for path in [
        String::new(),
        "/roots/0".into(),
        "/roots/0/kind".into(),
        "/roots/0/content".into(),
        table.into(),
        cell,
        quote,
        paragraph,
        inline.clone(),
        format!("{inline}/runs/0"),
        format!("{inline}/atoms/0"),
        attrs.clone(),
        format!("{attrs}/value/values"),
        format!("{attrs}/value/values/value/3"),
        format!("{attrs}/value/values/value/3/value/default"),
    ] {
        let mut changed = wire.clone();
        changed
            .pointer_mut(&path)
            .expect("existing DTO")
            .as_object_mut()
            .unwrap()
            .insert("future".into(), serde_json::json!({"keep": "me"}));
        assert!(
            decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
            "field at {path}"
        );
    }

    for mark in [
        serde_json::json!({"type":"bold"}),
        serde_json::json!({"type":"link","href":"https://example.invalid","title":null}),
    ] {
        let mut changed = wire.clone();
        *changed
            .pointer_mut(&format!("{inline}/runs/0/marks"))
            .unwrap() = serde_json::json!([mark]);
        assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_some());
        changed
            .pointer_mut(&format!("{inline}/runs/0/marks/0"))
            .unwrap()["future"] = true.into();
        assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_none());
    }
}

#[test]
fn duplicate_json_keys_cannot_overwrite_null_or_bypass_version_gates() {
    let slice =
        paragraph_slice(NodeAttrs::new([("textAlign".into(), AttrValue::Null)].into()).unwrap());
    let metadata = encode_metadata(&slice).unwrap();
    let null_attr = r#""textAlign":{"type":"null"}"#;
    assert!(metadata.contains(null_attr));
    for replacement in [
        r#""textAlign":{"type":"null"},"textAlign":{"type":"string","value":"left"}"#,
        r#""textAlign":{"type":"string","value":"left"},"textAlign":{"type":"null"}"#,
        r#""textAlign":{"type":"null"},"text\u0041lign":{"type":"string","value":"left"}"#,
    ] {
        let duplicate = metadata.replacen(null_attr, replacement, 1);
        // Keep this as raw JSON: a Value intermediary would erase duplicates.
        assert!(decode_metadata(slice.plain_text(), &duplicate).is_none());
        assert!(
            decode_metadata(
                slice.plain_text(),
                &duplicate.replacen("\"version\":7", "\"version\":4", 1)
            )
            .is_none()
        );
    }

    let nested = paragraph_slice(nested_null_attrs());
    let metadata = encode_metadata(&nested).unwrap();
    let duplicate = metadata.replacen(
        r#""default":{"type":"null"}"#,
        r#""default":{"type":"null"},"default":{"type":"bool","value":false}"#,
        1,
    );
    assert_ne!(duplicate, metadata);
    assert!(decode_metadata(nested.plain_text(), &duplicate).is_none());

    let (document, intro, cell) = table_fixture("atom");
    let atom_slice = copy_cell(&mut session(&document, intro), cell);
    let metadata = encode_metadata(&atom_slice).unwrap();
    let extension: serde_json::Value =
        serde_json::from_str(&encode_metadata(&nested).unwrap()).unwrap();
    let field = format!(
        "\"extension\":{}",
        extension["roots"][0]["attrs"]["extension"]
    );
    let duplicate = metadata.replacen(
        &field,
        &format!("{field},\"extension\":{{\"type\":\"bool\",\"value\":false}}"),
        1,
    );
    assert_ne!(duplicate, metadata);
    assert!(decode_metadata(atom_slice.plain_text(), &duplicate).is_none());

    // Valid legacy envelopes also reject ambiguous keys, independent of Null.
    let legacy = paragraph_slice(NodeAttrs::empty());
    let metadata = encode_metadata(&legacy).unwrap();
    let duplicate = metadata.replacen("\"version\":4", "\"version\":999,\"version\":4", 1);
    assert!(decode_metadata(legacy.plain_text(), &duplicate).is_none());
}

// Only the named role carries null, so each recursive path independently
// proves that it selects v7 instead of hiding under a different null value.
fn table_fixture(role: &str) -> (XiaomuDocument, NodeId, NodeId) {
    let attrs = |name| {
        if role == name {
            nested_null_attrs()
        } else {
            NodeAttrs::empty()
        }
    };
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, NodeAttrs::empty());
    let paragraph = paragraph(&mut builder, attrs("paragraph"));
    let quote = builder
        .insert(
            NodeKind::Quote,
            attrs("quote"),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let mut image_attrs = attrs("image")
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    image_attrs.insert(
        "src".into(),
        AttrValue::String("https://example.invalid/image.png".into()),
    );
    image_attrs.insert("alt".into(), AttrValue::String("image".into()));
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new(image_attrs).unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let cell = builder
        .insert(
            NodeKind::TableCell,
            attrs("cell"),
            NodeContent::children([quote, image]),
        )
        .unwrap();
    let row_attrs = if role == "legacy-row" {
        NodeAttrs::new([("legacy".into(), AttrValue::Bool(true))].into()).unwrap()
    } else {
        attrs("row")
    };
    let row = builder
        .insert(NodeKind::TableRow, row_attrs, NodeContent::children([cell]))
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            attrs("table"),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: point(&document, paragraph, "中".len()),
            kind: AtomKind::new("mention").unwrap(),
            attrs: attrs("atom"),
            content: InlineAtomContent::new("@name").unwrap(),
        })
        .apply(&document)
        .unwrap();
    (document, intro, cell)
}

fn copy_cell(session: &mut DocumentSession, cell: NodeId) -> ClipboardSlice {
    session.set_cell_range_selection(cell, cell).unwrap();
    session.clipboard_slice().unwrap().unwrap()
}

fn child(document: &XiaomuDocument, node: NodeId, index: usize) -> NodeId {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[index]
}

#[test]
fn null_on_every_clipboard_payload_path_survives_fresh_ids_undo_and_redo() {
    for role in [
        "table",
        "row",
        "cell",
        "quote",
        "paragraph",
        "atom",
        "image",
    ] {
        let (document, intro, cell) = table_fixture(role);
        let slice = copy_cell(&mut session(&document, intro), cell);
        let wire = assert_v7_round_trip(&slice);
        let decoded = decode_metadata(slice.plain_text(), &wire.to_string()).unwrap();
        let mut target = session(&document, intro);
        let before_selection = target.selection();
        target
            .apply_intent(&EditIntent::PasteSlice { slice: decoded })
            .unwrap();
        let pasted = target.document().clone();
        let pasted_selection = target.selection();
        let pasted_table = child(&pasted, pasted.root(), 1);
        let pasted_row = child(&pasted, pasted_table, 0);
        let pasted_cell = child(&pasted, pasted_row, 0);
        let pasted_quote = child(&pasted, pasted_cell, 0);
        let pasted_paragraph = child(&pasted, pasted_quote, 0);
        let pasted_image = child(&pasted, pasted_cell, 1);
        let pasted_atom = pasted
            .node(pasted_paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()[0]
            .atom();
        for id in [
            pasted_table,
            pasted_row,
            pasted_cell,
            pasted_quote,
            pasted_paragraph,
            pasted_image,
            pasted_atom,
        ] {
            assert!(document.node(id).is_none(), "fresh {role} payload identity");
        }
        assert_eq!(copy_cell(&mut target, pasted_cell), slice, "{role}");
        assert_eq!(target.history_depths(), (1, 0));
        target.undo().unwrap();
        assert_eq!(target.document().store(), document.store(), "{role}");
        assert_eq!(target.selection(), before_selection);
        target.redo().unwrap();
        assert_eq!(target.document().store(), pasted.store(), "{role}");
        assert_eq!(target.selection(), pasted_selection);
    }
}

#[test]
fn null_free_fragments_keep_v4_v5_v6_read_and_write_compatibility() {
    let plain = paragraph_slice(
        NodeAttrs::new([("legacy".into(), AttrValue::String("kept".into()))].into()).unwrap(),
    );
    for (slice, expected_version) in [
        (plain, 4),
        (
            {
                let (doc, intro, cell) = table_fixture("none");
                copy_cell(&mut session(&doc, intro), cell)
            },
            5,
        ),
        (
            {
                let (doc, intro, cell) = table_fixture("legacy-row");
                copy_cell(&mut session(&doc, intro), cell)
            },
            6,
        ),
    ] {
        let metadata = encode_metadata(&slice).unwrap();
        let wire: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(wire["version"], expected_version);
        assert_eq!(
            decode_metadata(slice.plain_text(), &metadata).as_ref(),
            Some(&slice)
        );
        // This is an intentional tightening of old readers, which ignored
        // extra DTO fields; valid legacy payloads above remain accepted.
        for path in ["", "/roots/0", "/roots/0/kind", "/roots/0/content"] {
            let mut changed = wire.clone();
            changed.pointer_mut(path).unwrap()["future"] = true.into();
            assert!(
                decode_metadata(slice.plain_text(), &changed.to_string()).is_none(),
                "v{expected_version}: {path}"
            );
        }
    }
}
