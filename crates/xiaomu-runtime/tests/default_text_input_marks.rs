//! Public-API contract for construction-time default text-input mark consumption.
//! Uses the existing deterministic mixed-inline fixture and listener helpers.

#[path = "support/mixed_marks.rs"]
mod support;

#[path = "default_text_input_marks/boundaries.rs"]
mod boundaries;

use std::{cell::Cell, rc::Rc};

use support::*;
use xiaomu_core::document::{AtomKind, Mark, MarkKind, MarkSet, NodeId, XiaomuDocument};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DefaultTextInputMarks, DocumentSelection, DocumentSession, EditIntent, EditPlan,
    IntentDisposition, PolicyError, PrimaryEdit, SelectionUpdate, SessionContext, SessionError,
    SessionOutcome, SessionPolicy,
};

#[derive(Clone, Copy, Debug)]
enum Mode {
    NoPolicy,
    DefaultPolicy,
    Preserve,
    Consume,
}

const MODES: [Mode; 4] = [
    Mode::NoPolicy,
    Mode::DefaultPolicy,
    Mode::Preserve,
    Mode::Consume,
];

struct DefaultPolicy;
impl SessionPolicy for DefaultPolicy {}

struct Options(DefaultTextInputMarks);
impl SessionPolicy for Options {
    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        self.0
    }
}

fn session(document: &XiaomuDocument, selection: DocumentSelection, mode: Mode) -> DocumentSession {
    match mode {
        Mode::NoPolicy => DocumentSession::new(document.clone(), selection),
        Mode::DefaultPolicy => {
            DocumentSession::new_with_policy(document.clone(), selection, Box::new(DefaultPolicy))
        }
        Mode::Preserve | Mode::Consume => DocumentSession::new_with_policy(
            document.clone(),
            selection,
            Box::new(Options(if matches!(mode, Mode::Consume) {
                DefaultTextInputMarks::ConsumePending
            } else {
                DefaultTextInputMarks::PreservePending
            })),
        ),
    }
    .unwrap()
}

fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

fn range(start: usize, end: usize) -> TextRange {
    TextRange::new(offset(start), offset(end)).unwrap()
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    inline(document, node)
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn insert(session: &mut DocumentSession, value: &str) {
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText { text: value.into() })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
}

// Install exact pending Bold or Some(empty) with public mark commands, even
// when the surrounding text/atom already contributes a different mark set.
fn pending(session: &mut DocumentSession, empty: bool) -> MarkSet {
    for kind in [
        MarkKind::Bold,
        MarkKind::Italic,
        MarkKind::Code,
        MarkKind::Underline,
        MarkKind::Strike,
        MarkKind::Link,
        MarkKind::TextStyle,
    ] {
        session
            .apply_intent(&EditIntent::RemoveMark { kind })
            .unwrap();
    }
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    if empty {
        session
            .apply_intent(&EditIntent::RemoveMark {
                kind: MarkKind::Bold,
            })
            .unwrap();
    }
    let expected = if empty {
        MarkSet::empty()
    } else {
        marks([Mark::Bold])
    };
    assert_eq!(session.stored_marks(), Some(&expected));
    expected
}

fn assert_pending_after(session: &DocumentSession, mode: Mode, expected: &MarkSet) {
    assert_eq!(
        session.stored_marks(),
        if matches!(mode, Mode::Consume) {
            None
        } else {
            Some(expected)
        },
        "{mode:?}"
    );
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    notifications: (usize, usize),
}

impl Snapshot {
    fn capture(session: &DocumentSession, counts: &Counts) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            notifications: counts.get(),
        }
    }

    fn assert_unchanged(&self, session: &DocumentSession, counts: &Counts) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(counts.get(), self.notifications);
    }
}

#[test]
fn plain_and_empty_nodes_consume_only_when_opted_in_and_unicode_typing_stays_one_undo() {
    assert_eq!(
        DefaultTextInputMarks::default(),
        DefaultTextInputMarks::PreservePending
    );
    for mode in MODES {
        for seed in ["", "ab"] {
            for empty in [false, true] {
                // Explicit empty is exercised between inherited Bold runs.
                let base = if empty {
                    marks([Mark::Bold])
                } else {
                    MarkSet::empty()
                };
                let (document, node, _) = fixture(seed, base, &[]);
                let at = usize::from(!seed.is_empty());
                let selection = DocumentSelection::collapsed(point(&document, node, at, 0));
                let mut session = session(&document, selection, mode);
                assert_eq!(
                    session.default_text_input_marks(),
                    if matches!(mode, Mode::Consume) {
                        DefaultTextInputMarks::ConsumePending
                    } else {
                        DefaultTextInputMarks::PreservePending
                    }
                );
                let counts = listen(&mut session);
                let expected = pending(&mut session, empty);
                assert_eq!(counts.get(), (0, 0));
                assert_eq!(session.document().revision(), document.revision());
                assert_eq!(session.history_depths(), (0, 0));
                let mut inserted = String::new();
                for (index, value) in ["中", "🙂", "e", "\u{301}"].into_iter().enumerate() {
                    let raw = at + inserted.len();
                    insert(&mut session, value);
                    inserted.push_str(value);
                    assert_eq!(
                        text(session.document(), node),
                        format!("{}{inserted}{}", &seed[..at], &seed[at..])
                    );
                    assert_eq!(text_marks(session.document(), node, raw), &expected);
                    assert_pending_after(&session, mode, &expected);
                    let caret = point(session.document(), node, at + inserted.len(), 0);
                    assert_eq!(session.selection(), DocumentSelection::collapsed(caret));
                    assert_eq!(session.effective_input_marks_at(caret).unwrap(), expected);
                    assert_eq!(session.history_depths(), (1, 0));
                    assert_eq!(counts.get(), (index + 1, 0));
                }
                let after = session.document().clone();
                let after_selection = session.selection();
                assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
                assert_eq!(session.document().store(), document.store());
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), (0, 1));
                assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
                assert_eq!(session.document().store(), after.store());
                assert_eq!(session.selection(), after_selection);
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (6, 0));
            }
        }
    }
}

#[test]
fn absent_pending_marks_still_inherit_and_do_not_create_pending_state() {
    for mode in MODES {
        for seed in ["", "a"] {
            let (document, node, _) = fixture(seed, marks([Mark::Italic]), &[]);
            let selection = DocumentSelection::collapsed(point(&document, node, seed.len(), 0));
            let mut session = session(&document, selection, mode);
            insert(&mut session, "中");
            assert_eq!(session.stored_marks(), None);
            assert_eq!(
                text_marks(session.document(), node, seed.len()),
                &if seed.is_empty() {
                    MarkSet::empty()
                } else {
                    marks([Mark::Italic])
                }
            );
        }
    }
}

#[test]
fn composition_uses_pending_marks_for_insertion_and_replacement_then_isolates_history() {
    for mode in MODES {
        for empty in [false, true] {
            for replace in [false, true] {
                let (document, node, _) = fixture("a中z", marks([Mark::Bold]), &[]);
                // The byte-addressed IME range is deliberately away from the
                // live caret, proving it is the explicit range being replaced.
                let selection = DocumentSelection::collapsed(point(&document, node, 5, 0));
                let mut session = session(&document, selection, mode);
                let counts = listen(&mut session);
                let expected = pending(&mut session, empty);
                let replacement = range(1, if replace { 4 } else { 1 });
                assert_eq!(
                    session
                        .effective_composition_marks(node, replacement)
                        .unwrap(),
                    expected
                );
                assert_eq!(
                    session
                        .apply_intent(&EditIntent::CommitComposition {
                            range: replacement,
                            text: "拼🙂".into(),
                        })
                        .unwrap(),
                    SessionOutcome::DocumentChanged
                );
                assert_eq!(
                    text(session.document(), node),
                    if replace { "a拼🙂z" } else { "a拼🙂中z" }
                );
                assert_eq!(text_marks(session.document(), node, 1), &expected);
                assert_pending_after(&session, mode, &expected);
                assert_eq!(
                    session.selection(),
                    DocumentSelection::collapsed(point(session.document(), node, 8, 0))
                );
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (1, 0));
                let committed = session.document().clone();
                let committed_selection = session.selection();
                insert(&mut session, "后");
                assert_eq!(text_marks(session.document(), node, 8), &expected);
                assert_eq!(session.history_depths(), (2, 0));
                session.undo().unwrap();
                assert_eq!(session.document().store(), committed.store());
                assert_eq!(session.selection(), committed_selection);
                session.undo().unwrap();
                assert_eq!(session.document().store(), document.store());
                assert_eq!(session.selection(), selection);
                session.redo().unwrap();
                assert_eq!(session.document().store(), committed.store());
                assert_eq!(session.selection(), committed_selection);
            }
        }
    }
}

#[test]
fn empty_insert_is_a_complete_noop_and_does_not_break_contiguous_typing() {
    for mode in MODES {
        for empty in [false, true] {
            let (document, node, _) = fixture("", MarkSet::empty(), &[]);
            let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
            let mut session = session(&document, selection, mode);
            let counts = listen(&mut session);
            pending(&mut session, empty);
            let before = Snapshot::capture(&session, &counts);
            assert_eq!(
                session
                    .apply_intent(&EditIntent::InsertText {
                        text: String::new()
                    })
                    .unwrap(),
                SessionOutcome::NoChange
            );
            before.assert_unchanged(&session, &counts);
            insert(&mut session, "中");
            let before = Snapshot::capture(&session, &counts);
            assert_eq!(
                session
                    .apply_intent(&EditIntent::InsertText {
                        text: String::new()
                    })
                    .unwrap(),
                SessionOutcome::NoChange
            );
            before.assert_unchanged(&session, &counts);
            insert(&mut session, "🙂");
            assert_eq!(session.history_depths(), (1, 0));
            session.undo().unwrap();
            assert_eq!(session.document().store(), document.store());
        }
    }
}

#[test]
fn empty_plain_composition_keeps_pending_state_and_its_legacy_history_boundary() {
    for mode in MODES {
        let (document, node, _) = fixture("", MarkSet::empty(), &[]);
        let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
        let mut session = session(&document, selection, mode);
        let counts = listen(&mut session);
        pending(&mut session, true);
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(
            session
                .apply_intent(&EditIntent::CommitComposition {
                    range: range(0, 0),
                    text: String::new()
                })
                .unwrap(),
            SessionOutcome::NoChange
        );
        before.assert_unchanged(&session, &counts);
        insert(&mut session, "中");
        let before = Snapshot::capture(&session, &counts);
        assert_eq!(
            session
                .apply_intent(&EditIntent::CommitComposition {
                    range: range(3, 3),
                    text: String::new()
                })
                .unwrap(),
            SessionOutcome::NoChange
        );
        before.assert_unchanged(&session, &counts);
        insert(&mut session, "🙂");
        // Existing CommitComposition isolates even a plain empty commit.
        assert_eq!(session.history_depths(), (2, 0));
    }
}

struct CountOptions {
    mode: DefaultTextInputMarks,
    calls: Rc<Cell<usize>>,
}
impl SessionPolicy for CountOptions {
    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        // Test instrumentation only: validation rules remain immutable.
        self.calls.set(self.calls.get() + 1);
        self.mode
    }
}

#[test]
fn option_is_captured_once_and_sessions_do_not_share_pending_marks_or_history() {
    let (document, node, _) = fixture("", MarkSet::empty(), &[]);
    let selection = DocumentSelection::collapsed(point(&document, node, 0, 0));
    let calls = Rc::new(Cell::new(0));
    let mut consume = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(CountOptions {
            mode: DefaultTextInputMarks::ConsumePending,
            calls: calls.clone(),
        }),
    )
    .unwrap();
    let mut preserve = DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(CountOptions {
            mode: DefaultTextInputMarks::PreservePending,
            calls: calls.clone(),
        }),
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    let consume_counts = listen(&mut consume);
    let preserve_counts = listen(&mut preserve);
    pending(&mut consume, false);
    pending(&mut preserve, true);
    let other = Snapshot::capture(&preserve, &preserve_counts);
    for value in ["中", "🙂"] {
        assert_eq!(
            consume.default_text_input_marks(),
            DefaultTextInputMarks::ConsumePending
        );
        insert(&mut consume, value);
        assert_eq!(consume.stored_marks(), None);
    }
    consume.undo().unwrap();
    consume.redo().unwrap();
    other.assert_unchanged(&preserve, &preserve_counts);
    let other = Snapshot::capture(&consume, &consume_counts);
    insert(&mut preserve, "a");
    assert_eq!(preserve.stored_marks(), Some(&MarkSet::empty()));
    assert_eq!(
        preserve.default_text_input_marks(),
        DefaultTextInputMarks::PreservePending
    );
    other.assert_unchanged(&consume, &consume_counts);
    assert_eq!(calls.get(), 2);
}

#[test]
fn every_atom_and_hard_break_seam_uses_pending_marks_then_inherits_inserted_unicode() {
    for mode in MODES {
        for only_atoms in [false, true] {
            let raw = if only_atoms { 0 } else { 3 };
            let (document, node, atoms) = fixture(
                if only_atoms { "" } else { "中尾" },
                marks([Mark::Italic]),
                &[
                    (raw, AtomKind::hard_break(), marks([Mark::Code])),
                    (
                        raw,
                        AtomKind::new("mention").unwrap(),
                        marks([Mark::Underline]),
                    ),
                ],
            );
            for ordinal in 0..=atoms.len() {
                for empty in [false, true] {
                    for composition in [false, true] {
                        let selection =
                            DocumentSelection::collapsed(point(&document, node, raw, ordinal));
                        let mut session = session(&document, selection, mode);
                        let counts = listen(&mut session);
                        let expected = pending(&mut session, empty);
                        let intent = if composition {
                            EditIntent::CommitComposition {
                                range: range(raw, raw),
                                text: "🙂".into(),
                            }
                        } else {
                            EditIntent::InsertText {
                                text: "🙂".into()
                            }
                        };
                        assert_eq!(
                            session.apply_intent(&intent).unwrap(),
                            SessionOutcome::DocumentChanged
                        );
                        assert_eq!(text_marks(session.document(), node, raw), &expected);
                        assert_pending_after(&session, mode, &expected);
                        let placements = inline(session.document(), node).atoms();
                        assert_eq!(placements.len(), atoms.len());
                        for (index, atom) in atoms.iter().enumerate() {
                            assert_eq!(placements[index].atom(), *atom);
                            assert_eq!(
                                placements[index].text_offset().as_usize(),
                                if index < ordinal {
                                    raw
                                } else {
                                    raw + "🙂".len()
                                }
                            );
                            assert_eq!(session.document().node(*atom), document.node(*atom));
                            assert_eq!(
                                atom_marks(session.document(), *atom),
                                atom_marks(&document, *atom)
                            );
                        }
                        let after = session.document().clone();
                        let after_selection = session.selection();
                        assert_eq!(counts.get(), (1, 0));
                        insert(&mut session, "文");
                        assert_eq!(
                            text_marks(session.document(), node, raw + "🙂".len()),
                            &expected
                        );
                        assert_eq!(
                            session.history_depths(),
                            (if composition { 2 } else { 1 }, 0)
                        );
                        session.undo().unwrap();
                        if composition {
                            assert_eq!(session.document().store(), after.store());
                            assert_eq!(session.selection(), after_selection);
                            session.undo().unwrap();
                        }
                        assert_eq!(session.document().store(), document.store());
                        assert_eq!(session.selection(), selection);
                    }
                }
            }
        }
    }
}
