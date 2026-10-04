//! Explicit mark setting/removal, independent of toggle and host mark policy.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, LinkAttributes, LinkMark, Mark, MarkKind, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, StringAttribute, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, IntentDisposition,
    PolicyError, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

fn marks(values: impl IntoIterator<Item = Mark>) -> MarkSet {
    MarkSet::new(values).unwrap()
}

fn old_link() -> Mark {
    Mark::Link(LinkMark::new("https://example.test", Some("old".into())))
}

fn new_link() -> Mark {
    Mark::Link(LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(StringAttribute::Value("https://example.test".into()))
            .with_target(StringAttribute::Value("_self".into()))
            .with_rel(StringAttribute::Null)
            .with_class(StringAttribute::Value("".into()))
            .with_title(StringAttribute::Missing),
    ))
}

fn fixture(runs: &[(&str, MarkSet)]) -> (XiaomuDocument, NodeId) {
    let inline = InlineContent::new(
        runs.iter()
            .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap()),
    )
    .unwrap();
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
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

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn selection(document: &XiaomuDocument, node: NodeId, a: usize, b: usize) -> DocumentSelection {
    DocumentSelection::new(point(document, node, a), point(document, node, b))
}

fn session(document: &XiaomuDocument, node: NodeId, a: usize, b: usize) -> DocumentSession {
    DocumentSession::new(document.clone(), selection(document, node, a, b)).unwrap()
}

fn mark_at(session: &DocumentSession, node: NodeId, raw: usize) -> MarkSet {
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let mut cursor = 0;
    for run in inline.runs() {
        cursor += run.len_bytes();
        if raw < cursor {
            return run.marks().clone();
        }
    }
    panic!("offset must address text");
}

fn insert(session: &mut DocumentSession, text: &str) {
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText { text: text.into() })
            .unwrap(),
        SessionOutcome::DocumentChanged,
    );
}

struct Listener(Rc<Cell<(usize, usize)>>);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

fn listen(session: &mut DocumentSession) -> Rc<Cell<(usize, usize)>> {
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    counts
}

#[test]
fn set_link_replaces_attributes_and_preserves_reversed_unicode_selection_in_one_undo() {
    let original_marks = marks([Mark::Bold, old_link()]);
    let (document, node) = fixture(&[("a你z", original_marks.clone())]);
    let mut session = session(&document, node, 4, 1);
    let before_selection = session.selection();
    let counts = listen(&mut session);
    assert_eq!(
        session
            .apply_intent(&EditIntent::SetMark { mark: new_link() })
            .unwrap(),
        SessionOutcome::DocumentChanged,
    );
    let after = session.document().clone();
    assert_eq!(mark_at(&session, node, 0), original_marks);
    assert_eq!(mark_at(&session, node, 1), marks([Mark::Bold, new_link()]));
    assert_eq!(mark_at(&session, node, 4), original_marks);
    assert_eq!(session.selection(), before_selection);
    assert_eq!(
        session.document().revision().as_u64(),
        document.revision().as_u64() + 1
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (1, 0));

    assert_eq!(
        session
            .apply_intent(&EditIntent::SetMark { mark: new_link() })
            .unwrap(),
        SessionOutcome::NoChange,
    );
    assert_eq!(session.document().revision(), after.revision());
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before_selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), before_selection);
}

#[test]
fn remove_link_across_different_values_is_one_undo_and_keeps_other_marks() {
    let (document, node) = fixture(&[
        ("a", marks([old_link(), Mark::Bold])),
        ("bc", marks([new_link(), Mark::Code])),
        ("d", marks([Mark::Italic])),
    ]);
    let mut session = session(&document, node, 0, 4);
    let before_selection = session.selection();
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    assert_eq!(mark_at(&session, node, 0), marks([Mark::Bold]));
    assert_eq!(mark_at(&session, node, 1), marks([Mark::Code]));
    assert_eq!(mark_at(&session, node, 3), marks([Mark::Italic]));
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(session.selection(), before_selection);
    let revision = session.document().revision();
    assert_eq!(
        session
            .apply_intent(&EditIntent::RemoveMark {
                kind: MarkKind::Link
            })
            .unwrap(),
        SessionOutcome::NoChange,
    );
    assert_eq!(session.document().revision(), revision);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before_selection);
}

#[test]
fn setting_a_partially_marked_range_fills_the_gap_instead_of_toggling() {
    let (document, node) = fixture(&[
        ("a", marks([new_link(), Mark::Bold])),
        ("b", marks([Mark::Bold])),
        ("c", marks([old_link(), Mark::Italic])),
    ]);
    let mut session = session(&document, node, 0, 3);
    session
        .apply_intent(&EditIntent::SetMark { mark: new_link() })
        .unwrap();
    assert_eq!(mark_at(&session, node, 0), marks([new_link(), Mark::Bold]));
    assert_eq!(mark_at(&session, node, 1), marks([new_link(), Mark::Bold]));
    assert_eq!(
        mark_at(&session, node, 2),
        marks([new_link(), Mark::Italic])
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn idempotent_range_intents_do_not_advance_revision_history_or_listeners() {
    let (document, node) = fixture(&[
        ("a", marks([Mark::Bold, old_link()])),
        ("b", marks([Mark::Bold, Mark::Italic])),
    ]);
    let mut session = session(&document, node, 0, 2);
    let counts = listen(&mut session);
    for intent in [
        EditIntent::SetMark { mark: Mark::Bold },
        EditIntent::RemoveMark {
            kind: MarkKind::Code,
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent).unwrap(),
            SessionOutcome::NoChange
        );
    }
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 0));
}

#[test]
fn collapsed_set_replaces_inherited_link_and_persists_through_continuous_typing() {
    let (document, node) = fixture(&[("ab", marks([Mark::Bold, old_link()]))]);
    let mut session = session(&document, node, 1, 1);
    let before_selection = session.selection();
    let counts = listen(&mut session);
    assert_eq!(
        session
            .apply_intent(&EditIntent::SetMark { mark: new_link() })
            .unwrap(),
        SessionOutcome::NoChange,
    );
    let pending = marks([Mark::Bold, new_link()]);
    assert_eq!(session.stored_marks(), Some(&pending));
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 0));
    insert(&mut session, "中");
    insert(&mut session, "X");
    assert_eq!(mark_at(&session, node, 1), pending);
    assert_eq!(mark_at(&session, node, 4), pending);
    assert_eq!(mark_at(&session, node, 5), marks([Mark::Bold, old_link()]));
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), before_selection);
}

#[test]
fn collapsed_remove_uses_explicit_marks_and_retains_explicit_empty() {
    let (document, node) = fixture(&[("ab", marks([old_link()]))]);
    let mut session = session(&document, node, 1, 1);
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
        .unwrap();
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&marks([Mark::Italic])));
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Italic,
        })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&MarkSet::empty()));
    insert(&mut session, "X");
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&MarkSet::empty()));
    insert(&mut session, "Y");
    assert_eq!(mark_at(&session, node, 1), MarkSet::empty());
    assert_eq!(mark_at(&session, node, 2), MarkSet::empty());
    assert_eq!(mark_at(&session, node, 3), marks([old_link()]));
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn collapsed_inheritance_uses_left_run_at_zero_and_boundary() {
    for offset in [0, 1] {
        let (document, node) = fixture(&[
            ("a", marks([Mark::Bold, old_link()])),
            ("b", marks([Mark::Italic])),
        ]);
        let mut session = session(&document, node, offset, offset);
        session
            .apply_intent(&EditIntent::SetMark { mark: new_link() })
            .unwrap();
        assert_eq!(
            session.stored_marks(),
            Some(&marks([Mark::Bold, new_link()]))
        );
    }
}

#[test]
fn collapsed_idempotent_commands_preserve_inherited_or_explicit_marks_and_open_group() {
    for explicit in [false, true] {
        let (document, node) = fixture(&[("a", marks([Mark::Bold]))]);
        let mut session = session(&document, node, 1, 1);
        if explicit {
            session
                .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
                .unwrap();
        }
        insert(&mut session, "b");
        let pending = session.stored_marks().cloned();
        let revision = session.document().revision();
        let counts = listen(&mut session);
        for intent in [
            EditIntent::SetMark { mark: Mark::Bold },
            EditIntent::RemoveMark {
                kind: MarkKind::Link,
            },
        ] {
            assert_eq!(
                session.apply_intent(&intent).unwrap(),
                SessionOutcome::NoChange
            );
            assert_eq!(session.stored_marks(), pending.as_ref());
        }
        assert_eq!(session.document().revision(), revision);
        assert_eq!(counts.get(), (0, 0));
        insert(&mut session, "c");
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
    }
}

#[test]
fn real_collapsed_mark_change_splits_the_typing_group_without_its_own_undo_entry() {
    let (document, node) = fixture(&[("a", MarkSet::empty())]);
    let mut session = session(&document, node, 1, 1);
    insert(&mut session, "b");
    session
        .apply_intent(&EditIntent::SetMark { mark: old_link() })
        .unwrap();
    insert(&mut session, "c");
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(mark_at(&session, node, 1), MarkSet::empty());
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}

#[test]
fn explicit_marks_do_not_impose_inline_code_exclusion() {
    for collapsed in [false, true] {
        for code_first in [false, true] {
            let (document, node) = fixture(&[("ab", marks([Mark::Italic]))]);
            let mut session = session(&document, node, 1, if collapsed { 1 } else { 2 });
            let pair = if code_first {
                [Mark::Code, new_link()]
            } else {
                [new_link(), Mark::Code]
            };
            for mark in pair {
                session.apply_intent(&EditIntent::SetMark { mark }).unwrap();
            }
            let expected = marks([Mark::Italic, Mark::Code, new_link()]);
            if collapsed {
                assert_eq!(session.stored_marks(), Some(&expected));
                insert(&mut session, "X");
            }
            assert_eq!(mark_at(&session, node, 1), expected);
        }
    }
}

#[test]
fn existing_link_toggle_still_removes_by_kind_even_when_attributes_differ() {
    for collapsed in [false, true] {
        let (document, node) = fixture(&[("ab", marks([Mark::Bold, old_link()]))]);
        let mut session = session(&document, node, 1, if collapsed { 1 } else { 2 });
        session
            .apply_intent(&EditIntent::ToggleMark { mark: new_link() })
            .unwrap();
        if collapsed {
            assert_eq!(session.stored_marks(), Some(&marks([Mark::Bold])));
        } else {
            assert_eq!(mark_at(&session, node, 1), marks([Mark::Bold]));
        }
    }
}

struct RejectExplicit;

impl SessionPolicy for RejectExplicit {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(
            intent,
            EditIntent::SetMark { .. } | EditIntent::RemoveMark { .. }
        ) {
            return Err(PolicyError::new("explicit mark rejected"));
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn explicit_mark_preflight_rejection_preserves_marks_revision_listeners_and_typing_group() {
    for range in [false, true] {
        let (document, node) = fixture(&[("a", marks([old_link()]))]);
        let mut session = DocumentSession::new_with_policy(
            document.clone(),
            selection(&document, node, 1, 1),
            Box::new(RejectExplicit),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        insert(&mut session, "b");
        let before = session.document().clone();
        let before_selection = session.selection();
        let before_marks = session.stored_marks().cloned();
        let counts = listen(&mut session);
        let target = if range {
            selection(&before, node, 0, 2)
        } else {
            before_selection
        };
        for intent in [
            EditIntent::SetMark { mark: new_link() },
            EditIntent::RemoveMark {
                kind: MarkKind::Link,
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
            assert_eq!(counts.get(), (0, 0));
        }
        insert(&mut session, "c");
        assert_eq!(session.history_depths(), (1, 0));
    }
}

struct PreserveLinkPresence(bool);

impl SessionPolicy for PreserveLinkPresence {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        let has_link = document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .any(|run| run.marks().contains(MarkKind::Link));
        if has_link != self.0 {
            return Err(PolicyError::new("candidate link presence rejected"));
        }
        Ok(())
    }
}

#[test]
fn candidate_rejection_of_set_or_remove_restores_atomic_target_and_transient_state() {
    for linked in [false, true] {
        let initial_marks = if linked {
            marks([old_link()])
        } else {
            MarkSet::empty()
        };
        let (document, node) = fixture(&[("a", initial_marks)]);
        let mut session = DocumentSession::new_with_policy(
            document.clone(),
            selection(&document, node, 1, 1),
            Box::new(PreserveLinkPresence(linked)),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
            .unwrap();
        insert(&mut session, "b");
        let before = session.document().clone();
        let before_selection = session.selection();
        let before_marks = session.stored_marks().cloned();
        let counts = listen(&mut session);
        let intent = if linked {
            EditIntent::RemoveMark {
                kind: MarkKind::Link,
            }
        } else {
            EditIntent::SetMark { mark: new_link() }
        };
        assert!(matches!(
            session.apply_intent_with_selection(selection(&before, node, 0, 2), &intent),
            Err(SessionError::Policy(_))
        ));
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), before_selection);
        assert_eq!(session.stored_marks(), before_marks.as_ref());
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(counts.get(), (0, 0));
        insert(&mut session, "c");
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
    }
}

#[test]
fn collapsed_atom_seam_keeps_its_exact_caret_while_updating_typing_marks() {
    let (document, node) = fixture(&[("ab", marks([Mark::Bold, old_link()]))]);
    let offset = point(&document, node, 1).offset();
    let document = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(node, offset, 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@Ada").unwrap(),
        })
        .apply(&document)
        .unwrap();
    let caret = InlinePoint::new(node, offset, 1, CursorAffinity::Before);
    let selection = DocumentSelection::collapsed(caret);
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: new_link() })
        .unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(
        session.stored_marks(),
        Some(&marks([Mark::Bold, new_link()]))
    );
    insert(&mut session, "X");
    assert_eq!(mark_at(&session, node, 1), marks([Mark::Bold, new_link()]));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.selection(), selection);
}
