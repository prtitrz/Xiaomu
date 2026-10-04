//! Text and individual inline atoms share one range-formatting transaction.

#[path = "support/mixed_marks.rs"]
mod support;

use support::*;
use xiaomu_core::document::{
    AtomKind, LinkAttributes, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeKind,
    NodeStoreBuilder, StringAttribute, TextStyleAttributes, TextStyleMark,
};
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent, SessionOutcome};

#[test]
fn only_break_selection_formats_each_same_byte_atom_and_roundtrips_history() {
    let (document, node, atoms) = fixture(
        "",
        MarkSet::empty(),
        &[
            (0, AtomKind::hard_break(), marks([Mark::Italic])),
            (0, AtomKind::hard_break(), MarkSet::empty()),
            (0, AtomKind::new("hardBreak").unwrap(), marks([Mark::Code])),
        ],
    );
    for ordinal in 0..3 {
        for reversed in [false, true] {
            let a = point(&document, node, 0, ordinal);
            let b = point(&document, node, 0, ordinal + 1);
            let selection = if reversed {
                DocumentSelection::new(b, a)
            } else {
                DocumentSelection::new(a, b)
            };
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            let counts = listen(&mut session);
            assert!(!session.selection().is_collapsed());
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                .unwrap();
            let after = session.document().clone();
            assert_eq!(session.selection(), selection);
            assert_eq!(session.history_depths(), (1, 0));
            assert_eq!(counts.get(), (1, 0));
            assert_eq!(session.stored_marks(), None);
            assert!(inline(session.document(), node).runs().is_empty());
            for (index, atom) in atoms.iter().enumerate() {
                assert_eq!(
                    atom_marks(session.document(), *atom).contains(MarkKind::Bold),
                    index == ordinal
                );
                let old = document.node(*atom).unwrap();
                let new = session.document().node(*atom).unwrap();
                assert_eq!(old.kind(), new.kind());
                assert_eq!(old.attrs(), new.attrs());
                assert_eq!(
                    old.content().as_inline_atom().unwrap().fallback_text(),
                    new.content().as_inline_atom().unwrap().fallback_text()
                );
            }
            assert_eq!(
                inline(session.document(), node).atoms(),
                inline(&document, node).atoms()
            );
            session.undo().unwrap();
            assert_eq!(session.document().store(), document.store());
            assert_eq!(session.selection(), selection);
            session.redo().unwrap();
            assert_eq!(session.document().store(), after.store());
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                .unwrap();
            assert_eq!(session.document().store(), document.store());
        }
    }
}

#[test]
fn every_unicode_and_atom_gap_pair_uses_exact_half_open_membership_in_both_directions() {
    let text = "中\n🙂尾";
    let atom_specs =
        [0, 3, 3, 3, 4, 11, 11].map(|raw| (raw, AtomKind::hard_break(), marks([Mark::Italic])));
    let (document, node, atoms) = fixture(text, marks([Mark::Italic]), &atom_specs);
    let mut gaps = Vec::new();
    for raw in [0, 3, 4, 8, 11] {
        let offset = inline(&document, node).offset_at(raw).unwrap();
        for ordinal in 0..=inline(&document, node).atom_count_at(offset) {
            gaps.push((raw, ordinal));
        }
    }
    for (index, &start) in gaps.iter().enumerate() {
        for &end in &gaps[index + 1..] {
            for reversed in [false, true] {
                let a = point(&document, node, start.0, start.1);
                let b = point(&document, node, end.0, end.1);
                let selection = if reversed {
                    DocumentSelection::new(b, a)
                } else {
                    DocumentSelection::new(a, b)
                };
                let mut session = DocumentSession::new(document.clone(), selection).unwrap();
                session
                    .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
                    .unwrap();
                let next = session.document();
                for (raw, _) in text.char_indices() {
                    assert_eq!(
                        text_marks(next, node, raw).contains(MarkKind::Bold),
                        raw >= start.0 && raw < end.0,
                        "text {raw}: {start:?}..{end:?}"
                    );
                }
                let atom_keys = [(0, 0), (3, 0), (3, 1), (3, 2), (4, 0), (11, 0), (11, 1)];
                for (atom, key) in atoms.iter().zip(atom_keys) {
                    assert_eq!(
                        atom_marks(next, *atom).contains(MarkKind::Bold),
                        key >= start && key < end,
                        "atom {key:?}: {start:?}..{end:?}"
                    );
                }
                assert_eq!(session.selection(), selection);
                assert_eq!(inline(next, node).atoms(), inline(&document, node).atoms());
                let revision = next.revision();
                assert_eq!(
                    session
                        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
                        .unwrap(),
                    SessionOutcome::NoChange
                );
                assert_eq!(session.document().revision(), revision);
                session
                    .apply_intent(&EditIntent::RemoveMark {
                        kind: MarkKind::Bold,
                    })
                    .unwrap();
                assert_eq!(session.document().store(), document.store());
                session.undo().unwrap();
                session.undo().unwrap();
                assert_eq!(session.document().store(), document.store());
            }
        }
    }
}

#[test]
fn toggle_decision_includes_atoms_and_text_together() {
    for (text_marks, break_marks) in [
        (marks([Mark::Bold]), MarkSet::empty()),
        (MarkSet::empty(), marks([Mark::Bold])),
    ] {
        let (document, node, atoms) = fixture(
            "a\n",
            text_marks,
            &[(1, AtomKind::hard_break(), break_marks)],
        );
        let selection =
            DocumentSelection::new(point(&document, node, 0, 0), point(&document, node, 2, 0));
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        assert!(text_marks_at_all(&session, node, MarkKind::Bold));
        assert!(atom_marks(session.document(), atoms[0]).contains(MarkKind::Bold));
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        assert!(!text_marks_at_all(&session, node, MarkKind::Bold));
        assert!(!atom_marks(session.document(), atoms[0]).contains(MarkKind::Bold));
        session.undo().unwrap();
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
    }
}

fn text_marks_at_all(
    session: &DocumentSession,
    node: xiaomu_core::document::NodeId,
    kind: MarkKind,
) -> bool {
    inline(session.document(), node)
        .runs()
        .iter()
        .all(|run| run.marks().contains(kind))
}

#[test]
fn exact_attributed_marks_and_code_stay_independent_and_survive_v10_copy() {
    let (document, node, atoms) = fixture(
        "中\n🙂",
        marks([Mark::Italic]),
        &[(3, AtomKind::hard_break(), marks([Mark::Code]))],
    );
    let selection =
        DocumentSelection::new(point(&document, node, 3, 0), point(&document, node, 3, 1));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let link = Mark::Link(LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href("原值🙂".into())
            .with_target(StringAttribute::Null)
            .with_title("".into()),
    ));
    let style = Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(StringAttribute::Null)
            .with_font_family("宋体, serif".into())
            .with_font_size("calc(1em + 2px)".into()),
    ));
    for mark in [link.clone(), style.clone(), Mark::Bold] {
        session.apply_intent(&EditIntent::SetMark { mark }).unwrap();
    }
    assert_eq!(
        atom_marks(session.document(), atoms[0]),
        &marks([Mark::Code, link, style, Mark::Bold])
    );
    assert_eq!(inline(session.document(), node), inline(&document, node));
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "\n");
    let encoded = encode_metadata(&slice).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap()["version"],
        10
    );
    assert_eq!(
        decode_metadata(slice.plain_text(), &encoded).unwrap(),
        slice
    );
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    assert!(atom_marks(session.document(), atoms[0]).contains(MarkKind::Code));
    assert!(atom_marks(session.document(), atoms[0]).contains(MarkKind::TextStyle));
    for _ in 0..4 {
        session.undo().unwrap();
    }
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn cross_block_reverse_range_includes_empty_atom_blocks_and_exact_endpoint_atoms() {
    let mut builder = NodeStoreBuilder::new();
    let empty = MarkSet::empty();
    let (head, head_atoms) = block(
        &mut builder,
        "中",
        empty.clone(),
        &[
            (3, AtomKind::hard_break(), empty.clone()),
            (3, AtomKind::hard_break(), empty.clone()),
        ],
    );
    let (middle, middle_atoms) = block(
        &mut builder,
        "",
        empty.clone(),
        &[(0, AtomKind::hard_break(), empty.clone())],
    );
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([middle]),
        )
        .unwrap();
    let (tail, tail_atoms) = block(
        &mut builder,
        "🙂\n尾",
        empty.clone(),
        &[
            (0, AtomKind::hard_break(), empty.clone()),
            (0, AtomKind::hard_break(), empty),
        ],
    );
    let document = finish(builder, vec![head, quote, tail]);
    let selection =
        DocumentSelection::new(point(&document, tail, 0, 1), point(&document, head, 3, 1));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let counts = listen(&mut session);
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    for (atom, expected) in [
        (head_atoms[0], false),
        (head_atoms[1], true),
        (middle_atoms[0], true),
        (tail_atoms[0], true),
        (tail_atoms[1], false),
    ] {
        assert_eq!(
            atom_marks(session.document(), atom).contains(MarkKind::Bold),
            expected
        );
    }
    assert_eq!(session.document().node(head), document.node(head));
    assert_eq!(session.document().node(tail), document.node(tail));
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (1, 0));
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(session.document().store(), document.store());
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
}

#[test]
fn cross_block_text_and_atoms_share_one_global_toggle_decision() {
    let mut builder = NodeStoreBuilder::new();
    let bold = marks([Mark::Bold]);
    let (head, head_atoms) = block(
        &mut builder,
        "a中",
        bold.clone(),
        &[(1, AtomKind::hard_break(), bold.clone())],
    );
    let (tail, tail_atoms) = block(
        &mut builder,
        "🙂\n尾",
        bold,
        &[(4, AtomKind::hard_break(), MarkSet::empty())],
    );
    let document = finish(builder, vec![head, tail]);
    let selection =
        DocumentSelection::new(point(&document, head, 1, 0), point(&document, tail, 5, 0));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    for atom in [head_atoms[0], tail_atoms[0]] {
        assert!(atom_marks(session.document(), atom).contains(MarkKind::Bold));
    }
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    for atom in [head_atoms[0], tail_atoms[0]] {
        assert!(!atom_marks(session.document(), atom).contains(MarkKind::Bold));
    }
    assert!(text_marks(session.document(), head, 0).contains(MarkKind::Bold));
    assert!(!text_marks(session.document(), head, 1).contains(MarkKind::Bold));
    assert!(!text_marks(session.document(), tail, 0).contains(MarkKind::Bold));
    assert!(text_marks(session.document(), tail, 5).contains(MarkKind::Bold));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn collapsed_nonzero_atom_gap_toggle_updates_pending_marks_and_next_typing_only() {
    for kind in [AtomKind::hard_break(), AtomKind::new("mention").unwrap()] {
        // An empty built-in break is an actual unmarked child. Legacy
        // unmarked extensions remain transparent to text inheritance.
        let expected = if kind.is_hard_break() {
            marks([Mark::Bold])
        } else {
            marks([Mark::Italic, Mark::Bold])
        };
        let (document, node, atoms) = fixture(
            "a中",
            marks([Mark::Italic]),
            &[
                (1, kind.clone(), MarkSet::empty()),
                (1, kind, marks([Mark::Code])),
            ],
        );
        let caret = DocumentSelection::collapsed(point(&document, node, 1, 1));
        let mut session = DocumentSession::new(document.clone(), caret).unwrap();
        let counts = listen(&mut session);
        assert_eq!(
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                .unwrap(),
            SessionOutcome::NoChange
        );
        assert_eq!(session.selection(), caret);
        assert_eq!(session.stored_marks(), Some(&expected));
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.document().revision(), document.revision());
        assert_eq!(session.history_depths(), (0, 0));
        assert_eq!(counts.get(), (0, 0));
        session
            .apply_intent(&EditIntent::InsertText { text: "拼".into() })
            .unwrap();
        let after = session.document().clone();
        assert_eq!(text_marks(&after, node, 1), &expected);
        assert_eq!(inline(&after, node).atoms()[0].text_offset().as_usize(), 1);
        assert_eq!(inline(&after, node).atoms()[1].text_offset().as_usize(), 4);
        for atom in atoms {
            assert_eq!(after.node(atom), document.node(atom));
        }
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.selection(), caret);
        session.redo().unwrap();
        assert_eq!(session.document().store(), after.store());
    }
}

struct FrozenAtom {
    atom: xiaomu_core::document::NodeId,
    marks: MarkSet,
}

impl xiaomu_runtime::session::SessionPolicy for FrozenAtom {
    fn validate_document(
        &self,
        document: &xiaomu_core::document::XiaomuDocument,
    ) -> Result<(), xiaomu_runtime::session::PolicyError> {
        if atom_marks(document, self.atom) != &self.marks {
            return Err(xiaomu_runtime::session::PolicyError::new(
                "atom mark candidate rejected",
            ));
        }
        Ok(())
    }
}

#[test]
fn rejected_atom_mark_candidate_preserves_pending_marks_selection_listeners_and_typing_group() {
    use xiaomu_runtime::session::SessionError;

    let initial_atom_marks = marks([Mark::Code]);
    let (document, node, atoms) = fixture(
        "a",
        MarkSet::empty(),
        &[(0, AtomKind::hard_break(), initial_atom_marks.clone())],
    );
    let initial_caret = DocumentSelection::collapsed(point(&document, node, 1, 0));
    let mut session = DocumentSession::new_with_policy(
        document.clone(),
        initial_caret,
        Box::new(FrozenAtom {
            atom: atoms[0],
            marks: initial_atom_marks,
        }),
    )
    .unwrap();
    let counts = listen(&mut session);
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = session.document().clone();
    let before_selection = session.selection();
    let before_marks = session.stored_marks().cloned();
    let before_counts = counts.get();
    let target = DocumentSelection::new(point(&before, node, 0, 0), point(&before, node, 0, 1));
    for intent in [
        EditIntent::ToggleMark { mark: Mark::Italic },
        EditIntent::SetMark { mark: Mark::Italic },
        EditIntent::RemoveMark {
            kind: MarkKind::Code,
        },
    ] {
        assert!(matches!(
            session.apply_intent_with_selection(target, &intent),
            Err(SessionError::Policy(_))
        ));
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), before_selection);
        assert_eq!(session.stored_marks(), before_marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), before_counts);
    }
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(
        session.history_depths(),
        (1, 0),
        "failed formatting must leave typing coalescing open"
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), initial_caret);
}

#[test]
fn only_atom_idempotent_explicit_marks_keep_revision_history_and_listeners_unchanged() {
    let (document, node, atoms) = fixture(
        "",
        MarkSet::empty(),
        &[(0, AtomKind::hard_break(), marks([Mark::Bold]))],
    );
    let selection =
        DocumentSelection::new(point(&document, node, 0, 0), point(&document, node, 0, 1));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let counts = listen(&mut session);
    for intent in [
        EditIntent::SetMark { mark: Mark::Bold },
        EditIntent::RemoveMark {
            kind: MarkKind::Italic,
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent).unwrap(),
            SessionOutcome::NoChange
        );
    }
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(session.stored_marks(), None);
    assert_eq!(counts.get(), (0, 0));
    assert_eq!(
        atom_marks(session.document(), atoms[0]),
        &marks([Mark::Bold])
    );
}
