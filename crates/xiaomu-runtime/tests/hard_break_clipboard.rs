//! Exact v10 atom identity and marks, with unchanged legacy wire representations.

use serde_json::{Value, json};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes, LinkMark,
    Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, StringAttribute,
    TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn document(
    text: &str,
    atoms: Vec<(usize, AtomKind, InlineAtomContent)>,
) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let buffer = TextBuffer::from_string(text.to_owned());
    let placements = atoms
        .into_iter()
        .map(|(offset, kind, content)| {
            let atom = builder
                .insert(
                    NodeKind::InlineAtom(kind),
                    NodeAttrs::empty(),
                    NodeContent::InlineAtom(content),
                )
                .unwrap();
            InlineAtomPlacement::new(atom, buffer.offset_at(offset).unwrap())
        })
        .collect::<Vec<_>>();
    let runs = if text.is_empty() {
        Vec::new()
    } else {
        vec![TextRun::new(text, MarkSet::empty()).unwrap()]
    };
    let leaf = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([leaf]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), leaf)
}

fn copy(document: XiaomuDocument, leaf: NodeId) -> ClipboardSlice {
    let inline = document.node(leaf).unwrap().content().as_inline().unwrap();
    let end = inline.offset_at(inline.len_bytes()).unwrap();
    let ordinal = inline
        .atoms()
        .iter()
        .filter(|atom| atom.text_offset() == end)
        .count();
    let selection = DocumentSelection::new(
        InlinePoint::new(leaf, TextOffset::ZERO, 0, CursorAffinity::Before),
        InlinePoint::new(leaf, end, ordinal, CursorAffinity::After),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn break_slice(marks: MarkSet) -> ClipboardSlice {
    let (doc, leaf) = document(
        "",
        vec![(
            0,
            AtomKind::hard_break(),
            InlineAtomContent::hard_break().with_marks(marks),
        )],
    );
    copy(doc, leaf)
}

fn all_marks() -> MarkSet {
    MarkSet::new([
        Mark::Bold,
        Mark::Italic,
        Mark::Code,
        Mark::Underline,
        Mark::Strike,
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href(StringAttribute::Null)
                .with_target("".into())
                .with_rel("noreferrer 未解析🙂".into())
                .with_title("title".into()),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color("var(--未知)".into())
                .with_font_family(StringAttribute::Null),
        )),
    ])
    .unwrap()
}

fn roundtrip(slice: &ClipboardSlice, version: u32) -> Value {
    let metadata = encode_metadata(slice).unwrap();
    let wire: Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(wire["version"], version);
    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(&decoded, slice);
    assert_eq!(encode_metadata(&decoded).unwrap(), metadata);
    assert!(decode_metadata("stale", &metadata).is_none());
    wire
}

const ATOM: &str = "/roots/0/content/value/atoms/0";

#[test]
fn literal_lf_and_typed_breaks_keep_distinct_identity_order_and_independent_marks() {
    let marks = all_marks();
    let text = "中\n🙂";
    let (doc, leaf) = document(
        text,
        vec![
            (0, AtomKind::hard_break(), InlineAtomContent::hard_break()),
            (
                3,
                AtomKind::hard_break(),
                InlineAtomContent::hard_break().with_marks(marks.clone()),
            ),
            (3, AtomKind::hard_break(), InlineAtomContent::hard_break()),
            (
                4,
                AtomKind::new("hardBreak").unwrap(),
                InlineAtomContent::new("\n").unwrap(),
            ),
            (
                text.len(),
                AtomKind::hard_break(),
                InlineAtomContent::hard_break(),
            ),
        ],
    );
    let slice = copy(doc, leaf);
    assert_eq!(slice.plain_text(), "\n中\n\n\n\n🙂\n");
    assert_eq!(slice.blocks()[0].inline().text(), text);
    assert!(slice.blocks()[0].inline().runs()[0].marks().is_empty());
    let wire = roundtrip(&slice, 10);
    let atoms = &slice.blocks()[0].inline().atoms();
    assert_eq!(atoms[1].content().marks(), &marks);
    assert!(atoms[2].content().marks().is_empty());
    assert_ne!(atoms[0].kind(), atoms[3].kind());
    assert_eq!(atoms[0].kind().as_str(), atoms[3].kind().as_str());
    let wire_atoms = &wire["roots"][0]["content"]["value"]["atoms"];
    assert_eq!(wire_atoms[0]["kind"], json!({"type":"hard_break"}));
    assert_eq!(wire_atoms[0]["marks"], json!([]));
    assert_eq!(wire_atoms[3]["kind"], "hardBreak");
    assert!(wire_atoms[3].get("marks").is_none());
}

#[test]
fn only_consecutive_breaks_keep_ordinal_selection_and_lf_fallback() {
    for count in 1..=3 {
        let (doc, leaf) = document(
            "",
            (0..count)
                .map(|_| (0, AtomKind::hard_break(), InlineAtomContent::hard_break()))
                .collect(),
        );
        let slice = copy(doc.clone(), leaf);
        assert_eq!(slice.plain_text(), "\n".repeat(count));
        assert!(slice.blocks()[0].inline().runs().is_empty());
        roundtrip(&slice, 10);
        let selection = DocumentSelection::new(
            InlinePoint::new(leaf, TextOffset::ZERO, count - 1, CursorAffinity::Before),
            InlinePoint::new(leaf, TextOffset::ZERO, count, CursorAffinity::Before),
        );
        let selected = DocumentSession::new(doc, selection)
            .unwrap()
            .clipboard_slice()
            .unwrap()
            .unwrap();
        assert_eq!(selected.plain_text(), "\n");
        assert_eq!(selected.blocks()[0].inline().atoms().len(), 1);
        roundtrip(&selected, 10);
    }
}

#[test]
fn marked_extensions_use_v10_without_becoming_builtins() {
    for name in ["hardBreak", "mention", " extension🙂 "] {
        let kind = AtomKind::new(name).unwrap();
        let marks = all_marks();
        let (doc, leaf) = document(
            "x",
            vec![(
                0,
                kind.clone(),
                InlineAtomContent::new("\n")
                    .unwrap()
                    .with_marks(marks.clone()),
            )],
        );
        let slice = copy(doc, leaf);
        let wire = roundtrip(&slice, 10);
        assert_eq!(
            wire.pointer(ATOM).unwrap()["kind"],
            json!({"type":"extension", "value": name})
        );
        for version in 4..=9 {
            let mut old = wire.clone();
            old["version"] = version.into();
            assert!(decode_metadata(slice.plain_text(), &old.to_string()).is_none());
        }
        assert_eq!(slice.blocks()[0].inline().atoms()[0].kind(), &kind);
        assert!(!slice.blocks()[0].inline().atoms()[0].kind().is_hard_break());
        assert_eq!(
            slice.blocks()[0].inline().atoms()[0].content().marks(),
            &marks
        );
    }
}

#[test]
fn all_link_and_style_attribute_states_survive_nested_atom_marks() {
    let states = [
        StringAttribute::Missing,
        StringAttribute::Null,
        "".into(),
        "uninterpreted🙂 value".into(),
    ];
    for mut code in 0..4usize.pow(5) {
        let values: [_; 5] = std::array::from_fn(|_| {
            let value = states[code % 4].clone();
            code /= 4;
            value
        });
        let [href, target, rel, class, title] = values;
        let mark = Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href(href)
                .with_target(target)
                .with_rel(rel)
                .with_class(class)
                .with_title(title),
        ));
        roundtrip(&break_slice(MarkSet::new([mark]).unwrap()), 10);
    }
    for mut code in 0..4usize.pow(3) {
        let values: [_; 3] = std::array::from_fn(|_| {
            let value = states[code % 4].clone();
            code /= 4;
            value
        });
        let [color, font_family, font_size] = values;
        let mark = Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(color)
                .with_font_family(font_family)
                .with_font_size(font_size),
        ));
        roundtrip(&break_slice(MarkSet::new([mark]).unwrap()), 10);
    }
}

#[test]
fn v10_is_required_for_each_new_atom_shape_even_when_marks_are_empty() {
    for marks in [MarkSet::empty(), all_marks()] {
        let slice = break_slice(marks);
        let original = roundtrip(&slice, 10);
        for version in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 999] {
            let mut changed = original.clone();
            changed["version"] = version.into();
            assert!(decode_metadata(slice.plain_text(), &changed.to_string()).is_none());
        }
    }
    let (doc, leaf) = document(
        "",
        vec![(
            0,
            AtomKind::new("hardBreak").unwrap(),
            InlineAtomContent::new("\n").unwrap(),
        )],
    );
    let legacy = copy(doc, leaf);
    let original = roundtrip(&legacy, 4);
    for version in 4..=10 {
        let mut changed = original.clone();
        changed["version"] = version.into();
        assert_eq!(
            decode_metadata("\n", &changed.to_string()),
            Some(legacy.clone())
        );
        changed.pointer_mut(ATOM).unwrap()["marks"] = json!([]);
        assert!(decode_metadata("\n", &changed.to_string()).is_none());
    }
}

#[test]
fn legacy_v4_through_v9_encode_to_the_identical_bytes() {
    let leaf = |marks: &str, attrs: &str| {
        format!(
            r#"{{"kind":{{"type":"paragraph"}},"attrs":{attrs},"content":{{"type":"inline","value":{{"runs":[{{"text":"x","marks":{marks}}}],"atoms":[{{"anchor":0,"kind":"hardBreak","attrs":{{}},"fallback":"\n"}}]}}}}}}"#
        )
    };
    let plain = leaf("[]", "{}");
    let table = |leaf: &str, row_attrs: &str| {
        format!(
            r#"{{"kind":{{"type":"table"}},"attrs":{{}},"content":{{"type":"table","value":{{"rows":[[{{"kind":{{"type":"table_cell"}},"attrs":{{}},"content":{{"type":"children","value":{{"children":[{leaf}]}}}}}}]]{row_attrs}}}}}}}"#
        )
    };
    let row = r#","row_attrs":[{"row":{"type":"string","value":"r"}}]"#;
    let null = leaf("[]", r#"{"nullable":{"type":"null"}}"#);
    let link = leaf(
        r#"[{"type":"link_attributes","attrs":{"href":{"type":"null"},"target":{"type":"missing"},"rel":{"type":"missing"},"class":{"type":"missing"},"title":{"type":"missing"}}}]"#,
        "{}",
    );
    let style = leaf(
        r#"[{"type":"text_style","attrs":{"color":{"type":"missing"},"font_family":{"type":"null"},"font_size":{"type":"string","value":""}}}]"#,
        "{}",
    );
    for (version, root, fallback) in [
        (4, plain.clone(), "\nx"),
        (5, table(&plain, ""), " x"),
        (6, table(&plain, row), " x"),
        (7, null, "\nx"),
        (8, link, "\nx"),
        (9, style, "\nx"),
    ] {
        let metadata =
            format!(r#"{{"format":"xiaomu.clipboard","version":{version},"roots":[{root}]}}"#);
        let decoded = decode_metadata(fallback, &metadata).unwrap();
        assert_eq!(encode_metadata(&decoded).unwrap(), metadata, "v{version}");
    }
}

#[test]
fn copy_wire_rebuild_restores_exact_payloads_under_new_identities_and_undo() {
    let marks = all_marks();
    let (source, leaf) = document(
        "a\nb",
        vec![
            (
                1,
                AtomKind::hard_break(),
                InlineAtomContent::hard_break().with_marks(marks.clone()),
            ),
            (
                1,
                AtomKind::new("hardBreak").unwrap(),
                InlineAtomContent::new("\n").unwrap().with_marks(marks),
            ),
        ],
    );
    let slice = copy(source, leaf);
    let encoded = encode_metadata(&slice).unwrap();
    let slice = decode_metadata(slice.plain_text(), &encoded).unwrap();
    let expected = slice.blocks()[0].inline().atoms().to_vec();
    let (target, leaf) = document("target", vec![]);
    let original = target.clone();
    let mut session = DocumentSession::new(
        target,
        DocumentSelection::collapsed(InlinePoint::new(
            leaf,
            TextOffset::ZERO,
            0,
            CursorAffinity::Before,
        )),
    )
    .unwrap();
    for _ in 0..2 {
        session
            .apply_intent(&EditIntent::PasteSlice {
                slice: slice.clone(),
            })
            .unwrap();
    }
    let pasted = session.document().clone();
    let inline = pasted.node(leaf).unwrap().content().as_inline().unwrap();
    let placements = inline.atoms();
    assert_eq!(placements.len(), 4);
    for (index, placement) in placements.iter().enumerate() {
        for earlier in &placements[..index] {
            assert_ne!(placement.atom(), earlier.atom());
        }
        let node = pasted.node(placement.atom()).unwrap();
        let expected = &expected[index % 2];
        assert_eq!(node.kind(), &NodeKind::InlineAtom(expected.kind().clone()));
        assert_eq!(node.attrs(), expected.attrs());
        assert_eq!(node.content().as_inline_atom().unwrap(), expected.content());
    }
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
    session.redo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.document().store(), pasted.store());
}

#[test]
fn plain_lf_fallback_alone_cannot_reconstruct_atom_identity_or_marks() {
    let rich = break_slice(all_marks());
    let (doc, leaf) = document("\n", vec![]);
    let plain = copy(doc, leaf);
    assert_eq!(rich.plain_text(), plain.plain_text());
    assert_ne!(rich, plain);
    assert!(!plain.blocks()[0].inline().runs().is_empty());
    assert!(plain.blocks()[0].inline().atoms().is_empty());
    assert!(decode_metadata(rich.plain_text(), "").is_none());
    roundtrip(&plain, 4);
}

#[test]
fn malformed_typed_atoms_reject_the_whole_slice_without_coercing_defaults() {
    let slice = break_slice(all_marks());
    let original = roundtrip(&slice, 10);
    for field in ["anchor", "kind", "attrs", "fallback", "marks"] {
        let mut wire = original.clone();
        wire.pointer_mut(ATOM)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            decode_metadata("\n", &wire.to_string()).is_none(),
            "missing {field}"
        );
        let mut wire = original.clone();
        wire.pointer_mut(ATOM).unwrap()[field] = Value::Null;
        assert!(
            decode_metadata("\n", &wire.to_string()).is_none(),
            "null {field}"
        );
    }
    for (field, value) in [
        ("anchor", json!(1)),
        ("anchor", json!(-1)),
        ("anchor", json!(0.5)),
        ("anchor", json!("0")),
        ("kind", json!("hardBreak")),
        ("kind", json!({"type":"unknown"})),
        ("kind", json!({"type":"hard_break", "value":"hardBreak"})),
        ("kind", json!({"type":"extension"})),
        ("kind", json!({"type":"extension", "value":null})),
        ("kind", json!({"type":"extension", "value":" "})),
        ("attrs", json!({"unexpected":{"type":"null"}})),
        ("fallback", json!("")),
        ("fallback", json!("\r\n")),
        ("fallback", json!("not a break")),
        ("marks", json!({})),
        ("marks", json!([null])),
        ("marks", json!([{"type":"future"}])),
        ("marks", json!([{"type":"bold", "value":true}])),
    ] {
        let mut wire = original.clone();
        wire.pointer_mut(ATOM).unwrap()[field] = value;
        assert!(
            decode_metadata("\n", &wire.to_string()).is_none(),
            "invalid {field}"
        );
    }
    for path in [ATOM.to_owned(), format!("{ATOM}/kind")] {
        let mut wire = original.clone();
        wire.pointer_mut(&path).unwrap()["future"] = json!(true);
        assert!(decode_metadata("\n", &wire.to_string()).is_none());
    }
    // An offset inside a multi-byte scalar is invalid even if in byte bounds.
    let (doc, leaf) = document(
        "🙂",
        vec![(0, AtomKind::hard_break(), InlineAtomContent::hard_break())],
    );
    let slice = copy(doc, leaf);
    let mut wire = roundtrip(&slice, 10);
    wire.pointer_mut(ATOM).unwrap()["anchor"] = json!(1);
    assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
}

#[test]
fn exact_link_and_style_slots_are_as_strict_inside_atoms_as_in_runs() {
    let original = roundtrip(&break_slice(all_marks()), 10);
    let atom_marks = format!("{ATOM}/marks");
    for (index, fields) in [
        (5, &["href", "target", "rel", "class", "title"][..]),
        (6, &["color", "font_family", "font_size"][..]),
    ] {
        let mark_path = format!("{atom_marks}/{index}");
        let attrs_path = format!("{mark_path}/attrs");
        for path in [&mark_path, &attrs_path] {
            let mut wire = original.clone();
            wire.pointer_mut(path).unwrap()["future"] = json!(true);
            assert!(decode_metadata("\n", &wire.to_string()).is_none());
        }
        for field in fields {
            let mut wire = original.clone();
            wire.pointer_mut(&attrs_path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(*field);
            assert!(decode_metadata("\n", &wire.to_string()).is_none());
            for invalid in [
                json!(null),
                json!(false),
                json!(42),
                json!(""),
                json!([]),
                json!({"type":"unknown"}),
                json!({"type":"string"}),
                json!({"type":"string","value":null}),
                json!({"type":"string","value":42}),
                json!({"type":"missing","value":"hidden"}),
                json!({"type":"null","value":"hidden"}),
                json!({"type":"missing","future":true}),
            ] {
                let mut wire = original.clone();
                wire.pointer_mut(&attrs_path).unwrap()[*field] = invalid;
                assert!(decode_metadata("\n", &wire.to_string()).is_none());
            }
        }
        let mut wire = original.clone();
        let mut conflict = original.pointer(&mark_path).unwrap().clone();
        conflict["attrs"][fields[0]] = json!({"type":"string","value":"conflict"});
        wire.pointer_mut(&atom_marks)
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(conflict);
        assert!(decode_metadata("\n", &wire.to_string()).is_none());
    }
    // Canonical MarkSet continues to normalize identical marks; JSON field
    // duplication is a different ambiguity and is never normalized away.
    let mut wire = original.clone();
    wire.pointer_mut(&atom_marks)
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"bold"}));
    assert_eq!(
        decode_metadata("\n", &wire.to_string()),
        Some(break_slice(all_marks()))
    );
}

#[test]
fn duplicate_keys_reject_before_derive_can_overwrite_typed_semantics() {
    let metadata = encode_metadata(&break_slice(all_marks())).unwrap();
    for (from, to) in [
        ("\"version\":10", "\"version\":9,\"version\":10"),
        ("\"anchor\":0", "\"anchor\":1,\"anchor\":0"),
        (
            "\"type\":\"hard_break\"",
            "\"type\":\"extension\",\"type\":\"hard_break\"",
        ),
        ("\"fallback\":", "\"fallback\":\"hidden\",\"fallback\":"),
        ("\"marks\":[{", "\"marks\":null,\"marks\":[{"),
        (
            "\"href\":",
            "\"href\":{\"type\":\"missing\"},\"hr\\u0065f\":",
        ),
        (
            "\"color\":",
            "\"color\":{\"type\":\"missing\"},\"col\\u006fr\":",
        ),
        ("\"value\":\"\"", "\"value\":null,\"value\":\"\""),
    ] {
        assert!(metadata.contains(from), "{from}");
        assert!(decode_metadata("\n", &metadata.replacen(from, to, 1)).is_none());
    }
    let (doc, leaf) = document(
        "",
        vec![(
            0,
            AtomKind::new("mention").unwrap(),
            InlineAtomContent::new("x").unwrap().with_marks(all_marks()),
        )],
    );
    let mut wire = roundtrip(&copy(doc, leaf), 10);
    wire.pointer_mut(ATOM).unwrap()["attrs"] =
        json!({"custom":{"type":"object","value":{"key":{"type":"null"}}}});
    let metadata = wire.to_string();
    assert!(decode_metadata("x", &metadata).is_some());
    let duplicate = metadata.replacen(
        "\"key\":",
        "\"key\":{\"type\":\"bool\",\"value\":false},\"k\\u0065y\":",
        1,
    );
    assert_ne!(metadata, duplicate);
    assert!(decode_metadata("x", &duplicate).is_none());
}

#[test]
fn typed_atoms_nested_in_containers_and_tables_keep_v10_and_version_guards() {
    let slice = break_slice(all_marks());
    let wire = roundtrip(&slice, 10);
    let leaf = wire["roots"][0].clone();
    let quote = json!({"kind":{"type":"quote"},"attrs":{},"content":{"type":"children","value":{"children":[leaf]}}});
    let table = json!({"kind":{"type":"table"},"attrs":{},"content":{"type":"table","value":{"rows":[[{"kind":{"type":"table_cell"},"attrs":{},"content":{"type":"children","value":{"children":[quote.clone()]}}}]]}}});
    for (root, fallback) in [(quote, "\n"), (table, " ")] {
        let mut nested = wire.clone();
        nested["roots"] = json!([root]);
        let decoded = decode_metadata(fallback, &nested.to_string()).unwrap();
        roundtrip(&decoded, 10);
        for version in 4..=9 {
            nested["version"] = version.into();
            assert!(decode_metadata(fallback, &nested.to_string()).is_none());
        }
    }
}

#[test]
fn metadata_byte_value_and_depth_budgets_fail_closed_for_atom_marks_and_legacy() {
    let source = break_slice(all_marks());
    let original = roundtrip(&source, 10);
    // Atom marks share the same byte budget as text-run marks. The string is
    // otherwise a valid, uninterpreted canonical TextStyle attribute.
    let huge = "x".repeat(16 * 1024 * 1024);
    let mark = Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_color(huge.clone().into()),
    ));
    assert!(encode_metadata(&break_slice(MarkSet::new([mark]).unwrap())).is_err());
    let mut oversized = original.clone();
    oversized
        .pointer_mut(&format!("{ATOM}/marks/6/attrs/color"))
        .unwrap()["value"] = huge.into();
    assert!(decode_metadata("\n", &oversized.to_string()).is_none());

    // Identical marks normalize in Core, but do not evade the input-value
    // budget through repetition before normalization.
    let mut repeated = original.clone();
    repeated.pointer_mut(ATOM).unwrap()["marks"] = json!(vec![json!({"type":"bold"}); 50_001]);
    assert!(decode_metadata("\n", &repeated.to_string()).is_none());
    let (doc, leaf) = document("x", vec![]);
    let mut legacy = roundtrip(&copy(doc, leaf), 4);
    legacy["roots"][0]["content"]["value"]["runs"][0]["marks"] =
        json!(vec![json!({"type":"bold"}); 50_001]);
    assert!(decode_metadata("x", &legacy.to_string()).is_none());

    let mut deep = json!({"type":"null"});
    for _ in 0..130 {
        deep = json!({"type":"list","value":[deep]});
    }
    let mut nested = original.clone();
    nested.pointer_mut(ATOM).unwrap()["kind"] = json!({"type":"extension","value":"mention"});
    nested.pointer_mut(ATOM).unwrap()["attrs"] = json!({"deep":deep});
    assert!(decode_metadata("\n", &nested.to_string()).is_none());
}
