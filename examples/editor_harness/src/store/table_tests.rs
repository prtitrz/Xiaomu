use super::*;

#[test]
fn fixture_v5_preserves_rich_nested_tables_and_all_scalar_attrs() {
    let original = demo_fixture();
    let mut text = "xiaomu-fixture-doc v5\n".to_owned();
    write_node(&original, original.root(), &mut text).unwrap();
    assert!(text.contains("table\n"));
    assert!(text.contains("row\n"));
    assert!(text.contains("cell\n"));
    assert!(text.contains("extension-tag=s:nested-table"));
    assert!(text.contains("extension-tag=s:row-2"));
    assert!(text.contains("extension-tag=s:b2"));
    assert!(canonical_semantics_equal(
        &original,
        &parse_document(&text).unwrap()
    ));
    for version in [2, 3, 4] {
        assert!(parse_document(&text.replacen("v5", &format!("v{version}"), 1)).is_err());
    }
}

#[test]
fn legacy_fixtures_still_load_and_tables_are_validated_without_redundant_dimensions() {
    for version in [2, 3, 4, 5] {
        assert!(parse_document(&format!("xiaomu-fixture-doc v{version}\np\t中文🙂\t\n")).is_ok());
    }
    for body in [
        "table\nend\n",
        "table\nrow\ncell\nend\nend\nend\n",
        "table\np\twrong child\t\nend\n",
        "table\nrow\ncell\np\ta\t\nend\nend\nrow\ncell\np\tb\t\nend\ncell\np\tc\t\nend\nend\nend\n",
        "table\t2\t2\nend\n",
    ] {
        assert!(
            parse_document(&format!("xiaomu-fixture-doc v5\n{body}")).is_err(),
            "{body}"
        );
    }
}
