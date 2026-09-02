//! P3.3 cross-block editing and clipboard integration regressions.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn inline(text: &str) -> InlineContent {
    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

struct Fixture {
    document: XiaomuDocument,
    first: NodeId,
    list_first: NodeId,
    list_second: NodeId,
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline("abc")),
        )
        .unwrap();
    let list_first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline("中间")),
        )
        .unwrap();
    let first_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([list_first]),
        )
        .unwrap();
    let list_second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline("尾巴")),
        )
        .unwrap();
    let second_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([list_second]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([first_item, second_item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, list]),
        )
        .unwrap();

    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        list_first,
        list_second,
    }
}

fn cross_block_selection(fixture: &Fixture) -> DocumentSelection {
    // Select from after `a` through the first scalar of `尾巴`. The selected
    // plain fallback is `bc\n中间\n尾`, and the surviving suffix is `巴`.
    DocumentSelection::new(
        point(&fixture.document, fixture.first, 1),
        point(&fixture.document, fixture.list_second, "尾".len()),
    )
}

#[test]
fn cross_block_delete_is_one_history_entry_and_round_trips_exact_store() {
    let fixture = fixture();
    let selection = cross_block_selection(&fixture);
    let before_store = fixture.document.store().clone();
    let mut session = DocumentSession::new(fixture.document.clone(), selection).unwrap();

    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.apply_intent(&EditIntent::Delete).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(text(session.document(), fixture.first), "a巴");
    assert!(session.document().node(fixture.list_first).is_none());
    assert!(session.document().node(fixture.list_second).is_none());

    let after_store = session.document().store().clone();
    let after_selection = session.selection();
    let DocumentPosition::Inline(caret) = after_selection.focus() else {
        panic!("delete must leave a text caret");
    };
    assert_eq!(caret.node_id(), fixture.first);
    assert_eq!(caret.text_offset().as_usize(), 1);
    assert_eq!(after_selection.anchor(), after_selection.focus());

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.history_depths(), (0, 1));
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);

    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(session.document().store(), &after_store);
    assert_eq!(session.selection(), after_selection);
}

#[test]
fn cut_flow_projects_clipboard_before_one_atomic_delete() {
    let fixture = fixture();
    let selection = cross_block_selection(&fixture);
    let mut session = DocumentSession::new(fixture.document.clone(), selection).unwrap();

    let slice = session
        .clipboard_slice()
        .unwrap()
        .expect("selected content");
    assert_eq!(slice.plain_text(), "bc\n中间\n尾");
    assert_eq!(slice.blocks().len(), 3);
    assert_eq!(session.history_depths(), (0, 0));

    assert_eq!(
        session.apply_intent(&EditIntent::Delete).unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(text(session.document(), fixture.first), "a巴");
}
