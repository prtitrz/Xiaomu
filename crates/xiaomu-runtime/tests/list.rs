//! P2.4 list editing command tests: build / convert / indent / outdent /
//! lift-out, plus the exact-undo guarantees of staged commands.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent, SessionOutcome};

fn offset_of(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextOffset {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap()
}

fn caret(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextSelection {
    TextSelection::collapsed(TextPoint::new(
        node,
        offset_of(document, node, raw),
        CursorAffinity::Before,
    ))
}

fn session_with(document: &XiaomuDocument, selection: TextSelection) -> DocumentSession {
    DocumentSession::new(document.clone(), DocumentSelection::text(selection)).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn empty_paragraph(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap()
}

fn list_item(builder: &mut NodeStoreBuilder, block: NodeId) -> NodeId {
    builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap()
}

/// `Document > [p(...)]`
fn one_paragraph(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let block = paragraph(&mut builder, text);
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), block)
}

/// `Document > [L(Bullet) > [item_a > [a], item_b > [b], item_c > [c]]]`
struct ThreeItemList {
    document: XiaomuDocument,
    list: NodeId,
    item_a: NodeId,
    a: NodeId,
    item_b: NodeId,
    b: NodeId,
    item_c: NodeId,
    c: NodeId,
}

fn three_item_list() -> ThreeItemList {
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder, "一");
    let b = paragraph(&mut builder, "二");
    let c = paragraph(&mut builder, "三");
    let item_a = list_item(&mut builder, a);
    let item_b = list_item(&mut builder, b);
    let item_c = list_item(&mut builder, c);
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_b, item_c]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    ThreeItemList {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        list,
        item_a,
        a,
        item_b,
        b,
        item_c,
        c,
    }
}

fn children_of(session: &DocumentSession, parent: NodeId) -> Vec<NodeId> {
    session
        .document()
        .node(parent)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn root_children(session: &DocumentSession) -> Vec<NodeId> {
    children_of(session, session.document().root())
}

fn kind_of(session: &DocumentSession, node: NodeId) -> NodeKind {
    session.document().node(node).unwrap().kind().clone()
}

fn text_of(session: &DocumentSession, node: NodeId) -> String {
    session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .map(|inline| {
            inline
                .runs()
                .iter()
                .map(|run| run.text().as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn caret_node_and_offset(session: &DocumentSession) -> (NodeId, usize) {
    let selection = session.text_selection().expect("single-block selection");
    (
        selection.focus().node_id(),
        selection.focus().offset().as_usize(),
    )
}

/// Comparable whole-store snapshot: every node as `(id, kind, content)` in
/// the store's deterministic order.
fn store_snapshot(document: &XiaomuDocument) -> Vec<(NodeId, NodeKind, NodeContent)> {
    document
        .store()
        .iter()
        .map(|node| (node.id(), node.kind().clone(), node.content().clone()))
        .collect()
}

fn turn_into(session: &mut DocumentSession, kind: NodeKind) -> SessionOutcome {
    session
        .apply_intent(&EditIntent::TurnInto { kind })
        .unwrap()
}

#[test]
fn turn_into_list_wraps_the_block_into_a_single_item() {
    let (document, block) = one_paragraph("项目");
    let mut session = session_with(&document, caret(&document, block, 3));

    assert_eq!(
        turn_into(&mut session, NodeKind::BulletList),
        SessionOutcome::DocumentChanged
    );

    let root = root_children(&session);
    assert_eq!(root.len(), 1);
    assert_eq!(kind_of(&session, root[0]), NodeKind::BulletList);
    let items = children_of(&session, root[0]);
    assert_eq!(items.len(), 1);
    assert_eq!(kind_of(&session, items[0]), NodeKind::ListItem);
    assert_eq!(children_of(&session, items[0]), vec![block]);
    assert_eq!(text_of(&session, block), "项目");
    // Identity survives the move, so the caret stays at its offset.
    assert_eq!(caret_node_and_offset(&session), (block, 3));
}

#[test]
fn wrapping_undo_restores_the_exact_store_and_the_redo_reuses_identities() {
    let (document, block) = one_paragraph("项目");
    let before = store_snapshot(&document);
    let mut session = session_with(&document, caret(&document, block, 0));

    turn_into(&mut session, NodeKind::BulletList);
    assert_eq!(session.history_depths(), (1, 0));
    let wrapped_root = root_children(&session);

    session.undo().unwrap();
    assert_eq!(store_snapshot(session.document()), before);
    assert_eq!(caret_node_and_offset(&session), (block, 0));

    // Redo must reuse the identities recorded after the wrap, not mint new
    // container ids.
    session.redo().unwrap();
    assert_eq!(root_children(&session), wrapped_root);
    assert_eq!(caret_node_and_offset(&session), (block, 0));
}

#[test]
fn paragraph_to_list_to_paragraph_closes_with_one_undo_each() {
    let (document, block) = one_paragraph("段落");
    let mut session = session_with(&document, caret(&document, block, 0));

    turn_into(&mut session, NodeKind::OrderedList);
    assert_eq!(
        kind_of(&session, root_children(&session)[0]),
        NodeKind::OrderedList
    );

    turn_into(&mut session, NodeKind::Paragraph);

    let root = root_children(&session);
    assert_eq!(root, vec![block]);
    assert_eq!(kind_of(&session, block), NodeKind::Paragraph);
    assert_eq!(text_of(&session, block), "段落");
    assert_eq!(caret_node_and_offset(&session), (block, 0));
    assert_eq!(session.history_depths(), (2, 0));

    // One undo returns to the ordered list, a second to the plain document.
    session.undo().unwrap();
    let root = root_children(&session);
    assert_eq!(root.len(), 1);
    assert_eq!(kind_of(&session, root[0]), NodeKind::OrderedList);
    assert_eq!(caret_node_and_offset(&session), (block, 0));

    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&document)
    );
    assert_eq!(root_children(&session), vec![block]);
    assert_eq!(caret_node_and_offset(&session), (block, 0));
}

#[test]
fn bullet_converts_to_ordered_by_rekinding_the_list_itself() {
    let (document, block) = one_paragraph("条目");
    let mut session = session_with(&document, caret(&document, block, 0));

    turn_into(&mut session, NodeKind::BulletList);
    let list = root_children(&session)[0];

    turn_into(&mut session, NodeKind::OrderedList);
    // Same list identity, new kind; blocks and items untouched.
    assert_eq!(root_children(&session), vec![list]);
    assert_eq!(kind_of(&session, list), NodeKind::OrderedList);
    assert_eq!(text_of(&session, block), "条目");
    assert_eq!(session.history_depths(), (2, 0));

    // Converting to the kind already present is a no-op.
    assert_eq!(
        turn_into(&mut session, NodeKind::OrderedList),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (2, 0));
}

#[test]
fn lifting_the_last_item_places_the_block_after_the_remaining_list() {
    let fixture = three_item_list();
    let ThreeItemList {
        ref document,
        list,
        c,
        item_c,
        ..
    } = fixture;
    let mut session = session_with(document, caret(document, c, 0));

    turn_into(&mut session, NodeKind::Paragraph);

    // Shift-Tab on the last item must not jump above earlier items.
    let root = root_children(&session);
    assert_eq!(root, vec![list, c]);
    assert_eq!(
        children_of(&session, list),
        vec![fixture.item_a, fixture.item_b]
    );
    assert!(!children_of(&session, list).contains(&item_c));
    assert_eq!(text_of(&session, c), "三");
    assert_eq!(caret_node_and_offset(&session), (c, 0));

    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&fixture.document)
    );
}

#[test]
fn lifting_out_of_a_multi_item_list_dissolves_only_the_focused_item() {
    let fixture = three_item_list();
    let ThreeItemList {
        ref document,
        list,
        item_b,
        b,
        ..
    } = fixture;
    let mut session = session_with(document, caret(document, b, 3));

    turn_into(&mut session, NodeKind::Paragraph);

    let root = root_children(&session);
    assert_eq!(root.len(), 3);
    assert_eq!(root[0], list);
    assert_eq!(root[1], b);
    let tail = root[2];
    assert_eq!(kind_of(&session, tail), NodeKind::BulletList);
    assert_eq!(children_of(&session, list), vec![fixture.item_a]);
    assert_eq!(children_of(&session, fixture.item_a), vec![fixture.a]);
    assert_eq!(children_of(&session, tail), vec![fixture.item_c]);
    assert!(!children_of(&session, list).contains(&item_b));
    assert_eq!(text_of(&session, b), "二");
    assert_eq!(caret_node_and_offset(&session), (b, 3));

    // Undo puts the lifted block back into its item exactly.
    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&fixture.document)
    );
    assert_eq!(caret_node_and_offset(&session), (b, 3));
}

#[test]
fn indent_nests_the_item_in_a_newly_created_inner_list() {
    let fixture = three_item_list();
    let ThreeItemList {
        ref document,
        list,
        item_b,
        b,
        item_c,
        c,
        ..
    } = fixture;
    let mut session = session_with(document, caret(document, c, 3));

    assert_eq!(
        session.apply_intent(&EditIntent::IndentListItem).unwrap(),
        SessionOutcome::DocumentChanged
    );

    // item_c moved under a fresh inner list appended to item_b.
    let item_b_blocks = children_of(&session, item_b);
    assert_eq!(item_b_blocks[0], b);
    let inner_list = item_b_blocks[1];
    assert_eq!(kind_of(&session, inner_list), NodeKind::BulletList);
    let inner_items = children_of(&session, inner_list);
    assert_eq!(inner_items, vec![item_c]);
    assert_eq!(children_of(&session, item_c), vec![c]);
    // The outer list now holds only the first two items.
    assert_eq!(children_of(&session, list), vec![fixture.item_a, item_b]);
    assert_eq!(text_of(&session, c), "三");
    assert_eq!(caret_node_and_offset(&session), (c, 3));

    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&fixture.document)
    );
    assert_eq!(caret_node_and_offset(&session), (c, 3));

    session.redo().unwrap();
    assert_eq!(children_of(&session, item_b)[1], inner_list);
    assert_eq!(children_of(&session, inner_list), vec![item_c]);
}

#[test]
fn outdent_moves_the_item_after_its_outer_item_and_drops_the_empty_list() {
    // Document > [L > [A > [head, L2 > [B > [nested]]]]]
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "头");
    let nested = paragraph(&mut builder, "嵌");
    let inner_item = list_item(&mut builder, nested);
    let inner_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([inner_item]),
        )
        .unwrap();
    let outer_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([head, inner_list]),
        )
        .unwrap();
    let outer_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([outer_item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([outer_list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = session_with(&document, caret(&document, nested, 3));

    assert_eq!(
        session.apply_intent(&EditIntent::OutdentListItem).unwrap(),
        SessionOutcome::DocumentChanged
    );

    // The item re-entered after its outer item; the emptied nested list
    // dissolved in the same command.
    assert_eq!(
        children_of(&session, outer_list),
        vec![outer_item, inner_item]
    );
    assert_eq!(children_of(&session, outer_item), vec![head]);
    assert_eq!(children_of(&session, inner_item), vec![nested]);
    assert_eq!(text_of(&session, nested), "嵌");
    assert_eq!(caret_node_and_offset(&session), (nested, 3));

    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&document)
    );
    assert_eq!(caret_node_and_offset(&session), (nested, 3));

    session.redo().unwrap();
    assert_eq!(
        children_of(&session, outer_list),
        vec![outer_item, inner_item]
    );
    assert_eq!(caret_node_and_offset(&session), (nested, 3));
}

#[test]
fn indent_reuses_the_previous_item_s_trailing_nested_list() {
    // Document > [L > [A > [a, inner > [B > [b]]], C > [c]]]
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder, "一");
    let b = paragraph(&mut builder, "二");
    let c = paragraph(&mut builder, "三");
    let item_b = list_item(&mut builder, b);
    let inner_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_b]),
        )
        .unwrap();
    let item_a = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([a, inner_list]),
        )
        .unwrap();
    let item_c = list_item(&mut builder, c);
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_c]),
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
    let mut session = session_with(&document, caret(&document, c, 0));

    // item_c's previous sibling is item_a, whose trailing child is the
    // nested list — it is reused instead of creating another one.
    session.apply_intent(&EditIntent::IndentListItem).unwrap();
    assert_eq!(children_of(&session, list), vec![item_a]);
    assert_eq!(children_of(&session, item_a), vec![a, inner_list]);
    assert_eq!(children_of(&session, inner_list), vec![item_b, item_c]);
    assert_eq!(children_of(&session, item_c), vec![c]);
    assert_eq!(caret_node_and_offset(&session), (c, 0));

    session.undo().unwrap();
    assert_eq!(
        store_snapshot(session.document()),
        store_snapshot(&document)
    );
    assert_eq!(caret_node_and_offset(&session), (c, 0));
}

#[test]
fn indenting_the_first_item_is_a_no_op() {
    let (document, block) = one_paragraph("唯一");
    let mut session = session_with(&document, caret(&document, block, 0));

    turn_into(&mut session, NodeKind::BulletList);

    assert_eq!(
        session.apply_intent(&EditIntent::IndentListItem).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn outdenting_a_top_level_item_is_a_no_op() {
    let (document, block) = one_paragraph("唯一");
    let mut session = session_with(&document, caret(&document, block, 0));

    turn_into(&mut session, NodeKind::BulletList);

    assert_eq!(
        session.apply_intent(&EditIntent::OutdentListItem).unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn turn_into_paragraph_outside_a_list_is_a_no_op_when_already_paragraph() {
    let (document, block) = one_paragraph("正文");
    let mut session = session_with(&document, caret(&document, block, 0));

    assert_eq!(
        turn_into(&mut session, NodeKind::Paragraph),
        SessionOutcome::NoChange
    );
    assert_eq!(session.history_depths(), (0, 0));
}

#[test]
fn enter_in_a_list_item_creates_a_sibling_item_holding_the_tail() {
    let (document, block) = one_paragraph("一二");
    let mut session = session_with(&document, caret(&document, block, 3));
    turn_into(&mut session, NodeKind::BulletList);
    let before = store_snapshot(session.document());
    let list = root_children(&session)[0];
    let item = children_of(&session, list)[0];

    session.apply_intent(&EditIntent::SplitBlock).unwrap();

    let items = children_of(&session, list);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], item);
    assert_eq!(children_of(&session, item), vec![block]);
    assert_eq!(text_of(&session, block), "一");
    let tail = children_of(&session, items[1])[0];
    assert_eq!(text_of(&session, tail), "二");
    assert_eq!(caret_node_and_offset(&session), (tail, 0));

    session.undo().unwrap();
    assert_eq!(store_snapshot(session.document()), before);
    assert_eq!(caret_node_and_offset(&session), (block, 3));
}

#[test]
fn enter_at_the_end_of_a_list_item_inserts_an_empty_sibling() {
    let fixture = three_item_list();
    let mut session = session_with(&fixture.document, caret(&fixture.document, fixture.a, 3));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();

    let items = children_of(&session, fixture.list);
    assert_eq!(items.len(), 4);
    assert_eq!(items[0], fixture.item_a);
    assert_eq!(items[2], fixture.item_b);
    assert_eq!(items[3], fixture.item_c);
    let tail = children_of(&session, items[1])[0];
    assert_eq!(text_of(&session, fixture.a), "一");
    assert_eq!(text_of(&session, tail), "");
    assert_eq!(caret_node_and_offset(&session), (tail, 0));
}

#[test]
fn enter_on_an_empty_top_level_item_lifts_out_of_the_list() {
    let mut builder = NodeStoreBuilder::new();
    let a = paragraph(&mut builder, "一");
    let empty = empty_paragraph(&mut builder);
    let item_a = list_item(&mut builder, a);
    let item_empty = list_item(&mut builder, empty);
    let list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_empty]),
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
    let mut session = session_with(&document, caret(&document, empty, 0));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();

    // Last empty item lifts after the remaining list.
    assert_eq!(root_children(&session), vec![list, empty]);
    assert_eq!(children_of(&session, list), vec![item_a]);
    assert_eq!(kind_of(&session, empty), NodeKind::Paragraph);
    assert_eq!(caret_node_and_offset(&session), (empty, 0));
}

#[test]
fn enter_on_an_empty_nested_item_outdents_one_level() {
    let mut builder = NodeStoreBuilder::new();
    let head = paragraph(&mut builder, "头");
    let nested = empty_paragraph(&mut builder);
    let inner_item = list_item(&mut builder, nested);
    let inner_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([inner_item]),
        )
        .unwrap();
    let outer_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([head, inner_list]),
        )
        .unwrap();
    let outer_list = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([outer_item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([outer_list]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = session_with(&document, caret(&document, nested, 0));

    session.apply_intent(&EditIntent::SplitBlock).unwrap();

    assert_eq!(
        children_of(&session, outer_list),
        vec![outer_item, inner_item]
    );
    assert_eq!(children_of(&session, outer_item), vec![head]);
    assert_eq!(children_of(&session, inner_item), vec![nested]);
    assert_eq!(caret_node_and_offset(&session), (nested, 0));
}
