//! Default intents keep mixed-inline seams, payloads and atomic history exact.

#[allow(dead_code)]
mod mixed_structure_support;
use mixed_structure_support::*;
use xiaomu_core::document::{Mark, NodeKind, NodeStoreBuilder, XiaomuDocument};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, PolicyError, SessionOutcome, SessionPolicy,
};

#[test]
fn split_every_unicode_boundary_and_atom_gap_preserves_payloads_and_history() {
    let mut builder = NodeStoreBuilder::new();
    let first = block(
        &mut builder,
        "a中🙂z",
        &[
            (0, true),
            (1, true),
            (1, true),
            (1, false),
            (4, false),
            (8, true),
            (9, true),
        ],
    );
    let document = finish(builder, &[first]);
    for raw in [0, 1, 4, 8, 9] {
        for ordinal in 0..=inline(&document, first).atom_count_at(offset(raw)) {
            let selection = DocumentSelection::collapsed(point(first, raw, ordinal));
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            let events = listen(&mut session);
            session
                .apply_intent(&EditIntent::SetMark {
                    mark: Mark::Underline,
                })
                .unwrap();
            let marks = session.stored_marks().cloned();
            assert_eq!(
                session.apply_intent(&EditIntent::SplitBlock).unwrap(),
                SessionOutcome::DocumentChanged
            );
            let tail = children(session.document(), document.root())[1];
            assert_eq!(focus(&session), point(tail, 0, 0));
            assert_eq!(session.stored_marks(), marks.as_ref());
            assert_eq!(text(session.document(), first), &"a中🙂z"[..raw]);
            assert_eq!(text(session.document(), tail), &"a中🙂z"[raw..]);
            let split_index = inline(&document, first)
                .atoms()
                .iter()
                .filter(|atom| atom.text_offset().as_usize() < raw)
                .count()
                + ordinal;
            let original_atoms = atoms(&document, first);
            assert_eq!(
                atoms(session.document(), first),
                original_atoms[..split_index]
            );
            assert_eq!(
                atoms(session.document(), tail),
                original_atoms[split_index..]
            );
            for (old, new) in inline(&document, first).atoms()[split_index..]
                .iter()
                .zip(inline(session.document(), tail).atoms())
            {
                assert_eq!(
                    new.text_offset().as_usize(),
                    old.text_offset().as_usize() - raw
                );
            }
            assert_payloads(&document, session.document(), &original_atoms);
            round_trip(&mut session, &document, selection, &events);
        }
    }
}

#[test]
fn reverse_mixed_range_split_removes_only_selected_atoms() {
    let mut builder = NodeStoreBuilder::new();
    let first = block(
        &mut builder,
        "a中🙂z",
        &[(1, true), (1, false), (4, true), (8, true), (8, false)],
    );
    let document = finish(builder, &[first]);
    let original_atoms = atoms(&document, first);
    let selection = DocumentSelection::new(point(first, 8, 1), point(first, 1, 1));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let events = listen(&mut session);
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let tail = focus(&session).node_id();
    assert_eq!(text(session.document(), first), "a");
    assert_eq!(text(session.document(), tail), "z");
    assert_eq!(atoms(session.document(), first), [original_atoms[0]]);
    assert_eq!(atoms(session.document(), tail), [original_atoms[4]]);
    assert_eq!(
        inline(session.document(), tail).atoms()[0].text_offset(),
        offset(0)
    );
    for removed in &original_atoms[1..4] {
        assert!(session.document().node(*removed).is_none());
    }
    assert_payloads(
        &document,
        session.document(),
        &[original_atoms[0], original_atoms[4]],
    );
    round_trip(&mut session, &document, selection, &events);
}

#[test]
fn split_selection_between_same_boundary_breaks_and_extensions() {
    for reverse in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let first = block(
            &mut builder,
            "",
            &[(0, true), (0, false), (0, true), (0, false)],
        );
        let document = finish(builder, &[first]);
        let original_atoms = atoms(&document, first);
        let (start, end) = (point(first, 0, 1), point(first, 0, 3));
        let selection = if reverse {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        let events = listen(&mut session);
        session.apply_intent(&EditIntent::SplitBlock).unwrap();
        assert_eq!(atoms(session.document(), first), [original_atoms[0]]);
        assert_eq!(
            atoms(session.document(), focus(&session).node_id()),
            [original_atoms[3]]
        );
        round_trip(&mut session, &document, selection, &events);
    }
}

#[test]
fn join_and_backspace_land_after_first_blocks_trailing_atoms() {
    for intent in [EditIntent::JoinWithPrevious, EditIntent::Backspace] {
        for first_text in ["a", ""] {
            let mut builder = NodeStoreBuilder::new();
            let seam = first_text.len();
            let first = block(
                &mut builder,
                first_text,
                &[(seam, true), (seam, true), (seam, false)],
            );
            let second = block(&mut builder, "中", &[(0, false), (0, true), (3, true)]);
            let document = finish(builder, &[first, second]);
            let selection = DocumentSelection::collapsed(point(second, 0, 0));
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            session
                .apply_intent(&EditIntent::SetMark {
                    mark: Mark::Underline,
                })
                .unwrap();
            let pending = session.stored_marks().cloned();
            let events = listen(&mut session);
            session.apply_intent(&intent).unwrap();
            assert_eq!(focus(&session), point(first, seam, 3));
            assert_eq!(children(session.document(), document.root()), [first]);
            assert_eq!(text(session.document(), first), format!("{first_text}中"));
            let joined_atoms = [atoms(&document, first), atoms(&document, second)].concat();
            assert_eq!(atoms(session.document(), first), joined_atoms);
            assert_payloads(&document, session.document(), &joined_atoms);
            if matches!(intent, EditIntent::JoinWithPrevious) {
                assert_eq!(session.stored_marks(), None);
            } else {
                assert_eq!(session.stored_marks(), pending.as_ref());
            }
            round_trip(&mut session, &document, selection, &events);
        }
    }
}

#[test]
fn list_enter_only_break_items_split_instead_of_exiting_the_list() {
    for ordinal in 0..=3 {
        let mut builder = NodeStoreBuilder::new();
        let first = block(&mut builder, "", &[(0, true), (0, true), (0, false)]);
        let item = container(&mut builder, NodeKind::ListItem, &[first]);
        let list = container(&mut builder, NodeKind::BulletList, &[item]);
        let document = finish(builder, &[list]);
        let selection = DocumentSelection::collapsed(point(first, 0, ordinal));
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        let events = listen(&mut session);
        session.apply_intent(&EditIntent::SplitBlock).unwrap();
        assert_eq!(children(session.document(), document.root()), [list]);
        let items = children(session.document(), list);
        assert_eq!(items.len(), 2);
        let tail = children(session.document(), items[1])[0];
        assert_eq!(focus(&session), point(tail, 0, 0));
        assert_eq!(
            atoms(session.document(), first),
            atoms(&document, first)[..ordinal]
        );
        assert_eq!(
            atoms(session.document(), tail),
            atoms(&document, first)[ordinal..]
        );
        assert_payloads(&document, session.document(), &atoms(&document, first));
        round_trip(&mut session, &document, selection, &events);
    }
}

#[test]
fn cross_container_backspace_preserves_mixed_payloads_and_join_ordinal() {
    for source_is_item in [false, true] {
        for first_mixed in [false, true] {
            let mut builder = NodeStoreBuilder::new();
            let first_atoms: &[(usize, bool)] = if first_mixed {
                &[(1, true), (1, false)]
            } else {
                &[]
            };
            let first = block(&mut builder, "a", first_atoms);
            let second = block(&mut builder, "b", &[(0, true), (0, false)]);
            let first_item = container(&mut builder, NodeKind::ListItem, &[first]);
            let root_children = if source_is_item {
                let second_item = container(&mut builder, NodeKind::ListItem, &[second]);
                vec![container(
                    &mut builder,
                    NodeKind::BulletList,
                    &[first_item, second_item],
                )]
            } else {
                vec![
                    container(&mut builder, NodeKind::BulletList, &[first_item]),
                    second,
                ]
            };
            let document = finish(builder, &root_children);
            let selection = DocumentSelection::collapsed(point(second, 0, 0));
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            let events = listen(&mut session);
            session.apply_intent(&EditIntent::Backspace).unwrap();
            assert_eq!(
                focus(&session),
                point(first, 1, usize::from(first_mixed) * 2)
            );
            assert_eq!(text(session.document(), first), "ab");
            let joined_atoms = [atoms(&document, first), atoms(&document, second)].concat();
            assert_eq!(atoms(session.document(), first), joined_atoms);
            assert_payloads(&document, session.document(), &joined_atoms);
            assert!(session.document().node(second).is_none());
            round_trip(&mut session, &document, selection, &events);
        }
    }
}

struct KeepNodeCount(usize);
impl SessionPolicy for KeepNodeCount {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.node_count() != self.0 {
            Err(PolicyError::new("structure rejected"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn rejected_mixed_split_and_join_preserve_document_selection_marks_history_and_listeners() {
    for intent in [
        EditIntent::SplitBlock,
        EditIntent::JoinWithPrevious,
        EditIntent::Backspace,
    ] {
        let mut builder = NodeStoreBuilder::new();
        let first = block(&mut builder, "a", &[(1, true)]);
        let second = block(&mut builder, "b", &[(0, true), (0, false)]);
        let document = finish(builder, &[first, second]);
        let selection = DocumentSelection::collapsed(point(second, 0, 0));
        let mut session = DocumentSession::new_with_policy(
            document.clone(),
            selection,
            Box::new(KeepNodeCount(document.node_count())),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::SetMark {
                mark: Mark::Underline,
            })
            .unwrap();
        let marks = session.stored_marks().cloned();
        let events = listen(&mut session);
        assert!(session.apply_intent(&intent).is_err());
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.document().revision(), document.revision());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), (0, 0));
        assert!(events.borrow().is_empty());
    }
}
