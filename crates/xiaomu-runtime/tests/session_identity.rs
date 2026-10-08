//! Session-lifetime tokens remain independent of local document revisions.
use std::{cell::RefCell, rc::Rc};

use xiaomu_core::{
    document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::InlinePoint,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

fn session() -> DocumentSession {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    DocumentSession::new(
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(paragraph)),
    )
    .unwrap()
}

#[test]
fn new_sessions_over_the_same_snapshot_have_distinct_lifetimes() {
    let first = session();
    let second = DocumentSession::new(first.document().clone(), first.selection()).unwrap();
    assert_eq!(first.document().revision(), second.document().revision());
    assert_ne!(first.identity(), second.identity());
    let token = first.identity();
    assert_eq!(token, token.clone());
    drop(first);
    assert_ne!(token, second.identity());
}

#[test]
fn identity_survives_edits_history_selection_and_moves() {
    let mut session = session();
    let token = session.identity();
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    session.undo().unwrap();
    session.redo().unwrap();
    assert_eq!(session.identity(), token);
    let moved = session;
    assert_eq!(moved.identity(), token);
}

#[test]
fn in_place_replacement_in_the_same_container_invalidates_old_token() {
    let shared = Rc::new(RefCell::new(session()));
    let old_container = shared.clone();
    let old_token = shared.borrow().identity();
    let replacement = {
        let old = shared.borrow();
        DocumentSession::new(old.document().clone(), old.selection()).unwrap()
    };
    *shared.borrow_mut() = replacement;
    assert!(Rc::ptr_eq(&shared, &old_container));
    assert_ne!(shared.borrow().identity(), old_token);
}
