//! Explicit clipboard-to-template cloning does not change paste fitting.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use xiaomu_core::Error;
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
    LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{
    TableTreeTemplate, Transaction, TransactionOrigin, TransactionStep,
};
use xiaomu_runtime::clipboard::{ClipboardSlice, decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent, SessionError};

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn block(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    attrs: NodeAttrs,
    content: NodeContent,
) -> NodeId {
    builder.insert(kind, attrs, content).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    block(
        builder,
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    )
}

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let outside = paragraph(&mut builder, "outside");
    let link = Mark::Link(LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(StringAttribute::Value("https://example.test/".into()))
            .with_target(StringAttribute::Null)
            .with_rel(StringAttribute::Value("nofollow custom".into()))
            .with_class(StringAttribute::Value("opaque-link".into())),
    ));
    let style = Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(StringAttribute::Value("var(--ink)".into()))
            .with_font_family(StringAttribute::Null)
            .with_font_size(StringAttribute::Value("calc(1em + 2px)".into())),
    ));
    let hard_break = block(
        &mut builder,
        NodeKind::InlineAtom(AtomKind::hard_break()),
        NodeAttrs::empty(),
        NodeContent::InlineAtom(
            InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Bold]).unwrap()),
        ),
    );
    let mention = block(
        &mut builder,
        NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
        attrs(&[(
            "payload",
            AttrValue::Object([("nullable".into(), AttrValue::Null)].into()),
        )]),
        NodeContent::InlineAtom(
            InlineAtomContent::new("@中")
                .unwrap()
                .with_marks(MarkSet::new([link.clone(), style.clone()]).unwrap()),
        ),
    );
    let mixed = block(
        &mut builder,
        NodeKind::Paragraph,
        attrs(&[("alignment", AttrValue::Null)]),
        NodeContent::Inline(
            InlineContent::with_atoms(
                [
                    TextRun::new("中🙂", MarkSet::new([Mark::Italic, link]).unwrap()).unwrap(),
                    TextRun::new("tail", MarkSet::new([style]).unwrap()).unwrap(),
                ],
                [
                    InlineAtomPlacement::new(hard_break, TextOffset::ZERO),
                    InlineAtomPlacement::new(mention, TextOffset::ZERO),
                ],
            )
            .unwrap(),
        ),
    );
    let image = block(
        &mut builder,
        NodeKind::Image,
        attrs(&[
            ("asset", AttrValue::String("opaque-image-asset".into())),
            ("alt", AttrValue::String("图🙂".into())),
        ]),
        NodeContent::Atomic,
    );
    let nested_text = paragraph(&mut builder, "nested");
    let nested_cell = block(
        &mut builder,
        NodeKind::TableCell,
        attrs(&[("colwidth", AttrValue::Null)]),
        NodeContent::children([nested_text]),
    );
    let nested_row = block(
        &mut builder,
        NodeKind::TableRow,
        attrs(&[("rowTag", AttrValue::Integer(9))]),
        NodeContent::children([nested_cell]),
    );
    let nested = block(
        &mut builder,
        NodeKind::Table,
        attrs(&[("nested", AttrValue::Bool(true))]),
        NodeContent::children([nested_row]),
    );
    let header = block(
        &mut builder,
        NodeKind::TableHeader,
        attrs(&[
            ("rowspan", AttrValue::Integer(2)),
            ("colspan", AttrValue::Integer(2)),
            (
                "colwidth",
                AttrValue::List(vec![AttrValue::Integer(0), AttrValue::Integer(90)]),
            ),
            ("backgroundColor", AttrValue::Null),
        ]),
        NodeContent::children([mixed, image, nested]),
    );
    let row = block(
        &mut builder,
        NodeKind::TableRow,
        attrs(&[("height", AttrValue::Integer(50))]),
        NodeContent::children([header]),
    );
    let covered = block(
        &mut builder,
        NodeKind::TableRow,
        attrs(&[("covered", AttrValue::Null)]),
        NodeContent::children([]),
    );
    let table = block(
        &mut builder,
        NodeKind::Table,
        attrs(&[("layout", AttrValue::String("fixed".into()))]),
        NodeContent::children([row, covered]),
    );
    let root = block(
        &mut builder,
        NodeKind::Document,
        NodeAttrs::empty(),
        NodeContent::children([outside, table]),
    );
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        outside,
        table,
    )
}

fn copy(document: &XiaomuDocument, table: NodeId) -> ClipboardSlice {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::node(document, table).unwrap(),
    )
    .unwrap()
    .clipboard_slice()
    .unwrap()
    .unwrap()
}

fn session(document: &XiaomuDocument, outside: NodeId) -> DocumentSession {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(outside)),
    )
    .unwrap()
}

fn children(document: &XiaomuDocument, id: NodeId) -> &[NodeId] {
    document.node(id).unwrap().content().as_children().unwrap()
}

fn subtree_ids(document: &XiaomuDocument, root: NodeId) -> BTreeSet<NodeId> {
    let mut pending = vec![root];
    let mut ids = BTreeSet::new();
    while let Some(id) = pending.pop() {
        assert!(ids.insert(id));
        let content = document.node(id).unwrap().content();
        if let Some(children) = content.as_children() {
            pending.extend(children.iter().copied());
        }
        if let Some(inline) = content.as_inline() {
            pending.extend(inline.atoms().iter().map(|atom| atom.atom()));
        }
    }
    ids
}

fn insert(parent: NodeId, index: usize, tree: &TableTreeTemplate) -> Transaction {
    Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertTableTree {
        parent,
        index,
        tree: tree.clone(),
    })
}

#[test]
fn rich_native_and_v13_sources_preserve_every_payload_with_disjoint_fresh_ids_and_exact_history() {
    let (source, outside, table) = fixture();
    let native = copy(&source, table);
    let encoded = encode_metadata(&native).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&encoded).unwrap()["version"],
        13
    );
    let decoded = decode_metadata(native.plain_text(), &encoded).unwrap();
    assert_eq!(decoded, native);
    for clipboard in [native.clone(), decoded] {
        let before_clipboard = clipboard.clone();
        let tree = clipboard.roots()[0].table_tree_template().unwrap();
        let source_ids = subtree_ids(&source, table);
        assert_eq!(tree.node_count(), source_ids.len());
        let mut target = session(&source, outside);
        let before_selection = target.selection();
        target.apply(&insert(source.root(), 2, &tree)).unwrap();
        let first = target.document().clone();
        let first_table = children(&first, first.root())[2];
        assert_eq!(copy(&first, first_table), clipboard);
        let first_ids = subtree_ids(&first, first_table);
        assert!(first_ids.is_disjoint(&subtree_ids(&source, source.root())));
        assert_eq!(first_ids.len(), tree.node_count());
        target.apply(&insert(source.root(), 3, &tree)).unwrap();
        let second = target.document().clone();
        let second_table = children(&second, second.root())[3];
        assert_eq!(copy(&second, second_table), clipboard);
        let second_ids = subtree_ids(&second, second_table);
        assert!(second_ids.is_disjoint(&subtree_ids(&first, first.root())));
        assert_eq!(second_ids.len(), tree.node_count());
        assert_eq!(target.history_depths(), (2, 0));
        assert_eq!(target.selection(), before_selection);
        for id in subtree_ids(&source, source.root())
            .into_iter()
            .filter(|id| *id != source.root())
        {
            assert_eq!(second.node(id), source.node(id));
        }
        target.undo().unwrap();
        assert_eq!(target.document().store(), first.store());
        target.undo().unwrap();
        assert_eq!(target.document().store(), source.store());
        assert_eq!(target.selection(), before_selection);
        target.redo().unwrap();
        assert_eq!(target.document().store(), first.store());
        target.redo().unwrap();
        assert_eq!(target.document().store(), second.store());
        assert_eq!(subtree_ids(target.document(), second_table), second_ids);
        assert_eq!(clipboard, before_clipboard);
        assert_eq!(copy(&source, table), native);
    }
}

#[test]
fn bad_source_kind_and_failed_destinations_leave_clipboard_source_session_and_redo_unchanged() {
    let (source, outside, table) = fixture();
    let clipboard = copy(&source, table);
    let original_clipboard = clipboard.clone();
    assert_eq!(
        copy(&source, outside).roots()[0]
            .table_tree_template()
            .unwrap_err(),
        Error::InvalidTableStructure
    );
    let tree = clipboard.roots()[0].table_tree_template().unwrap();
    let mut target = session(&source, outside);
    target
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    target.undo().unwrap();
    target
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let before = target.document().clone();
    let selection = target.selection();
    let history = target.history_depths();
    let marks = target.stored_marks().cloned();
    let row = children(&source, table)[0];
    for transaction in [
        insert(row, 0, &tree),
        insert(outside, 0, &tree),
        insert(source.root(), usize::MAX, &tree),
        insert(source.root(), 2, &tree).with_step(TransactionStep::RemoveNode {
            node: source.root(),
        }),
    ] {
        assert!(target.apply(&transaction).is_err());
        assert_eq!(target.document().store(), before.store());
        assert_eq!(target.document().revision(), before.revision());
        assert_eq!(target.selection(), selection);
        assert_eq!(target.history_depths(), history);
        assert_eq!(target.stored_marks(), marks.as_ref());
        assert_eq!(clipboard, original_clipboard);
        assert_eq!(copy(&source, table), original_clipboard);
    }
    target.redo().unwrap();
    assert!(
        target
            .document()
            .node(outside)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .text()
            .as_str()
            .starts_with('x')
    );
    let mut control = session(&before, outside);
    control.apply(&insert(source.root(), 2, &tree)).unwrap();
    target.undo().unwrap();
    target.apply(&insert(source.root(), 2, &tree)).unwrap();
    assert_eq!(target.document().store(), control.document().store());
}

fn legacy_table() -> Value {
    json!({"format":"xiaomu.clipboard","version":5,"roots":[{
        "kind":{"type":"table"},"attrs":{},"content":{"type":"table","value":{"rows":[[{
            "kind":{"type":"table_cell"},"attrs":{},"content":{"type":"children","value":{"children":[{
                "kind":{"type":"paragraph"},"attrs":{},"content":{"type":"inline","value":{"runs":[{"text":"unit","marks":[]}]}}
            }]}}
        }]]}}
    }]})
}

#[test]
fn actual_legacy_wire_envelopes_and_v13_boundary_contracts_are_unchanged() {
    let (source, outside, _) = fixture();
    for version in 1..=13 {
        let mut wire = legacy_table();
        wire["version"] = json!(version);
        if version >= 11 {
            wire["closed"] = json!(version == 11);
        }
        for feature in ["header", "colspan", "rowspan", "colwidth"] {
            let mut carrying = wire.clone();
            let cell = &mut carrying["roots"][0]["content"]["value"]["rows"][0][0];
            match feature {
                "header" => cell["kind"]["type"] = json!("table_header"),
                "colwidth" => cell["attrs"][feature] = json!({"type":"null"}),
                _ => cell["attrs"][feature] = json!({"type":"integer","value":1}),
            }
            assert_eq!(
                decode_metadata("unit", &carrying.to_string()).is_some(),
                version == 13,
                "v{version} {feature}"
            );
        }
        let decoded = decode_metadata("unit", &wire.to_string());
        if version < 5 {
            assert!(decoded.is_none(), "v{version} must reject table payloads");
            continue;
        }
        let decoded = decoded.unwrap();
        let original = decoded.clone();
        let template = decoded.roots()[0].table_tree_template().unwrap();
        assert_eq!(decoded.is_closed(), version == 11);
        let mut target = session(&source, outside);
        target.apply(&insert(source.root(), 2, &template)).unwrap();
        let cloned = children(target.document(), source.root())[2];
        assert_eq!(copy(target.document(), cloned).roots(), decoded.roots());
        assert_eq!(decoded, original);
    }
    let mut missing_boundary = legacy_table();
    missing_boundary["version"] = json!(13);
    assert!(decode_metadata("unit", &missing_boundary.to_string()).is_none());
}

#[test]
fn obtaining_a_template_never_grants_closed_or_open_spanning_paste_fitting() {
    let (source, outside, table) = fixture();
    let closed = copy(&source, table);
    let mut wire: Value = serde_json::from_str(&encode_metadata(&closed).unwrap()).unwrap();
    wire["closed"] = json!(false);
    // One 2x2 origin: newline in atom fallback flattens inside the origin,
    // while covered logical slots remain blank TSV cells.
    let open_text = closed.plain_text().replace(['\t', '\r', '\n'], " ") + "\t\n\t";
    let open = decode_metadata(&open_text, &wire.to_string()).unwrap();
    assert!(!open.is_closed());
    for slice in [closed, open] {
        let saved = slice.clone();
        let _template = slice.roots()[0].table_tree_template().unwrap();
        let mut target = session(&source, outside);
        let before = target.document().clone();
        let selection = target.selection();
        assert_eq!(
            target.apply_intent(&EditIntent::PasteSlice {
                slice: slice.clone()
            }),
            Err(SessionError::UnsupportedTableOperation)
        );
        assert_eq!(target.document().store(), before.store());
        assert_eq!(target.document().revision(), before.revision());
        assert_eq!(target.selection(), selection);
        assert_eq!(target.history_depths(), (0, 0));
        assert_eq!(slice, saved);
    }
}
