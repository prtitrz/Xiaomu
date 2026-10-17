//! Genuine structural All uses generic atomic mark planning, never a text surrogate.
#[path = "support/mixed_marks.rs"]
mod support;

use support::*;
use xiaomu_core::document::{
    AtomKind, LinkAttributes, LinkMark, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, StringAttribute, TextStyleAttributes, TextStyleMark,
    XiaomuDocument,
};
use xiaomu_core::selection::NodeGap;
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, IntentDisposition, PolicyError, SessionContext,
    SessionError, SessionOutcome, SessionPolicy,
};

fn container(b: &mut NodeStoreBuilder, kind: NodeKind, children: Vec<NodeId>) -> NodeId {
    b.insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn table(b: &mut NodeStoreBuilder, children: Vec<NodeId>) -> NodeId {
    let cell = container(b, NodeKind::TableCell, children);
    let row = container(b, NodeKind::TableRow, vec![cell]);
    container(b, NodeKind::Table, vec![row])
}

fn nested_fixture() -> (XiaomuDocument, Vec<NodeId>, Vec<NodeId>) {
    let mut b = NodeStoreBuilder::new();
    let mut leaves = Vec::new();
    let mut atoms = Vec::new();
    for text in ["中\n🙂e\u{301}", "", "nested", "tail"] {
        let (p, ids) = block(
            &mut b,
            text,
            MarkSet::empty(),
            &[
                (0, AtomKind::hard_break(), MarkSet::empty()),
                (0, AtomKind::new("mention").unwrap(), MarkSet::empty()),
                (text.len(), AtomKind::hard_break(), MarkSet::empty()),
                (
                    text.len(),
                    AtomKind::new("hardBreak").unwrap(),
                    MarkSet::empty(),
                ),
            ],
        );
        leaves.push(p);
        atoms.extend(ids);
    }
    let inner = table(&mut b, vec![leaves[2]]);
    let outer = table(&mut b, vec![leaves[1], inner]);
    let item = container(&mut b, NodeKind::ListItem, vec![leaves[0], outer]);
    let list = container(&mut b, NodeKind::BulletList, vec![item]);
    let image = b
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap();
    let rule = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let empty = container(&mut b, NodeKind::Quote, vec![]);
    (
        finish(b, vec![image, list, empty, leaves[3], rule]),
        leaves,
        atoms,
    )
}

fn formats() -> Vec<Mark> {
    vec![
        Mark::Bold,
        Mark::Italic,
        Mark::Underline,
        Mark::Strike,
        Mark::Code,
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href("原值🙂".into())
                .with_target(StringAttribute::Null)
                .with_title("".into()),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(StringAttribute::Null)
                .with_font_family("宋体".into())
                .with_font_size("calc(1em + 2px)".into()),
        )),
    ]
}

#[test]
fn all_formats_plain_and_nested_documents_preserve_all_direction_and_one_history_unit() {
    let (nested, leaves, atoms) = nested_fixture();
    let (plain, p, _) = fixture("plain中🙂", MarkSet::empty(), &[]);
    let mut b = NodeStoreBuilder::new();
    let (a, _) = block(&mut b, "first", MarkSet::empty(), &[]);
    let (z, _) = block(&mut b, "last", MarkSet::empty(), &[]);
    let multi = finish(b, vec![a, z]);
    for (doc, leaves, atoms) in [
        (plain, vec![p], vec![]),
        (multi, vec![a, z], vec![]),
        (nested, leaves, atoms),
    ] {
        let all = DocumentSelection::all(&doc);
        for selection in [all, DocumentSelection::new(all.focus(), all.anchor())] {
            for mark in formats() {
                let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
                let counts = listen(&mut session);
                session
                    .apply_intent(&EditIntent::ToggleMark { mark: mark.clone() })
                    .unwrap();
                let after = session.document().clone();
                assert_eq!(session.selection(), selection);
                assert!(session.selection().is_all(&after));
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (1, 0));
                assert_eq!(session.stored_marks(), None);
                for leaf in &leaves {
                    let content = inline(&after, *leaf);
                    assert_eq!(content.atoms(), inline(&doc, *leaf).atoms());
                    assert!(
                        content
                            .runs()
                            .iter()
                            .all(|run| run.marks() == &marks([mark.clone()]))
                    );
                    if !content.runs().is_empty() {
                        assert_eq!(text_marks(&after, *leaf, 0), &marks([mark.clone()]));
                    }
                }
                for atom in &atoms {
                    assert_eq!(atom_marks(&after, *atom), &marks([mark.clone()]));
                }
                let revision = after.revision();
                assert_eq!(
                    session
                        .apply_intent(&EditIntent::SetMark { mark: mark.clone() })
                        .unwrap(),
                    SessionOutcome::NoChange
                );
                assert_eq!(session.document().revision(), revision);
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(counts.get(), (1, 0));
                session.undo().unwrap();
                assert_eq!(session.document().store(), doc.store());
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), (0, 1));
                session.redo().unwrap();
                assert_eq!(session.document().store(), after.store());
                assert_eq!(session.selection(), selection);
                session
                    .apply_intent(&EditIntent::ToggleMark { mark: mark.clone() })
                    .unwrap();
                assert_eq!(session.document().store(), doc.store());
                assert_eq!(session.selection(), selection);
                assert_eq!(session.history_depths(), (2, 0));
                assert_eq!(
                    session
                        .apply_intent(&EditIntent::RemoveMark { kind: mark.kind() })
                        .unwrap(),
                    SessionOutcome::NoChange
                );
            }
        }
    }
}

#[test]
fn empty_root_empty_paragraph_and_atomic_only_all_are_true_noops() {
    let empty_root = finish(NodeStoreBuilder::new(), vec![]);
    let (empty_p, _, _) = fixture("", MarkSet::empty(), &[]);
    let mut b = NodeStoreBuilder::new();
    let image = b
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap();
    let atomic = finish(b, vec![image]);
    for doc in [empty_root, empty_p, atomic] {
        let all = DocumentSelection::all(&doc);
        let mut session = DocumentSession::new(doc.clone(), all).unwrap();
        let counts = listen(&mut session);
        for mark in formats() {
            for intent in [
                EditIntent::ToggleMark { mark: mark.clone() },
                EditIntent::SetMark { mark: mark.clone() },
                EditIntent::RemoveMark { kind: mark.kind() },
            ] {
                assert_eq!(
                    session.apply_intent(&intent).unwrap(),
                    SessionOutcome::NoChange
                );
                assert_eq!(session.document().store(), doc.store());
                assert_eq!(session.document().revision(), doc.revision());
                assert_eq!(session.selection(), all);
                assert_eq!(session.stored_marks(), None);
                assert_eq!(session.history_depths(), (0, 0));
                assert_eq!(counts.get(), (0, 0));
            }
        }
    }
}

#[test]
fn all_toggle_uses_one_global_decision_and_exact_set_replaces_link_attributes() {
    let mut b = NodeStoreBuilder::new();
    let old = Mark::Link(LinkMark::new("old", None));
    let new = Mark::Link(LinkMark::new("new", Some("title".into())));
    let (a, atoms) = block(
        &mut b,
        "bold",
        marks([Mark::Bold, old.clone()]),
        &[(0, AtomKind::hard_break(), marks([old.clone()]))],
    );
    let (z, _) = block(&mut b, "plain", marks([old]), &[]);
    let doc = finish(b, vec![a, z]);
    let all = DocumentSelection::all(&doc);
    let mut session = DocumentSession::new(doc.clone(), all).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    for leaf in [a, z] {
        assert!(text_marks(session.document(), leaf, 0).contains(MarkKind::Bold));
    }
    assert!(atom_marks(session.document(), atoms[0]).contains(MarkKind::Bold));
    session
        .apply_intent(&EditIntent::SetMark { mark: new.clone() })
        .unwrap();
    for leaf in [a, z] {
        assert!(
            text_marks(session.document(), leaf, 0)
                .as_slice()
                .contains(&new)
        );
    }
    assert!(
        atom_marks(session.document(), atoms[0])
            .as_slice()
            .contains(&new)
    );
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    for leaf in [a, z] {
        assert!(!text_marks(session.document(), leaf, 0).contains(MarkKind::Link));
    }
    assert!(!atom_marks(session.document(), atoms[0]).contains(MarkKind::Link));
    for _ in 0..3 {
        session.undo().unwrap();
    }
    assert_eq!(session.document().store(), doc.store());
    assert_eq!(session.selection(), all);
}

struct ProtectedAtom {
    atom: NodeId,
    original: std::rc::Rc<std::cell::Cell<DocumentSelection>>,
}
impl SessionPolicy for ProtectedAtom {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(
            intent,
            EditIntent::ToggleMark { .. } | EditIntent::RemoveMark { .. }
        ) {
            assert!(context.selection().is_all(context.document()));
            assert_eq!(context.original_selection(), self.original.get());
        }
        Ok(IntentDisposition::Continue)
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if atom_marks(document, self.atom) != &marks([Mark::Code]) {
            return Err(PolicyError::new("protected atom marks"));
        }
        Ok(())
    }
}

#[test]
fn rejected_all_candidate_keeps_original_selection_pending_marks_redo_and_typing_group() {
    let (doc, node, atoms) = fixture(
        "a",
        MarkSet::empty(),
        &[(0, AtomKind::new("refChip").unwrap(), marks([Mark::Code]))],
    );
    let caret = DocumentSelection::collapsed(point(&doc, node, 1, 0));
    let expected_original = std::rc::Rc::new(std::cell::Cell::new(caret));
    let mut session = DocumentSession::new_with_policy(
        doc.clone(),
        caret,
        Box::new(ProtectedAtom {
            atom: atoms[0],
            original: expected_original.clone(),
        }),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    let counts = listen(&mut session);
    for intent in [
        EditIntent::ToggleMark { mark: Mark::Bold },
        EditIntent::RemoveMark {
            kind: MarkKind::Code,
        },
    ] {
        assert!(matches!(
            session.apply_intent_with_selection(DocumentSelection::all(&doc), &intent),
            Err(SessionError::Policy(_))
        ));
        assert_eq!(session.document().store(), doc.store());
        assert_eq!(session.document().revision(), doc.revision());
        assert_eq!(session.selection(), caret);
        assert_eq!(session.stored_marks(), Some(&marks([Mark::Bold])));
        assert_eq!(session.history_depths(), (0, 0));
        assert_eq!(counts.get(), (0, 0));
    }
    // A denied candidate cannot erase a redo entry.
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    session.undo().unwrap();
    assert!(
        session
            .apply_intent_with_selection(
                DocumentSelection::all(session.document()),
                &EditIntent::ToggleMark { mark: Mark::Bold }
            )
            .is_err()
    );
    assert_eq!(session.history_depths(), (0, 1));
    session.redo().unwrap();
    // Update the expected source after typing advances the caret.
    let before = session.document().clone();
    let current = session.selection();
    expected_original.set(current);
    assert!(
        session
            .apply_intent_with_selection(
                DocumentSelection::all(&before),
                &EditIntent::ToggleMark { mark: Mark::Bold }
            )
            .is_err()
    );
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (2, 0));
    let current = session.selection();
    expected_original.set(current);
    let before = session.document().clone();
    assert!(
        session
            .apply_intent_with_selection(
                DocumentSelection::all(&before),
                &EditIntent::ToggleMark { mark: Mark::Bold }
            )
            .is_err()
    );
    assert_eq!(session.selection(), current);
    session
        .apply_intent(&EditIntent::InsertText { text: "z".into() })
        .unwrap();
    assert_eq!(
        session.history_depths(),
        (2, 0),
        "failed All formatting leaves typing coalescing open"
    );
}

#[test]
fn partial_structural_gaps_still_fail_closed() {
    let (doc, _, _) = nested_fixture();
    let selection =
        DocumentSelection::new(NodeGap::new(doc.root(), 0), NodeGap::new(doc.root(), 2));
    let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
    for intent in [
        EditIntent::ToggleMark { mark: Mark::Bold },
        EditIntent::SetMark { mark: Mark::Bold },
        EditIntent::RemoveMark {
            kind: MarkKind::Bold,
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent),
            Err(SessionError::SelectionInvalid)
        );
        assert_eq!(session.document().store(), doc.store());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.history_depths(), (0, 0));
    }
}

#[test]
fn code_marks_and_code_blocks_remain_host_policy_decisions() {
    let mut b = NodeStoreBuilder::new();
    let (p, _) = block(&mut b, "plain", marks([Mark::Code]), &[]);
    let code = b
        .insert(
            NodeKind::CodeBlock,
            NodeAttrs::empty(),
            NodeContent::Inline(
                xiaomu_core::document::InlineContent::new([xiaomu_core::document::TextRun::new(
                    "code",
                    MarkSet::empty(),
                )
                .unwrap()])
                .unwrap(),
            ),
        )
        .unwrap();
    let doc = finish(b, vec![p, code]);
    let all = DocumentSelection::all(&doc);
    let mut session = DocumentSession::new(doc.clone(), all).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(
        text_marks(session.document(), p, 0),
        &marks([Mark::Code, Mark::Bold])
    );
    assert_eq!(
        text_marks(session.document(), code, 0),
        &marks([Mark::Bold])
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
}

struct AllSource {
    original: DocumentSelection,
}
impl SessionPolicy for AllSource {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert_eq!(context.original_selection(), self.original);
        assert!(context.selection().is_all(context.document()));
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn targeted_all_success_keeps_original_for_undo_and_exact_all_for_redo() {
    let (doc, node, _) = fixture("中🙂", MarkSet::empty(), &[]);
    let original = DocumentSelection::collapsed(point(&doc, node, 0, 0));
    let all = DocumentSelection::all(&doc);
    let reversed = DocumentSelection::new(all.focus(), all.anchor());
    let mut session =
        DocumentSession::new_with_policy(doc.clone(), original, Box::new(AllSource { original }))
            .unwrap();
    let counts = listen(&mut session);
    session
        .apply_intent_with_selection(reversed, &EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    let after = session.document().clone();
    assert_eq!(session.selection(), reversed);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.selection(), original);
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    assert_eq!(session.selection(), reversed);
    assert_eq!(session.document().store(), after.store());
}

#[test]
fn targeted_all_noop_retains_explicit_target_semantics_and_invalid_gap_rolls_back() {
    let (doc, node, _) = fixture("marked", marks([Mark::Bold]), &[]);
    let caret = DocumentSelection::collapsed(point(&doc, node, 0, 0));
    let mut session = DocumentSession::new(doc, caret).unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    session.undo().unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
        .unwrap();
    let before = session.document().clone();
    let pending = session.stored_marks().cloned();
    assert!(
        pending
            .as_ref()
            .is_some_and(|marks| marks.contains(MarkKind::Italic))
    );
    let counts = listen(&mut session);
    let invalid = DocumentSelection::new(
        NodeGap::new(before.root(), 0),
        NodeGap::new(before.root(), 99),
    );
    assert!(
        session
            .apply_intent_with_selection(invalid, &EditIntent::ToggleMark { mark: Mark::Bold })
            .is_err()
    );
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), caret);
    assert_eq!(session.stored_marks(), pending.as_ref());
    assert_eq!(session.history_depths(), (0, 1));
    assert_eq!(counts.get(), (0, 0));
    // An explicitly requested target is installed even when the default edit
    // is idempotent. This is the established selection-only success contract.
    let all = DocumentSelection::all(&before);
    assert_eq!(
        session
            .apply_intent_with_selection(all, &EditIntent::SetMark { mark: Mark::Bold })
            .unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(session.selection(), all);
    assert_eq!(session.stored_marks(), None);
    assert_eq!(counts.get(), (0, 1));
    assert_eq!(
        session
            .apply_intent(&EditIntent::RemoveMark {
                kind: MarkKind::Strike
            })
            .unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), all);
    assert_eq!(session.history_depths(), (0, 1));
    assert_eq!(counts.get(), (0, 1));
    session.redo().unwrap();
}
