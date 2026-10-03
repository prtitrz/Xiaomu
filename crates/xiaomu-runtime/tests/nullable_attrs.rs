//! Null attrs survive editing and exact runtime Undo/Redo.

use xiaomu_core::document::{
    AttrValue, InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

#[test]
fn null_attrs_survive_set_split_join_undo_and_redo() {
    let attrs = NodeAttrs::new(
        [
            ("textAlign".into(), AttrValue::Null),
            (
                "extension".into(),
                AttrValue::List(vec![AttrValue::Object(
                    [("default".into(), AttrValue::Null)].into(),
                )]),
            ),
        ]
        .into(),
    )
    .unwrap();
    let inline = InlineContent::new([TextRun::new("中🙂", Default::default()).unwrap()]).unwrap();
    let at = inline.offset_at("中".len()).unwrap();
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let original = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection =
        DocumentSelection::collapsed(TextPoint::new(paragraph, at, CursorAffinity::Before));
    let mut session = DocumentSession::new(original.clone(), selection).unwrap();
    session
        .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeAttrs {
                node: paragraph,
                attrs: attrs.clone(),
            },
        ))
        .unwrap();
    let with_attrs = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), original.store());
    assert_eq!(
        session
            .document()
            .node(paragraph)
            .unwrap()
            .attrs()
            .get("textAlign"),
        None
    );
    session.redo().unwrap();
    assert_eq!(session.document().store(), with_attrs.store());
    assert_eq!(
        session
            .document()
            .node(paragraph)
            .unwrap()
            .attrs()
            .get("textAlign"),
        Some(&AttrValue::Null)
    );

    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let split = session.document().clone();
    let split_selection = session.selection();
    let nodes = split.node(root).unwrap().content().as_children().unwrap();
    assert_eq!(nodes.len(), 2);
    for &node in nodes {
        assert_eq!(split.node(node).unwrap().attrs(), &attrs);
    }
    session.undo().unwrap();
    assert_eq!(session.document().store(), with_attrs.store());
    assert_eq!(session.selection(), selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), split.store());
    assert_eq!(session.selection(), split_selection);

    session.apply_intent(&EditIntent::JoinWithPrevious).unwrap();
    let joined = session.document().clone();
    let joined_selection = session.selection();
    assert_eq!(joined.store(), with_attrs.store());
    session.undo().unwrap();
    assert_eq!(session.document().store(), split.store());
    assert_eq!(session.selection(), split_selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), joined.store());
    assert_eq!(session.selection(), joined_selection);
}
