//! P4.6 Atomic Block Contract: node selections over atomic blocks.
//!
//! `HorizontalRule` and friends carry no editable interior, so the session
//! addresses them as whole-node positions (`DocumentPosition::Atomic`).
//! These tests pin validation, document ordering, removal as one logical
//! history change, and selection convergence after the removal.

use xiaomu_core::document::{
    InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, NodeGap};
use xiaomu_core::text::TextBuffer;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError, SessionOutcome,
};

fn offset_of(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    let text: String = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    TextBuffer::from_string(text).offset_at(byte).unwrap()
}

/// `Document > [p("前"), HorizontalRule, p("后")]`.
fn text_rule_text_document() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("前", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let last = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("后", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, rule, last]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        rule,
        last,
    )
}

fn inline_point(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentPosition {
    DocumentPosition::Inline(xiaomu_core::selection::InlinePoint::new(
        node,
        offset_of(document, node, byte),
        0,
        CursorAffinity::Before,
    ))
}

#[test]
fn atomic_selection_requires_atomic_content() {
    let (document, first, rule, _) = text_rule_text_document();
    let mut session = session_with(&document, first, 0);

    session.set_atomic_selection(rule).unwrap();
    assert_eq!(session.selection().focus(), DocumentPosition::Atomic(rule));

    // Text-bearing and container nodes cannot be node-selected.
    assert_eq!(
        session.set_atomic_selection(first),
        Err(SessionError::SelectionInvalid),
    );
}

fn session_with(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentSession {
    DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(inline_point(document, node, byte)),
    )
    .unwrap()
}

#[test]
fn ordered_places_the_atomic_block_between_its_neighbors() {
    let (document, first, rule, last) = text_rule_text_document();

    // Focus later in the document than the anchor: head/tail resolve by
    // document order.
    let selection = DocumentSelection::new(
        inline_point(&document, last, 3),
        DocumentPosition::Atomic(rule),
    );
    let (head, tail) = selection.ordered(&document).unwrap();
    assert_eq!(head, DocumentPosition::Atomic(rule));
    assert_eq!(tail, inline_point(&document, last, 3));

    let selection = DocumentSelection::new(
        DocumentPosition::Atomic(rule),
        inline_point(&document, first, 3),
    );
    let (head, tail) = selection.ordered(&document).unwrap();
    assert_eq!(head, inline_point(&document, first, 3));
    assert_eq!(tail, DocumentPosition::Atomic(rule));
}

#[test]
fn backspace_on_atomic_selection_removes_the_block_as_one_change() {
    let (document, first, rule, _) = text_rule_text_document();
    let mut session = session_with(&document, first, 0);
    session.set_atomic_selection(rule).unwrap();

    let outcome = session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(outcome, SessionOutcome::DocumentChanged);
    assert!(session.document().node(rule).is_none());
    let children = session
        .document()
        .node(session.document().root())
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.len(), 2);

    // The selection converges to the gap the rule occupied.
    assert_eq!(
        session.selection().focus(),
        DocumentPosition::Gap(NodeGap::new(session.document().root(), 1)),
    );

    // Undo restores the block and reinstates the node selection.
    session.undo().unwrap();
    assert!(session.document().node(rule).is_some());
    assert_eq!(session.selection().focus(), DocumentPosition::Atomic(rule));

    // Redo removes it again.
    session.redo().unwrap();
    assert!(session.document().node(rule).is_none());
}

#[test]
fn delete_intent_removes_the_atomic_selection_too() {
    let (document, first, rule, _) = text_rule_text_document();
    let mut session = session_with(&document, first, 0);
    session.set_atomic_selection(rule).unwrap();

    session.apply_intent(&EditIntent::Delete).unwrap();
    assert!(session.document().node(rule).is_none());
}

#[test]
fn stale_atomic_selections_fail_validation() {
    let (document, first, rule, _) = text_rule_text_document();

    // Remove the rule directly; the stale node selection must not validate.
    let shrunk = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::RemoveNode { node: rule })
        .apply(&document)
        .unwrap();
    assert_eq!(
        DocumentSelection::collapsed(rule).validate(&shrunk),
        Err(SessionError::SelectionInvalid),
    );

    // The same holds for a selection whose endpoint mixes inline and
    // atomic positions.
    let mixed = DocumentSelection::new(
        inline_point(&shrunk, first, 0),
        DocumentPosition::Atomic(rule),
    );
    assert_eq!(mixed.validate(&shrunk), Err(SessionError::SelectionInvalid));
}

#[test]
fn atomic_endpoints_map_through_unrelated_text_edits() {
    let (document, first, rule, _) = text_rule_text_document();

    // A text replacement in the first paragraph does not disturb the
    // node selection identity.
    let replaced = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: first,
            range: TextBuffer::from_string("前".to_owned())
                .range(
                    TextBuffer::from_string("前".to_owned())
                        .offset_at(0)
                        .unwrap(),
                    TextBuffer::from_string("前".to_owned())
                        .offset_at(3)
                        .unwrap(),
                )
                .unwrap(),
            replacement: "前前".to_owned(),
        })
        .apply_with_changes(&document)
        .unwrap();
    let mapped = DocumentSelection::collapsed(rule).map_through(replaced.changes(), &document);
    assert!(mapped.is_ok());
    let mapped = mapped.unwrap();
    assert_eq!(mapped.focus(), DocumentPosition::Atomic(rule));
}
