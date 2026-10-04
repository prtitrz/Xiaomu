//! Exact typed task clipboard source projection and v12 encoding.

mod task_list_support;
use task_list_support::*;

use serde_json::json;
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

#[test]
fn open_and_closed_task_fragments_preserve_each_checked_state_without_normalization() {
    for value in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ] {
        let (document, first, tail) = fixture(value.clone());
        for closed in [false, true] {
            let slice = copy(&document, first, tail, closed);
            assert_eq!(slice.plain_text(), "task中🙂\ncode\n\ntail");
            assert_eq!(slice.roots()[0].kind(), &NodeKind::TaskList);
            let item = &slice.roots()[0].content().as_children().unwrap()[0];
            assert_eq!(item.kind(), &NodeKind::TaskItem);
            assert_eq!(item.attrs().get("checked"), value.as_ref());
            let wire = roundtrip(&slice);
            assert_eq!(wire["roots"][0]["kind"], json!({"type":"task_list"}));
            assert_eq!(
                wire["roots"][0]["content"]["value"]["children"][0]["kind"],
                json!({"type":"task_item"})
            );
            assert_eq!(document.revision().as_u64(), 0);
        }
    }
}

#[test]
fn mixed_task_and_ordinary_nesting_preserves_all_attrs_code_images_and_break_marks() {
    let rich = NodeAttrs::new(
        [
            ("null".into(), AttrValue::Null),
            ("bool".into(), AttrValue::Bool(false)),
            ("integer".into(), AttrValue::Integer(-5)),
            ("string".into(), AttrValue::String("值🙂".into())),
            (
                "object".into(),
                AttrValue::Object(
                    [(
                        "list".into(),
                        AttrValue::List(vec![AttrValue::Null, AttrValue::Bool(true)]),
                    )]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
    .unwrap();
    let mut builder = NodeStoreBuilder::new();
    let marks = MarkSet::new([Mark::Bold, Mark::Italic]).unwrap();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break().with_marks(marks.clone())),
        )
        .unwrap();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            rich.clone(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("task🙂", marks.clone()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let code = builder
        .insert(
            NodeKind::CodeBlock,
            rich.clone(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("a\nb", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let mut image_values = rich
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    image_values.insert(
        "src".into(),
        AttrValue::String("https://example.invalid/task.png".into()),
    );
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new(image_values).unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let nested_text = text(&mut builder, NodeKind::Paragraph, "nested");
    let nested_task = builder
        .insert(
            NodeKind::TaskItem,
            attrs(Some(AttrValue::Null)),
            NodeContent::children([nested_text]),
        )
        .unwrap();
    let nested_list = container(&mut builder, NodeKind::TaskList, vec![nested_task]);
    let ordinary_text = text(&mut builder, NodeKind::Paragraph, "ordinary");
    let ordinary_item = container(
        &mut builder,
        NodeKind::ListItem,
        vec![ordinary_text, nested_list],
    );
    let ordinary_list = container(&mut builder, NodeKind::OrderedList, vec![ordinary_item]);
    let task = builder
        .insert(
            NodeKind::TaskItem,
            rich.clone(),
            NodeContent::children([first, code, image, ordinary_list]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::TaskList,
            rich.clone(),
            NodeContent::children([task]),
        )
        .unwrap();
    let quote = container(&mut builder, NodeKind::Quote, vec![list]);
    let outer_item = container(&mut builder, NodeKind::ListItem, vec![quote]);
    let outer_list = container(&mut builder, NodeKind::BulletList, vec![outer_item]);
    let root = container(&mut builder, NodeKind::Document, vec![outer_list]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let slice = copy(&document, first, nested_text, true);
    let wire = roundtrip(&slice);
    assert_eq!(
        slice.blocks()[0].inline().atoms()[0].kind(),
        &AtomKind::hard_break()
    );
    assert_eq!(
        slice.blocks()[0].inline().atoms()[0].content().marks(),
        &marks
    );
    assert_eq!(slice.blocks()[0].attrs(), &rich);
    assert_eq!(slice.blocks()[1].kind(), &NodeKind::CodeBlock);
    assert_eq!(slice.blocks()[1].inline().text(), "a\nb");
    assert_eq!(slice.blocks()[1].attrs(), &rich);
    assert!(
        wire.to_string()
            .contains("https://example.invalid/task.png")
    );
    assert_eq!(
        slice.plain_text(),
        "\ntask🙂\na\nb\nordinary\nnested\nhttps://example.invalid/task.png"
    );
}

#[test]
fn task_detection_reaches_rectangular_table_cell_payloads_and_closed_tables() {
    let mut builder = NodeStoreBuilder::new();
    let first = text(&mut builder, NodeKind::Paragraph, "cell task");
    let item = builder
        .insert(
            NodeKind::TaskItem,
            attrs(Some(AttrValue::Bool(true))),
            NodeContent::children([first]),
        )
        .unwrap();
    let list = container(&mut builder, NodeKind::TaskList, vec![item]);
    let quote = container(&mut builder, NodeKind::Quote, vec![list]);
    let cell = container(&mut builder, NodeKind::TableCell, vec![quote]);
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::new([("row".into(), AttrValue::Null)].into()).unwrap(),
            NodeContent::children([cell]),
        )
        .unwrap();
    let table = container(&mut builder, NodeKind::Table, vec![row]);
    let root = container(&mut builder, NodeKind::Document, vec![table]);
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(first)),
    )
    .unwrap();
    session.set_cell_range_selection(cell, cell).unwrap();
    let rectangular = session.clipboard_slice().unwrap().unwrap();
    assert!(!rectangular.is_closed());
    assert_eq!(rectangular.plain_text(), "cell task");
    for slice in [rectangular, copy(&document, first, first, true)] {
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
fn pruned_task_retains_its_wrapper_without_imposing_a_host_schema() {
    for value in [
        None,
        Some(AttrValue::Null),
        Some(AttrValue::Bool(false)),
        Some(AttrValue::Bool(true)),
    ] {
        let (document, first, tail) = fixture(value.clone());
        let item = document.parent_of(first).unwrap();
        let code = document
            .node(item)
            .unwrap()
            .content()
            .as_children()
            .unwrap()[1];
        let code_start = xiaomu_core::selection::InlinePoint::new(
            code,
            document
                .node(code)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at(1)
                .unwrap(),
            0,
            xiaomu_core::selection::CursorAffinity::Before,
        );
        let session = DocumentSession::new(
            document.clone(),
            DocumentSelection::new(code_start, end(&document, tail)),
        )
        .unwrap();
        let pruned = session.clipboard_slice().unwrap().unwrap();
        roundtrip(&pruned);
        let task = &pruned.roots()[0].content().as_children().unwrap()[0];
        assert_eq!(task.kind(), &NodeKind::TaskItem);
        assert_eq!(task.attrs().get("checked"), value.as_ref());
        assert_eq!(
            task.content().as_children().unwrap()[0].kind(),
            &NodeKind::CodeBlock
        );
        assert_eq!(pruned.plain_text(), "ode\n\ntail");
        assert_eq!(session.document().store(), document.store());
        // A text-only selection within that code leaf retains the existing v4 contract.
        let leaf_session = DocumentSession::new(
            document.clone(),
            DocumentSelection::new(InlinePoint::at_start_of(code), end(&document, code)),
        )
        .unwrap();
        let leaf = leaf_session.clipboard_slice().unwrap().unwrap();
        assert_eq!(leaf.roots()[0].kind(), &NodeKind::CodeBlock);
        assert!(encode_metadata(&leaf).unwrap().contains("\"version\":4"));
    }
}

#[test]
fn legacy_non_task_bytes_and_single_leaf_copy_remain_unchanged() {
    let (document, first, tail) = fixture(None);
    for node in [first, tail] {
        let session = DocumentSession::new(
            document.clone(),
            DocumentSelection::new(InlinePoint::at_start_of(node), end(&document, node)),
        )
        .unwrap();
        let slice = session.clipboard_slice().unwrap().unwrap();
        let metadata = encode_metadata(&slice).unwrap();
        assert!(metadata.contains("\"version\":4"));
        assert!(!metadata.contains("\"closed\""));
        assert_eq!(decode_metadata(slice.plain_text(), &metadata), Some(slice));
    }
    let tail_slice = DocumentSession::new(
        document.clone(),
        DocumentSelection::new(InlinePoint::at_start_of(tail), end(&document, tail)),
    )
    .unwrap()
    .clipboard_slice()
    .unwrap()
    .unwrap();
    assert_eq!(
        encode_metadata(&tail_slice).unwrap(),
        r#"{"format":"xiaomu.clipboard","version":4,"roots":[{"kind":{"type":"paragraph"},"attrs":{},"content":{"type":"inline","value":{"runs":[{"text":"tail","marks":[]}]}}}]}"#
    );
}
