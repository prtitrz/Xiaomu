//! Mixed caret/selection mark inheritance, matched to actual factory input semantics.
//! Plain clipboard paste uses context/caret marks; typed range replacement uses
//! marksAcross. The latter oracle's insertContent paths are not clipboard paste.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkMark, Mark, MarkKind,
    MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, IntentDisposition, PolicyError, SessionContext,
    SessionOutcome, SessionPolicy,
};

fn marks(mark: Mark) -> MarkSet {
    MarkSet::new([mark]).unwrap()
}
fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}
fn point(node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, CursorAffinity::Before)
}
fn marked_payload() -> MarkSet {
    MarkSet::new([
        Mark::Code,
        Mark::Link(LinkMark::new(
            "https://exact.example/a",
            Some("title🙂".into()),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color("var(--color)".into())
                .with_font_family("未知".into()),
        )),
    ])
    .unwrap()
}
fn fixture(only_atoms: bool) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let raw = if only_atoms { 0 } else { 2 };
    let mut placements = vec![];
    for atom_marks in [marks(Mark::Italic), MarkSet::empty(), marked_payload()] {
        let atom = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::hard_break()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::hard_break().with_marks(atom_marks)),
            )
            .unwrap();
        placements.push(InlineAtomPlacement::new(atom, offset(raw)));
    }
    let runs = if only_atoms {
        vec![]
    } else {
        vec![
            TextRun::new("ab", marks(Mark::Bold)).unwrap(),
            TextRun::new("cd", marks(Mark::Underline)).unwrap(),
        ]
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
fn inserted_marks(session: &DocumentSession, node: NodeId, raw: usize) -> MarkSet {
    let mut end = 0;
    for run in session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
    {
        end += run.len_bytes();
        if raw < end {
            return run.marks().clone();
        }
    }
    panic!("inserted text must exist")
}
fn check_undo(
    session: &mut DocumentSession,
    before: &XiaomuDocument,
    selection: DocumentSelection,
) {
    let after = session.document().clone();
    let after_selection = session.selection();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), None);
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), after_selection);
}
struct CheckContext {
    expected: MarkSet,
    pending: Option<MarkSet>,
}
impl SessionPolicy for CheckContext {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Delete) {
            return Ok(IntentDisposition::StoredMarks(self.pending.clone()));
        }
        assert_eq!(
            context.effective_typing_marks(),
            Some(self.expected.clone())
        );
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn every_marked_break_gap_queries_and_real_typing_paste_ime_agree() {
    for only_atoms in [false, true] {
        let (document, node) = fixture(only_atoms);
        let raw = if only_atoms { 0 } else { 2 };
        let leading = if only_atoms {
            marks(Mark::Italic)
        } else {
            marks(Mark::Bold)
        };
        for (ordinal, expected) in [
            leading,
            marks(Mark::Italic),
            MarkSet::empty(),
            marked_payload(),
        ]
        .into_iter()
        .enumerate()
        {
            let at = point(node, raw, ordinal);
            let selection = DocumentSelection::collapsed(at);
            for intent in [
                EditIntent::InsertText {
                    text: "🙂".into()
                },
                EditIntent::PasteText {
                    text: "🙂".into()
                },
                EditIntent::CommitComposition {
                    range: TextRange::new(offset(raw), offset(raw)).unwrap(),
                    text: "🙂".into(),
                },
            ] {
                let mut session = DocumentSession::new_with_policy(
                    document.clone(),
                    selection,
                    Box::new(CheckContext {
                        expected: expected.clone(),
                        pending: None,
                    }),
                )
                .unwrap();
                assert_eq!(session.effective_input_marks_at(at).unwrap(), expected);
                assert_eq!(
                    session.effective_input_marks(node, offset(raw)).unwrap(),
                    expected
                );
                assert_eq!(
                    session
                        .effective_composition_marks(
                            node,
                            TextRange::new(offset(raw), offset(raw)).unwrap()
                        )
                        .unwrap(),
                    expected
                );
                assert_eq!(
                    session.apply_intent(&intent).unwrap(),
                    SessionOutcome::DocumentChanged
                );
                assert_eq!(inserted_marks(&session, node, raw), expected);
                check_undo(&mut session, &document, selection);
            }
        }
    }
}

#[test]
fn explicit_empty_and_custom_marks_override_atoms_for_all_collapsed_input_paths() {
    let (document, node) = fixture(true);
    for expected in [MarkSet::empty(), marks(Mark::Strike)] {
        for intent in [
            EditIntent::InsertText { text: "X".into() },
            EditIntent::PasteText { text: "X".into() },
            EditIntent::CommitComposition {
                range: TextRange::new(offset(0), offset(0)).unwrap(),
                text: "X".into(),
            },
        ] {
            let at = point(node, 0, 3);
            let selection = DocumentSelection::collapsed(at);
            let mut session = DocumentSession::new_with_policy(
                document.clone(),
                selection,
                Box::new(CheckContext {
                    expected: expected.clone(),
                    pending: Some(expected.clone()),
                }),
            )
            .unwrap();
            session.apply_intent(&EditIntent::Delete).unwrap();
            assert_eq!(session.stored_marks(), Some(&expected));
            assert_eq!(session.effective_input_marks_at(at).unwrap(), expected);
            session.apply_intent(&intent).unwrap();
            assert_eq!(inserted_marks(&session, node, 0), expected);
            check_undo(&mut session, &document, selection);
        }
    }
}

#[test]
fn typed_range_inherits_selected_child_but_actual_clipboard_paste_inherits_left_gap() {
    let (document, node) = fixture(false);
    for (start, end, typed, pasted) in [
        (
            point(node, 2, 0),
            point(node, 2, 1),
            marks(Mark::Italic),
            marks(Mark::Bold),
        ),
        (
            point(node, 2, 1),
            point(node, 2, 2),
            MarkSet::empty(),
            marks(Mark::Italic),
        ),
        (
            point(node, 2, 2),
            point(node, 2, 3),
            marked_payload(),
            MarkSet::empty(),
        ),
        (
            point(node, 2, 3),
            point(node, 4, 0),
            marks(Mark::Underline),
            marked_payload(),
        ),
        (
            point(node, 1, 0),
            point(node, 3, 0),
            marks(Mark::Bold),
            marks(Mark::Bold),
        ),
        (
            point(node, 0, 0),
            point(node, 4, 0),
            marks(Mark::Bold),
            marks(Mark::Bold),
        ),
    ] {
        for reverse in [false, true] {
            let selection = if reverse {
                DocumentSelection::new(end, start)
            } else {
                DocumentSelection::new(start, end)
            };
            for (intent, expected) in [
                (EditIntent::InsertText { text: "X".into() }, &typed),
                (EditIntent::PasteText { text: "X".into() }, &pasted),
            ] {
                let mut session = DocumentSession::new(document.clone(), selection).unwrap();
                assert_eq!(
                    session.effective_input_marks_for_range(start, end).unwrap(),
                    typed
                );
                assert_eq!(session.effective_input_marks_at(start).unwrap(), pasted);
                session.apply_intent(&intent).unwrap();
                assert_eq!(
                    &inserted_marks(&session, node, start.text_offset().as_usize()),
                    expected
                );
                check_undo(&mut session, &document, selection);
            }
        }
    }
}

#[test]
fn range_start_hard_break_marks_are_captured_before_the_selected_atom_is_removed() {
    let (document, node) = fixture(true);
    for (start, end, expected) in [
        (0, 3, marks(Mark::Italic)),
        (1, 3, MarkSet::empty()),
        (2, 3, marked_payload()),
    ] {
        let selection = DocumentSelection::new(point(node, 0, end), point(node, 0, start));
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "中".into() })
            .unwrap();
        assert_eq!(inserted_marks(&session, node, 0), expected);
        check_undo(&mut session, &document, selection);
    }
}

#[test]
fn nonempty_composition_uses_right_text_child_and_query_matches_actual_commit() {
    let (document, node) = fixture(false);
    for focus_ordinal in 0..=3 {
        let selection = DocumentSelection::collapsed(point(node, 2, focus_ordinal));
        let mut session = DocumentSession::new(document.clone(), selection).unwrap();
        let range = TextRange::new(offset(2), offset(4)).unwrap();
        assert_eq!(
            session.effective_composition_marks(node, range).unwrap(),
            marks(Mark::Underline)
        );
        session
            .apply_intent(&EditIntent::CommitComposition {
                range,
                text: "中".into(),
            })
            .unwrap();
        assert_eq!(inserted_marks(&session, node, 2), marks(Mark::Underline));
        assert_eq!(
            session
                .document()
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .atoms()
                .len(),
            3
        );
        check_undo(&mut session, &document, selection);
    }
}

#[test]
fn collapsed_mark_commands_use_atom_marks_as_their_base() {
    let (document, node) = fixture(false);
    let at = point(node, 2, 1);
    let mut session =
        DocumentSession::new(document.clone(), DocumentSelection::collapsed(at)).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&MarkSet::empty()));
    session
        .apply_intent(&EditIntent::InsertText { text: "X".into() })
        .unwrap();
    assert_eq!(inserted_marks(&session, node, 2), MarkSet::empty());

    let mut session = DocumentSession::new(document, DocumentSelection::collapsed(at)).unwrap();
    session
        .apply_intent(&EditIntent::SetMark {
            mark: Mark::Underline,
        })
        .unwrap();
    assert_eq!(
        session.stored_marks(),
        Some(&MarkSet::new([Mark::Italic, Mark::Underline]).unwrap())
    );
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Italic,
        })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&marks(Mark::Underline)));
}

#[test]
fn exact_queries_reject_stale_points_even_when_explicit_marks_exist() {
    let (document, node) = fixture(false);
    let mut session = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(point(node, 2, 1)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Strike })
        .unwrap();
    for at in [
        point(node, 2, 4),
        point(node, 5, 0),
        point(document.root(), 0, 0),
    ] {
        assert!(session.effective_input_marks_at(at).is_err());
    }
    assert!(
        session
            .effective_input_marks_for_range(point(node, 2, 2), point(node, 2, 1))
            .is_err()
    );
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn legacy_unmarked_extension_range_keeps_left_run_input_and_composition_inheritance() {
    for (kind, atom_marks, expected) in [
        (
            AtomKind::new("mention").unwrap(),
            MarkSet::empty(),
            marks(Mark::Bold),
        ),
        (
            AtomKind::new("mention").unwrap(),
            marks(Mark::Code),
            marks(Mark::Underline),
        ),
        (
            AtomKind::hard_break(),
            MarkSet::empty(),
            marks(Mark::Underline),
        ),
    ] {
        let mut builder = NodeStoreBuilder::new();
        let payload = if kind.is_hard_break() {
            InlineAtomContent::hard_break()
        } else {
            InlineAtomContent::new("@legacy").unwrap()
        }
        .with_marks(atom_marks);
        let atom = builder
            .insert(
                NodeKind::InlineAtom(kind),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(payload),
            )
            .unwrap();
        let node = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::with_atoms(
                        [
                            TextRun::new("ab", marks(Mark::Bold)).unwrap(),
                            TextRun::new("cd", marks(Mark::Underline)).unwrap(),
                        ],
                        [InlineAtomPlacement::new(atom, offset(2))],
                    )
                    .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([node]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let (start, end) = (point(node, 2, 1), point(node, 4, 0));
        let selection = DocumentSelection::new(end, start);
        // Core's public range fact remains right-child marks for all nodes;
        // Runtime owns the compatibility gate for legacy input semantics.
        assert_eq!(
            document.inherited_inline_range_marks(start, end).unwrap(),
            Some(marks(Mark::Underline))
        );
        for intent in [
            EditIntent::InsertText { text: "X".into() },
            EditIntent::CommitComposition {
                range: TextRange::new(offset(2), offset(4)).unwrap(),
                text: "X".into(),
            },
        ] {
            let mut session = DocumentSession::new(document.clone(), selection).unwrap();
            assert_eq!(
                session.effective_input_marks_for_range(start, end).unwrap(),
                expected
            );
            assert_eq!(
                session.effective_input_marks(node, offset(2)).unwrap(),
                expected
            );
            assert_eq!(
                session
                    .effective_composition_marks(
                        node,
                        TextRange::new(offset(2), offset(4)).unwrap()
                    )
                    .unwrap(),
                expected
            );
            session.apply_intent(&intent).unwrap();
            assert_eq!(inserted_marks(&session, node, 2), expected);
            assert_eq!(session.document().node(atom), document.node(atom));
            check_undo(&mut session, &document, selection);
        }
    }
}
