//! Plain-text paste preserves mixed caret gaps and deletes atom ranges atomically.

#[allow(dead_code)]
mod mixed_structure_support;
use mixed_structure_support::*;
use xiaomu_core::document::{Mark, NodeId, NodeStoreBuilder, XiaomuDocument};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, PolicyError, SessionOutcome, SessionPolicy,
};

fn fixture(text: &str, raw: usize) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = block(
        &mut builder,
        text,
        &[(raw, true), (raw, true), (raw, false)],
    );
    (finish(builder, &[node]), node)
}

#[test]
fn paste_before_between_and_after_consecutive_breaks_and_extension_keeps_exact_payloads() {
    for (original, raw) in [("a中", 1), ("", 0)] {
        let (document, node) = fixture(original, raw);
        let ids = atoms(&document, node);
        let pasted = "🙂\n中";
        for ordinal in 0..=ids.len() {
            let selection = DocumentSelection::collapsed(point(node, raw, ordinal));
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            session
                .apply_intent(&EditIntent::SetMark {
                    mark: Mark::Underline,
                })
                .unwrap();
            let marks = session.stored_marks().cloned();
            let events = listen(&mut session);
            assert_eq!(
                session
                    .apply_intent(&EditIntent::PasteText {
                        text: pasted.into()
                    })
                    .unwrap(),
                SessionOutcome::DocumentChanged
            );
            assert_eq!(
                text(session.document(), node),
                format!("{}{pasted}{}", &original[..raw], &original[raw..])
            );
            assert_eq!(focus(&session), point(node, raw + pasted.len(), 0));
            assert_eq!(session.stored_marks(), marks.as_ref());
            assert_eq!(atoms(session.document(), node), ids);
            for (index, placement) in inline(session.document(), node).atoms().iter().enumerate() {
                let expected = if index < ordinal {
                    raw
                } else {
                    raw + pasted.len()
                };
                assert_eq!(placement.text_offset().as_usize(), expected);
            }
            assert_payloads(&document, session.document(), &ids);
            round_trip(&mut session, &document, selection, &events);
        }
    }
}

#[test]
fn paste_replaces_only_break_ranges_in_both_directions_without_eating_outside_atoms() {
    for all_breaks in [false, true] {
        for reverse in [false, true] {
            for replacement in ["中🙂", ""] {
                let mut builder = NodeStoreBuilder::new();
                let node = block(
                    &mut builder,
                    "",
                    &[(0, true), (0, true), (0, true), (0, false)],
                );
                let document = finish(builder, &[node]);
                let ids = atoms(&document, node);
                // An interior range covers only a typed hard break; the
                // wider range covers all typed breaks, leaving the extension.
                let (start, end) = if all_breaks { (0, 3) } else { (1, 2) };
                let (a, f) = (point(node, 0, start), point(node, 0, end));
                let selection = if reverse {
                    DocumentSelection::new(f, a)
                } else {
                    DocumentSelection::new(a, f)
                };
                let mut session = DocumentSession::new(document.clone(), selection).unwrap();
                let events = listen(&mut session);
                session
                    .apply_intent(&EditIntent::PasteText {
                        text: replacement.into(),
                    })
                    .unwrap();
                let expected_ids = [&ids[..start], &ids[end..]].concat();
                assert_eq!(atoms(session.document(), node), expected_ids);
                assert_eq!(text(session.document(), node), replacement);
                let after = if replacement.is_empty() {
                    point(node, 0, start)
                } else {
                    point(node, replacement.len(), 0)
                };
                assert_eq!(focus(&session), after);
                for removed in &ids[start..end] {
                    assert!(session.document().node(*removed).is_none());
                }
                assert_payloads(&document, session.document(), &expected_ids);
                round_trip(&mut session, &document, selection, &events);
            }
        }
    }
}

#[test]
fn reverse_range_paste_replaces_text_and_selected_atoms_only() {
    let mut builder = NodeStoreBuilder::new();
    let node = block(
        &mut builder,
        "a中🙂z",
        &[(1, true), (1, false), (4, true), (8, true), (8, false)],
    );
    let document = finish(builder, &[node]);
    let ids = atoms(&document, node);
    let selection = DocumentSelection::new(point(node, 8, 1), point(node, 1, 1));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let events = listen(&mut session);
    session
        .apply_intent(&EditIntent::PasteText {
            text: "X\nY".into(),
        })
        .unwrap();
    assert_eq!(text(session.document(), node), "aX\nYz");
    assert_eq!(atoms(session.document(), node), [ids[0], ids[4]]);
    assert_eq!(
        inline(session.document(), node).atoms()[0].text_offset(),
        offset(1)
    );
    assert_eq!(
        inline(session.document(), node).atoms()[1].text_offset(),
        offset(4)
    );
    assert_eq!(focus(&session), point(node, 4, 0));
    assert_payloads(&document, session.document(), &[ids[0], ids[4]]);
    round_trip(&mut session, &document, selection, &events);
}

#[test]
fn plain_text_paste_retains_legacy_range_noop_and_isolated_history() {
    let mut builder = NodeStoreBuilder::new();
    let node = block(&mut builder, "a中z", &[]);
    let document = finish(builder, &[node]);
    let selection = DocumentSelection::new(point(node, 4, 0), point(node, 1, 0));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let events = listen(&mut session);
    session
        .apply_intent(&EditIntent::PasteText {
            text: "🙂".into()
        })
        .unwrap();
    assert_eq!(text(session.document(), node), "a🙂z");
    assert_eq!(focus(&session), point(node, 5, 0));
    round_trip(&mut session, &document, selection, &events);
    let before_revision = session.document().revision();
    assert_eq!(
        session
            .apply_intent(&EditIntent::PasteText { text: "".into() })
            .unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.document().revision(), before_revision);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(events.borrow().len(), 3);

    // A successful paste remains its own unit between adjacent typing.
    for intent in [
        EditIntent::InsertText { text: "1".into() },
        EditIntent::PasteText { text: "2".into() },
        EditIntent::InsertText { text: "3".into() },
    ] {
        session.apply_intent(&intent).unwrap();
    }
    assert_eq!(session.history_depths(), (4, 0));
}

#[test]
fn paste_reuses_existing_text_input_mark_semantics_without_changing_inheritance() {
    for explicit_marks in [false, true] {
        for ordinal in 0..=3 {
            let (document, node) = fixture("ab", 1);
            let selection = DocumentSelection::collapsed(point(node, 1, ordinal));
            let make = || {
                let mut session = DocumentSession::new(document.clone(), selection).unwrap();
                if explicit_marks {
                    session
                        .apply_intent(&EditIntent::SetMark {
                            mark: Mark::Underline,
                        })
                        .unwrap();
                }
                session
            };
            let mut typed = make();
            let mut pasted = make();
            typed
                .apply_intent(&EditIntent::InsertText { text: "中".into() })
                .unwrap();
            pasted
                .apply_intent(&EditIntent::PasteText { text: "中".into() })
                .unwrap();
            assert_eq!(pasted.document().store(), typed.document().store());
            assert_eq!(pasted.selection(), typed.selection());
            assert_eq!(pasted.stored_marks(), typed.stored_marks());
        }
    }
}

struct RejectForbidden;
impl SessionPolicy for RejectForbidden {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.store().iter().any(|node| {
            node.content().as_inline().is_some_and(|inline| {
                inline
                    .runs()
                    .iter()
                    .any(|run| run.text().as_str().contains('🚫'))
            })
        }) {
            Err(PolicyError::new("forbidden text"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn rejected_mixed_paste_preserves_redo_selection_marks_revision_and_notifications() {
    let (document, node) = fixture("", 0);
    let selection = DocumentSelection::collapsed(point(node, 0, 1));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(RejectForbidden))
            .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "ok".into() })
        .unwrap();
    let after_typing = session.document().clone();
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::SetMark {
            mark: Mark::Underline,
        })
        .unwrap();
    let marks = session.stored_marks().cloned();
    let revision = session.document().revision();
    let events = listen(&mut session);
    assert!(
        session
            .apply_intent(&EditIntent::PasteText {
                text: "🚫".into()
            })
            .is_err()
    );
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), revision);
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(session.history_depths(), (0, 1));
    assert!(events.borrow().is_empty());
    session.redo().unwrap();
    assert_eq!(session.document().store(), after_typing.store());
    assert_eq!(session.history_depths(), (1, 0));
}

#[test]
fn rejected_paste_restores_an_open_mixed_typing_group() {
    let (document, node) = fixture("", 0);
    let selection = DocumentSelection::collapsed(point(node, 0, 1));
    let mut session =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(RejectForbidden))
            .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    assert!(
        session
            .apply_intent(&EditIntent::PasteText {
                text: "🚫".into()
            })
            .is_err()
    );
    session
        .apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(text(session.document(), node), "ab");
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
}
