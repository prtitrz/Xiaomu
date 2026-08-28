//! P3.3 structured paste integration regressions.

use xiaomu_core::document::{
    HeadingLevel, InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn inline(runs: impl IntoIterator<Item = TextRun>) -> InlineContent {
    InlineContent::new(runs).unwrap()
}

fn plain(text: &str) -> InlineContent {
    inline([TextRun::new(text, MarkSet::empty()).unwrap()])
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

fn mark_at(document: &XiaomuDocument, node: NodeId, raw: usize, kind: MarkKind) -> bool {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    let mut cursor = 0usize;
    for run in inline.runs() {
        let end = cursor + run.len_bytes();
        if raw >= cursor && raw < end {
            return run.marks().contains(kind);
        }
        cursor = end;
    }
    false
}

fn root_children(document: &XiaomuDocument) -> Vec<NodeId> {
    document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn single_block_slice() -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline([
                TextRun::new("X", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                TextRun::new("Y", MarkSet::empty()).unwrap(),
            ])),
        )
        .unwrap();
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
        point(&document, paragraph, 2),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn three_block_slice() -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let heading = builder
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(inline([TextRun::new(
                "X",
                MarkSet::new([Mark::Bold]).unwrap(),
            )
            .unwrap()])),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("Y")),
        )
        .unwrap();
    let code = builder
        .insert(
            NodeKind::CodeBlock,
            NodeAttrs::empty(),
            NodeContent::Inline(inline([TextRun::new(
                "Z",
                MarkSet::new([Mark::Italic]).unwrap(),
            )
            .unwrap()])),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, paragraph, code]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(point(&document, heading, 0), point(&document, code, 1));
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn two_block_slice() -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("Q")),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("R")),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(point(&document, first, 0), point(&document, second, 1));
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

#[test]
fn single_block_paste_replaces_host_inheritance_with_exact_source_marks() {
    let slice = single_block_slice();
    let mut builder = NodeStoreBuilder::new();
    let target = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline([TextRun::new(
                "ab",
                MarkSet::new([Mark::Italic]).unwrap(),
            )
            .unwrap()])),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([target]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let before_store = document.store().clone();
    let selection = DocumentSelection::collapsed(point(&document, target, 1));
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(text(session.document(), target), "aXYb");
    assert!(mark_at(session.document(), target, 0, MarkKind::Italic));
    assert!(mark_at(session.document(), target, 1, MarkKind::Bold));
    assert!(!mark_at(session.document(), target, 1, MarkKind::Italic));
    assert!(!mark_at(session.document(), target, 2, MarkKind::Bold));
    assert!(!mark_at(session.document(), target, 2, MarkKind::Italic));
    assert!(mark_at(session.document(), target, 3, MarkKind::Italic));

    let DocumentPosition::Text(caret) = session.selection().focus() else {
        panic!("paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), target);
    assert_eq!(caret.offset().as_usize(), 3);

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
}

#[test]
fn multi_block_paste_preserves_inserted_kinds_marks_suffix_and_caret_seam() {
    let slice = three_block_slice();
    let mut builder = NodeStoreBuilder::new();
    let target = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("ab")),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([target]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let before_store = document.store().clone();
    let selection = DocumentSelection::collapsed(point(&document, target, 1));
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));

    let children = root_children(session.document());
    assert_eq!(children.len(), 3);
    assert_eq!(children[0], target);
    assert_eq!(text(session.document(), target), "aX");
    assert!(mark_at(session.document(), target, 1, MarkKind::Bold));
    assert!(matches!(
        session.document().node(children[1]).unwrap().kind(),
        NodeKind::Paragraph
    ));
    assert_eq!(text(session.document(), children[1]), "Y");
    assert!(matches!(
        session.document().node(children[2]).unwrap().kind(),
        NodeKind::CodeBlock
    ));
    assert_eq!(text(session.document(), children[2]), "Zb");
    assert!(mark_at(
        session.document(),
        children[2],
        0,
        MarkKind::Italic
    ));
    assert!(!mark_at(
        session.document(),
        children[2],
        1,
        MarkKind::Italic
    ));

    let DocumentPosition::Text(caret) = session.selection().focus() else {
        panic!("paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), children[2]);
    assert_eq!(caret.offset().as_usize(), 1);

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
}

#[test]
fn structured_paste_replaces_cross_block_target_as_one_atomic_history_entry() {
    let slice = two_block_slice();
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("abc")),
        )
        .unwrap();
    let middle_text = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("中间")),
        )
        .unwrap();
    let middle_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([middle_text]),
        )
        .unwrap();
    let tail_text = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("尾巴")),
        )
        .unwrap();
    let tail_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([tail_text]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([middle_item, tail_item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let before_store = document.store().clone();
    let selection = DocumentSelection::new(
        point(&document, first, 1),
        point(&document, tail_text, "尾".len()),
    );
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));
    let children = root_children(session.document());
    assert_eq!(children.len(), 2);
    assert_eq!(children[0], first);
    assert_eq!(text(session.document(), first), "aQ");
    assert_eq!(text(session.document(), children[1]), "R巴");
    assert!(session.document().node(list).is_none());

    let DocumentPosition::Text(caret) = session.selection().focus() else {
        panic!("paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), children[1]);
    assert_eq!(caret.offset().as_usize(), 1);

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
}
