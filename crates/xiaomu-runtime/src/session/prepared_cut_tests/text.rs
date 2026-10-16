//! Generic opt-in text Cut: no host schema or business fixture.
use super::*;
use crate::session::DocumentPosition;
use xiaomu_core::document::{AtomKind, InlineAtomContent, InlineAtomPlacement};
use xiaomu_core::selection::CursorAffinity;

struct TextPolicy {
    enabled: bool,
    protected: Option<NodeId>,
    refuse_source: bool,
}
impl SessionPolicy for TextPolicy {
    fn prepare_text_cut(&self, _: SessionContext<'_>) -> Result<bool, PolicyError> {
        if self.refuse_source {
            return Err(PolicyError::new("source refused"));
        }
        Ok(self.enabled)
    }
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(purpose, ClipboardExportPurpose::Cut);
        Ok(Some(
            ClipboardExportSpec::new().with_text_projection(
                ClipboardTextProjection::text_between_lf_with_atoms([
                    AtomKind::new("token").unwrap()
                ])
                .unwrap(),
            ),
        ))
    }
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert!(!matches!(intent, EditIntent::Delete));
        Ok(IntentDisposition::Continue)
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self.protected.is_some_and(|id| document.node(id).is_none()) {
            return Err(PolicyError::new("protected token must survive"));
        }
        Ok(())
    }
}

fn mixed(builder: &mut NodeStoreBuilder, text: &str) -> (NodeId, Vec<NodeId>) {
    mixed_with_marks(builder, text, MarkSet::new([Mark::Bold]).unwrap())
}

fn mixed_with_marks(
    builder: &mut NodeStoreBuilder,
    text: &str,
    marks: MarkSet,
) -> (NodeId, Vec<NodeId>) {
    let runs = vec![TextRun::new(text, marks).unwrap()];
    let mut atoms = Vec::new();
    for index in 0..3 {
        atoms.push(
            builder
                .insert(
                    NodeKind::InlineAtom(AtomKind::new("token").unwrap()),
                    attrs("payload", AttrValue::String(format!("opaque-{index}"))),
                    NodeContent::InlineAtom(
                        InlineAtomContent::new("token")
                            .unwrap()
                            .with_marks(MarkSet::new([Mark::Italic]).unwrap()),
                    ),
                )
                .unwrap(),
        );
    }
    let offset = InlineContent::new(runs.clone())
        .unwrap()
        .offset_at(1)
        .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    runs,
                    atoms.iter().map(|id| InlineAtomPlacement::new(*id, offset)),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    (node, atoms)
}
fn finish(mut builder: NodeStoreBuilder, children: Vec<NodeId>) -> XiaomuDocument {
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}
fn point(document: &XiaomuDocument, id: NodeId, byte: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(
        id,
        document
            .node(id)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(byte)
            .unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
}
fn selection(
    document: &XiaomuDocument,
    head: (NodeId, usize, usize),
    tail: (NodeId, usize, usize),
    reverse: bool,
) -> DocumentSelection {
    let h = point(document, head.0, head.1, head.2);
    let t = point(document, tail.0, tail.1, tail.2);
    if reverse {
        DocumentSelection::new(DocumentPosition::Inline(t), DocumentPosition::Inline(h))
    } else {
        DocumentSelection::new(DocumentPosition::Inline(h), DocumentPosition::Inline(t))
    }
}
fn session(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    protected: Option<NodeId>,
) -> DocumentSession {
    DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(TextPolicy {
            enabled: true,
            protected,
            refuse_source: false,
        }),
    )
    .unwrap()
}
fn sentinels(session: &mut DocumentSession) {
    session.stored_marks = Some(MarkSet::new([Mark::Italic]).unwrap());
    session.history.restore_typing_group(true);
    let selection = session.selection();
    session.input_rule_undo = Some(
        session
            .prepare_input_rule_undo(
                InputRuleUndoSpec::new(transaction(), selection).unwrap(),
                session.document(),
                selection,
            )
            .unwrap(),
    );
}
fn text(document: &XiaomuDocument, id: NodeId) -> String {
    document
        .node(id)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

#[test]
fn same_block_unicode_and_ordinal_ranges_publish_once_and_restore_exactly() {
    for reverse in [false, true] {
        for ordinal_only in [false, true] {
            let mut builder = NodeStoreBuilder::new();
            let (id, atoms) = mixed(&mut builder, "a中z");
            let document = finish(builder, vec![id]);
            let range = selection(
                &document,
                (id, 1, 1),
                if ordinal_only { (id, 1, 2) } else { (id, 4, 0) },
                reverse,
            );
            let mut s = session(&document, range, Some(atoms[0]));
            sentinels(&mut s);
            let events = listen(&mut s);
            let mut writer = Writer::seeded();
            assert_eq!(
                publish_through_writer(&mut s, &mut writer),
                Ok(Some(SessionOutcome::DocumentChanged))
            );
            assert_eq!(writer.calls, 1);
            assert_eq!(writer.text, if ordinal_only { "\n" } else { "\n\n中" });
            assert_eq!(
                text(s.document(), id),
                if ordinal_only { "a中z" } else { "az" }
            );
            assert_eq!(s.document().node(atoms[0]), document.node(atoms[0]));
            assert!(s.document().node(atoms[1]).is_none());
            assert_eq!(s.document().node(atoms[2]).is_some(), ordinal_only);
            assert_eq!(events.borrow().len(), 1);
            assert_eq!(s.history_depths(), (1, 0));
            assert_eq!(history_image(&mut s).undo[0].group, HistoryGroup::Isolated);
            assert!(s.stored_marks().is_none());
            assert!(!s.input_rule_undo_available());
            let after = s.document().clone();
            let after_selection = s.selection();
            s.undo().unwrap();
            assert_eq!(s.document().store(), document.store());
            assert_eq!(s.selection(), range);
            s.redo().unwrap();
            assert_eq!(s.document().store(), after.store());
            assert_eq!(s.selection(), after_selection);
            insert(&mut s, "x");
            assert_eq!(s.history_depths(), (2, 0));
        }
    }
}

#[test]
fn multi_block_cut_preserves_suffix_atoms_identity_marks_and_order() {
    for reverse in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let (head, head_atoms) = mixed(&mut builder, "a中z");
        let middle = paragraph(&mut builder, "middle");
        let (tail, tail_atoms) =
            mixed_with_marks(&mut builder, "qéw", MarkSet::new([Mark::Strike]).unwrap());
        let quote = builder
            .insert(
                NodeKind::Quote,
                attrs("container", AttrValue::String("preserve-on-undo".into())),
                NodeContent::children([middle, tail]),
            )
            .unwrap();
        let document = finish(builder, vec![head, quote]);
        let range = selection(&document, (head, 1, 1), (tail, 1, 1), reverse);
        let mut s = session(&document, range, Some(tail_atoms[2]));
        let mut writer = Writer::seeded();
        publish_through_writer(&mut s, &mut writer)
            .unwrap()
            .unwrap();
        assert_eq!(writer.calls, 1);
        assert_eq!(text(s.document(), head), "aéw");
        assert_eq!(writer.text, "\n\n中z\nmiddle\nq\n");
        let inline = s
            .document()
            .node(head)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert_eq!(
            inline.atoms().iter().map(|p| p.atom()).collect::<Vec<_>>(),
            [head_atoms[0], tail_atoms[1], tail_atoms[2]]
        );
        for atom in [head_atoms[0], tail_atoms[1], tail_atoms[2]] {
            assert_eq!(s.document().node(atom), document.node(atom));
        }
        assert_eq!(
            inline.runs()[0].marks(),
            &MarkSet::new([Mark::Bold]).unwrap()
        );
        assert_eq!(
            inline.runs().last().unwrap().marks(),
            &MarkSet::new([Mark::Strike]).unwrap()
        );
        assert!(s.document().node(quote).is_none());
        let after = s.document().clone();
        s.undo().unwrap();
        assert_eq!(s.document().store(), document.store());
        assert_eq!(s.selection(), range);
        s.redo().unwrap();
        assert_eq!(s.document().store(), after.store());
    }
}

#[test]
fn protected_candidate_rejection_and_drop_preserve_all_state_and_allocator() {
    for reject in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let (id, atoms) = mixed(&mut builder, "a中z");
        let document = finish(builder, vec![id]);
        let range = selection(&document, (id, 1, 1), (id, 1, 2), true);
        let mut s = session(&document, range, reject.then_some(atoms[1]));
        let mut control = session(&document, range, reject.then_some(atoms[1]));
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let snapshot = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        if reject {
            assert!(publish_through_writer(&mut s, &mut writer).is_err());
        } else {
            let prepared = s.prepare_cut().unwrap().unwrap();
            assert_eq!(prepared.clipboard_slice().plain_text(), "\n");
            drop(prepared);
        }
        assert_eq!(writer, Writer::seeded());
        snapshot.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn ordinary_lf_cut_guard_and_default_policy_remain_closed() {
    let mut builder = NodeStoreBuilder::new();
    let (id, _) = mixed(&mut builder, "a中z");
    let document = finish(builder, vec![id]);
    let range = selection(&document, (id, 0, 0), (id, 4, 0), false);
    let s = session(&document, range, None);
    assert!(s.clipboard_slice_for(ClipboardExportPurpose::Cut).is_err());
    let mut disabled = DocumentSession::new_with_policy(
        document.clone(),
        range,
        Box::new(TextPolicy {
            enabled: false,
            protected: None,
            refuse_source: false,
        }),
    )
    .unwrap();
    assert!(disabled.prepare_cut().unwrap().is_none());
    assert!(
        disabled
            .clipboard_slice_for(ClipboardExportPurpose::Cut)
            .is_err()
    );
    let mut legacy = DocumentSession::new(document, range).unwrap();
    assert!(legacy.prepare_cut().unwrap().is_none());
}

mod geometry;

mod all;
mod all_attrs;
mod budget;
