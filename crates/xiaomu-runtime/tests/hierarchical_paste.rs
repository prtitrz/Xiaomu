//! P3.3 hierarchical structured-paste integration regressions.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn plain(text: &str) -> InlineContent {
    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn inline_point(
    document: &XiaomuDocument,
    node: NodeId,
    raw: usize,
    ordinal: usize,
) -> InlinePoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    InlinePoint::new(
        node,
        inline.offset_at(raw).unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
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

fn children(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn insert_atom(
    document: XiaomuDocument,
    node: NodeId,
    raw: usize,
    ordinal: usize,
    fallback: &str,
) -> (XiaomuDocument, NodeId) {
    let before: Vec<_> = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .collect();
    let at = inline_point(&document, node, raw, ordinal);
    let next = Transaction::new(TransactionOrigin::Extension("hierarchy-atom-test".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at,
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(&document)
        .unwrap();
    let atom = next
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .find(|atom| !before.contains(atom))
        .unwrap();
    (next, atom)
}

fn atom_fallback(document: &XiaomuDocument, atom: NodeId) -> String {
    document
        .node(atom)
        .unwrap()
        .content()
        .as_inline_atom()
        .unwrap()
        .fallback_text()
        .to_owned()
}

fn list_slice() -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("甲乙")),
        )
        .unwrap();
    let first_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("尾巴")),
        )
        .unwrap();
    let second_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([second]),
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
            NodeContent::children([list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection = DocumentSelection::new(
        point(&document, first, "甲".len()),
        point(&document, second, "尾".len()),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn list_slice_with_atom() -> ClipboardSlice {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("甲乙")),
        )
        .unwrap();
    let first_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("尾巴")),
        )
        .unwrap();
    let second_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([second]),
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
            NodeContent::children([list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let (document, _) = insert_atom(document, second, 0, 0, "@源");
    let selection = DocumentSelection::new(
        point(&document, first, "甲".len()),
        point(&document, second, "尾".len()),
    );
    let slice = DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert_eq!(slice.plain_text(), "乙\n@源尾");
    slice
}

fn assert_pasted_list(document: &XiaomuDocument, list: NodeId) -> NodeId {
    assert!(matches!(
        document.node(list).unwrap().kind(),
        NodeKind::BulletList
    ));
    let items = children(document, list);
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|item| matches!(document.node(*item).unwrap().kind(), NodeKind::ListItem))
    );
    let first = children(document, items[0])[0];
    let second = children(document, items[1])[0];
    assert_eq!(text(document, first), "乙");
    assert_eq!(text(document, second), "尾");
    second
}

#[test]
fn list_fragment_pastes_between_target_prefix_and_suffix_as_one_history_entry() {
    let slice = list_slice();
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

    let roots = children(session.document(), root);
    assert_eq!(roots.len(), 3);
    assert_eq!(roots[0], target);
    assert_eq!(text(session.document(), roots[0]), "a");
    let last_leaf = assert_pasted_list(session.document(), roots[1]);
    assert_eq!(text(session.document(), roots[2]), "b");

    let DocumentPosition::Inline(caret) = session.selection().focus() else {
        panic!("hierarchical paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), last_leaf);
    assert_eq!(caret.text_offset().as_usize(), "尾".len());

    let after_store = session.document().store().clone();
    let after_selection = session.selection();
    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &after_store);
    assert_eq!(session.selection(), after_selection);
}

#[test]
fn list_fragment_replaces_cross_block_target_atomically() {
    let slice = list_slice();
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(plain("abc")),
        )
        .unwrap();
    let middle = builder
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
            NodeContent::children([middle]),
        )
        .unwrap();
    let tail = builder
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
            NodeContent::children([tail]),
        )
        .unwrap();
    let old_list = builder
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
            NodeContent::children([first, old_list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let before_store = document.store().clone();
    let selection = DocumentSelection::new(
        point(&document, first, 1),
        point(&document, tail, "尾".len()),
    );
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert!(session.document().node(old_list).is_none());

    let roots = children(session.document(), root);
    assert_eq!(roots.len(), 3);
    assert_eq!(roots[0], first);
    assert_eq!(text(session.document(), roots[0]), "a");
    let last_leaf = assert_pasted_list(session.document(), roots[1]);
    assert_eq!(text(session.document(), roots[2]), "巴");

    let DocumentPosition::Inline(caret) = session.selection().focus() else {
        panic!("hierarchical paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), last_leaf);
    assert_eq!(caret.text_offset().as_usize(), "尾".len());

    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
}

#[test]
fn hierarchical_paste_materializes_source_atoms_and_moves_target_suffix_atoms() {
    let slice = list_slice_with_atom();
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
    let (document, target_atom) = insert_atom(document, target, 1, 0, "@目标");
    let before_store = document.store().clone();
    let selection = DocumentSelection::collapsed(inline_point(&document, target, 1, 0));
    let mut session = DocumentSession::new(document, selection).unwrap();

    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    assert_eq!(session.history_depths(), (1, 0));

    let roots = children(session.document(), root);
    assert_eq!(roots.len(), 3);
    assert_eq!(roots[0], target);
    assert_eq!(text(session.document(), target), "a");

    let last_leaf = assert_pasted_list(session.document(), roots[1]);
    let pasted_atoms = session
        .document()
        .node(last_leaf)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms();
    assert_eq!(pasted_atoms.len(), 1);
    let source_atom = pasted_atoms[0].atom();
    assert_eq!(pasted_atoms[0].text_offset().as_usize(), 0);
    assert_eq!(atom_fallback(session.document(), source_atom), "@源");
    assert_eq!(session.document().parent_of(source_atom), Some(last_leaf));

    let suffix = roots[2];
    assert_eq!(text(session.document(), suffix), "b");
    assert_eq!(session.document().parent_of(target_atom), Some(suffix));
    let suffix_atoms = session
        .document()
        .node(suffix)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms();
    assert_eq!(suffix_atoms.len(), 1);
    assert_eq!(suffix_atoms[0].atom(), target_atom);
    assert_eq!(suffix_atoms[0].text_offset().as_usize(), 0);
    assert_eq!(atom_fallback(session.document(), target_atom), "@目标");

    let DocumentPosition::Inline(caret) = session.selection().focus() else {
        panic!("hierarchical paste must leave a text caret");
    };
    assert_eq!(caret.node_id(), last_leaf);
    assert_eq!(caret.text_offset().as_usize(), "尾".len());

    let after_store = session.document().store().clone();
    let after_selection = session.selection();
    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &before_store);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), &after_store);
    assert_eq!(session.selection(), after_selection);
}
