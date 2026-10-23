use super::format::write_node;
use super::*;

fn multiline_fixture() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("alpha\nbeta", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let code = builder
        .insert(
            NodeKind::CodeBlock,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(
                    "fn main() {\n    println!(\"ok\");\n}",
                    MarkSet::empty(),
                )
                .unwrap()])
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, code]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

#[test]
fn fixture_v2_round_trips_hard_breaks_and_code_newlines() {
    let document = multiline_fixture();
    let mut encoded = String::from("xiaomu-fixture-doc v2\n");
    write_node(&document, document.root(), &mut encoded).unwrap();

    assert!(encoded.contains("alpha\\nbeta"));
    assert!(encoded.contains("fn main() \\{\\n    println!"));
    let decoded = parse_document(&encoded).unwrap();
    assert!(canonical_semantics_equal(&document, &decoded));
}

#[test]
fn fixture_text_escape_preserves_lf_round_trip() {
    let source = "a\nb\n";
    assert_eq!(unescape_text(&escape_text(source)), source);
}

#[test]
fn fixture_v4_round_trips_inline_atom_chips_and_media() {
    let document = demo_fixture();
    let table = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .iter()
        .copied()
        .find(|id| document.node(*id).unwrap().kind() == &NodeKind::Table)
        .unwrap();
    let document = xiaomu_core::transaction::Transaction::new(
        xiaomu_core::transaction::TransactionOrigin::UserInput,
    )
    .with_step(xiaomu_core::transaction::TransactionStep::RemoveNode { node: table })
    .apply(&document)
    .unwrap();
    let mut encoded = String::from("xiaomu-fixture-doc v4\n");
    write_node(&document, document.root(), &mut encoded).unwrap();

    // The mention chip serializes as an atom token plus its atom line,
    // and the media blocks encode as atomic lines with preserved attrs
    // (including the unknown extension tag).
    assert!(encoded.contains("{a#0}"));
    assert!(encoded.contains("atom\tmention\t@xiaomu"));
    assert!(encoded.contains("handle=s:xiaomu"));
    assert!(encoded.contains("hr\n"));
    assert!(encoded.contains("img\n"));
    assert!(encoded.contains("alt=s:晓木封面"));
    assert!(encoded.contains("data-x-extension-tag=s:v1"));

    let decoded = parse_document(&encoded).unwrap();
    assert!(canonical_semantics_equal(&document, &decoded));
}

#[test]
fn fixture_still_refuses_custom_node_kinds() {
    let mut builder = NodeStoreBuilder::new();
    let custom = builder
        .insert(
            NodeKind::Custom("x-spoiler".to_owned()),
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([custom]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let error = write_node(&document, document.root(), &mut String::new()).unwrap_err();
    assert!(error.0.contains("x-spoiler"), "{error}");
}

#[test]
fn fixture_v3_rejects_references_to_undefined_atoms() {
    let encoded = "xiaomu-fixture-doc v3\np\t{a#0}\t\n";
    assert!(parse_document(encoded).is_err());
}

#[test]
fn fixture_v5_round_trips_real_table_and_existing_content() {
    let document = demo_fixture();
    let mut encoded = String::from("xiaomu-fixture-doc v5\n");
    super::format::write_node_v5(&document, document.root(), &mut encoded).unwrap();
    assert!(encoded.contains("table\t2\t2\n"));
    assert!(encoded.contains("atom\tmention\t@xiaomu"));
    assert!(canonical_semantics_equal(
        &document,
        &parse_document(&encoded).unwrap()
    ));
    assert!(write_node(&document, document.root(), &mut String::new()).is_err());
    for version in [2, 3, 4] {
        assert!(parse_document(&encoded.replacen("v5", &format!("v{version}"), 1)).is_err());
    }
}

#[test]
fn fixture_store_selects_version_and_reopens_from_disk() {
    let path = std::env::temp_dir().join(format!("xiaomu-p56-fixture-{}.txt", std::process::id()));
    let mut store = FixtureStore::new(path.clone());
    for (document, version) in [(demo_fixture(), "v5"), (multiline_fixture(), "v4")] {
        store.save(&document).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with(&format!("xiaomu-fixture-doc {version}\n"))
        );
        let reopened = FixtureStore::new(path.clone()).load().unwrap().unwrap();
        assert!(canonical_semantics_equal(&document, &reopened));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn fixture_v5_rejects_malformed_grid_and_unbalanced_frames() {
    let valid = "xiaomu-fixture-doc v5\ntable\t1\t1\ntrow\ntcell\np\tx\t\nend\nend\nend\n";
    assert!(parse_document(valid).is_ok());
    for invalid in [
        valid.replace("table\t1\t1", "table\t0\t1"),
        valid.replace("table\t1\t1", "table\t2\t1"),
        valid.replace("table\t1\t1", "table\t1\t2"),
        valid.replace("table\t1\t1", "table\t1\t1\t2"),
        valid.replace("p\tx\t\n", ""),
        valid.replace("trow\n", "quote\n"),
        valid.replace("tcell\n", "li\n"),
        valid.trim_end_matches("end\n").to_owned(),
        format!("{valid}end\n"),
    ] {
        assert!(parse_document(&invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn fixture_v5_preserves_table_row_cell_and_nested_block_attrs() {
    let text = "xiaomu-fixture-doc v5\n@\tid=s:table\ntable\t1\t1\n@\tid=s:row\ntrow\n@\tid=s:cell\ntcell\nquote\n@\tid=s:text\np\t中文 👩‍💻é\\n第二行\t\nend\nend\nend\nend\n";
    let document = parse_document(text).unwrap();
    let mut encoded = String::from("xiaomu-fixture-doc v5\n");
    super::format::write_node_v5(&document, document.root(), &mut encoded).unwrap();
    for value in ["table", "row", "cell", "text"] {
        assert!(encoded.contains(&format!("id=s:{value}")));
    }
    assert!(canonical_semantics_equal(
        &document,
        &parse_document(&encoded).unwrap()
    ));
}

#[test]
fn fixture_v3_reads_a_real_inline_atom_snapshot() {
    let text =
        "xiaomu-fixture-doc v3\np\t{a#0}\t你好\t\n@\thandle=s:xiaomu\natom\tmention\t@xiaomu\n";
    let document = parse_document(text).unwrap();
    let paragraph = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let inline = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(inline.atoms().len(), 1);
    assert_eq!(inline.runs()[0].text().as_str(), "你好");
    let atom = document.node(inline.atoms()[0].atom()).unwrap();
    assert_eq!(
        atom.kind(),
        &NodeKind::InlineAtom(AtomKind::new("mention").unwrap())
    );
    let mut encoded = String::from("xiaomu-fixture-doc v4\n");
    write_node(&document, document.root(), &mut encoded).unwrap();
    assert!(canonical_semantics_equal(
        &document,
        &parse_document(&encoded).unwrap()
    ));
}
