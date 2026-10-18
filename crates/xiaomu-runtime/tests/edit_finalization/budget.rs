use super::*;
use xiaomu_core::document::{AttrValue, LinkMark};

#[test]
fn constructor_enforces_the_64_step_bound() {
    let (_, nodes) = fixture(&[""]);
    let mut tx = transaction();
    for _ in 0..64 {
        tx.push_step(TransactionStep::RemoveNode { node: nodes[0] });
    }
    // Payload admission is independent of later document-specific validity.
    assert!(EditFinalization::new(tx.clone(), None).is_ok());
    tx.push_step(TransactionStep::RemoveNode { node: nodes[0] });
    assert!(EditFinalization::new(tx, None).is_err());
}

#[test]
fn constructor_counts_owned_subtree_nodes_and_nested_attribute_depth() {
    let (document, nodes) = fixture(&[""]);
    let node = nodes[0];
    let tx = transaction().with_step(TransactionStep::RestoreSubtree {
        parent: document.root(),
        index: 0,
        root: node,
        nodes: vec![document.node(node).unwrap().clone(); 257],
    });
    assert!(EditFinalization::new(tx, None).is_err());
    let mut value = AttrValue::Null;
    for _ in 0..34 {
        value = AttrValue::List(vec![value]);
    }
    let attrs = NodeAttrs::new([("nested".into(), value)].into()).unwrap();
    assert!(
        EditFinalization::new(
            transaction().with_step(TransactionStep::SetNodeAttrs { node, attrs }),
            None,
        )
        .is_err()
    );
}

#[test]
fn byte_budget_covers_text_attributes_marks_node_content_origin_and_metadata() {
    let (document, nodes) = fixture(&[""]);
    let node = nodes[0];
    let huge = "x".repeat(64 * 1024 + 1);
    let attrs = NodeAttrs::new(
        [(
            "payload".into(),
            AttrValue::List(vec![AttrValue::Object(
                [("nested".into(), AttrValue::String(huge.clone()))].into(),
            )]),
        )]
        .into(),
    )
    .unwrap();
    let marks = MarkSet::new([Mark::Link(LinkMark::new(huge.clone(), None))]).unwrap();
    let mut metadata = transaction();
    metadata.set_metadata("payload", huge.clone()).unwrap();
    for tx in [
        transaction().with_step(replace(node, 0, 0, &huge)),
        transaction().with_step(TransactionStep::SetNodeAttrs { node, attrs }),
        transaction().with_step(TransactionStep::SetInlineAtomMarks { atom: node, marks }),
        append(&document, &huge),
        Transaction::new(TransactionOrigin::Extension(huge)),
        metadata,
    ] {
        assert!(EditFinalization::new(tx, None).is_err());
    }
}

#[test]
fn oversized_finalizer_response_rejects_without_history_marks_or_listener_changes() {
    let (document, nodes) = fixture(&[""]);
    let node = nodes[0];
    let (policy, observations) = policy(move |observation| {
        if text(&observation.candidate, node).contains('!') {
            finalized(
                transaction().with_step(replace(node, 0, 0, &"x".repeat(64 * 1024))),
                None,
            )
        } else {
            Ok(None)
        }
    });
    let mut session =
        DocumentSession::new_with_policy(document, caret(node, 0), Box::new(policy)).unwrap();
    bold(&mut session);
    insert(&mut session, "a").unwrap();
    let events = listen(&mut session);
    let snapshot = Snapshot::capture(&session, &events);
    assert!(matches!(
        insert(&mut session, "!"),
        Err(SessionError::Policy(_))
    ));
    snapshot.assert_unchanged(&session, &events);
    assert_eq!(observations.borrow().len(), 2);
    insert(&mut session, "b").unwrap();
    assert_eq!(text(session.document(), node), "ab");
    assert_eq!(session.history_depths(), (1, 0));
}
