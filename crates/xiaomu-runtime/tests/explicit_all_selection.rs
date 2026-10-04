//! Explicit structural Select All must never be inferred from text coverage.
use xiaomu_core::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{InlinePoint, NodeGap};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition, PolicyError,
    SelectionUpdate, SessionContext, SessionError, SessionOutcome, SessionPolicy,
};

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let leading = b
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap();
    let hb = b
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Bold]).unwrap()),
            ),
        )
        .unwrap();
    let first = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([], [InlineAtomPlacement::new(hb, TextOffset::ZERO)])
                    .unwrap(),
            ),
        )
        .unwrap();
    let item = b
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let list = b
        .insert(
            NodeKind::OrderedList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let empty = b
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let tail = b
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("尾🙂", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let trailing = b
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([leading, list, empty, tail, trailing]),
        )
        .unwrap();
    (XiaomuDocument::new(root, b.finish()).unwrap(), first, tail)
}

#[test]
fn explicit_all_is_direction_independent_and_not_a_full_text_or_partial_gap_range() {
    let (doc, first, tail) = fixture();
    let all = DocumentSelection::all(&doc);
    all.validate(&doc).unwrap();
    assert!(all.is_all(&doc));
    assert!(DocumentSelection::new(all.focus(), all.anchor()).is_all(&doc));
    let text = DocumentSelection::new(
        InlinePoint::at_start_of(first),
        InlinePoint::new(
            tail,
            doc.node(tail)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .offset_at("尾🙂".len())
                .unwrap(),
            0,
            xiaomu_core::selection::CursorAffinity::Before,
        ),
    );
    assert!(!text.is_all(&doc));
    assert!(
        !DocumentSelection::new(NodeGap::new(doc.root(), 0), NodeGap::new(doc.root(), 4))
            .is_all(&doc)
    );
    let open = DocumentSession::new(doc.clone(), text)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert!(!open.is_closed());
    assert_eq!(open.roots().len(), 3);
    let mut session = DocumentSession::new(doc.clone(), text).unwrap();
    assert_eq!(
        session.set_document_selection(all).unwrap(),
        SessionOutcome::SelectionChanged
    );
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(session.document().revision(), doc.revision());
    assert!(
        session
            .set_document_selection(DocumentSelection::collapsed(NodeGap::new(doc.root(), 99)))
            .is_err()
    );
    assert_eq!(session.selection(), all);
}

#[test]
fn copy_all_retains_boundary_atoms_empty_containers_and_hard_breaks_with_closed_v11() {
    let (doc, _, _) = fixture();
    let all = DocumentSelection::all(&doc);
    for selection in [all, DocumentSelection::new(all.focus(), all.anchor())] {
        let session = DocumentSession::new(doc.clone(), selection).unwrap();
        let slice = session.clipboard_slice().unwrap().unwrap();
        assert!(slice.is_closed());
        assert_eq!(slice.roots().len(), 5);
        assert_eq!(slice.roots()[0].kind(), &NodeKind::Image);
        assert_eq!(slice.roots()[1].kind(), &NodeKind::OrderedList);
        assert!(slice.roots()[2].content().as_children().unwrap().is_empty());
        assert_eq!(slice.roots()[4].kind(), &NodeKind::HorizontalRule);
        let atom = &slice.blocks()[0].inline().atoms()[0];
        assert!(atom.kind().is_hard_break());
        assert_eq!(atom.content().marks(), &MarkSet::new([Mark::Bold]).unwrap());
        let metadata = encode_metadata(&slice).unwrap();
        assert!(metadata.contains("\"version\":11"));
        assert!(metadata.contains("\"closed\":true"));
        assert_eq!(
            decode_metadata(slice.plain_text(), &metadata),
            Some(slice.clone())
        );
        for invalid in [
            metadata.replace("\"version\":11", "\"version\":10"),
            metadata.replace(",\"closed\":true", ""),
            metadata.replace("\"closed\":true", "\"closed\":false"),
            metadata.replace("\"closed\":true", "\"closed\":null"),
            metadata
                .replace("\"version\":11", "\"version\":10")
                .replace("\"closed\":true", "\"closed\":null"),
        ] {
            assert!(decode_metadata(slice.plain_text(), &invalid).is_none());
        }
        assert_eq!(session.document().store(), doc.store());
        assert_eq!(session.history_depths(), (0, 0));
    }
}

#[test]
fn empty_root_all_copies_nothing_and_partial_gaps_do_not_gain_copy_support() {
    let mut b = NodeStoreBuilder::new();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let all = DocumentSelection::all(&doc);
    assert!(all.is_all(&doc));
    assert!(all.is_collapsed());
    assert_eq!(
        DocumentSession::new(doc, all)
            .unwrap()
            .clipboard_slice()
            .unwrap(),
        None
    );
    let (doc, _, _) = fixture();
    let partial = DocumentSelection::new(NodeGap::new(doc.root(), 0), NodeGap::new(doc.root(), 2));
    assert!(
        DocumentSession::new(doc, partial)
            .unwrap()
            .clipboard_slice()
            .is_err()
    );
}

#[test]
fn all_copy_rejects_unknown_leaf_instead_of_dropping_it() {
    let mut b = NodeStoreBuilder::new();
    let unknown = b
        .insert(
            NodeKind::custom("unknown").unwrap(),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("opaque").unwrap()),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([unknown]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let all = DocumentSelection::all(&doc);
    let session = DocumentSession::new(doc, all).unwrap();
    assert!(session.clipboard_slice().is_err());
    assert_eq!(session.selection(), all);
    assert_eq!(session.history_depths(), (0, 0));
}

struct NoChangeOrReject(bool);
impl SessionPolicy for NoChangeOrReject {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert!(context.selection().is_all(context.document()));
        if self.0 {
            Err(PolicyError::new("rejected"))
        } else {
            Ok(IntentDisposition::NoChange)
        }
    }
}

#[test]
fn root_range_edits_reach_policy_and_rejection_or_nochange_preserves_state() {
    let (doc, _, _) = fixture();
    let all = DocumentSelection::all(&doc);
    let slice = DocumentSession::new(doc.clone(), all)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    let intents = [
        EditIntent::Delete,
        EditIntent::Backspace,
        EditIntent::InsertText { text: "new".into() },
        EditIntent::PasteSlice {
            slice: slice.clone(),
        },
    ];
    for reject in [false, true] {
        let mut session =
            DocumentSession::new_with_policy(doc.clone(), all, Box::new(NoChangeOrReject(reject)))
                .unwrap();
        for intent in &intents {
            let result = session.apply_intent(intent);
            if reject {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap(), SessionOutcome::NoChange);
            }
            assert_eq!(session.document().store(), doc.store());
            assert_eq!(session.document().revision(), doc.revision());
            assert_eq!(session.selection(), all);
            assert_eq!(session.history_depths(), (0, 0));
        }
    }
    let mut generic = DocumentSession::new(doc.clone(), all).unwrap();
    for intent in &intents {
        assert!(generic.apply_intent(intent).is_err());
        assert_eq!(generic.document().store(), doc.store());
        assert_eq!(generic.selection(), all);
        assert_eq!(generic.history_depths(), (0, 0));
    }
    assert_eq!(
        generic.apply_intent(&EditIntent::PasteSlice { slice }),
        Err(SessionError::ClipboardClosedUnsupported)
    );
}

struct RemoveFirst;
impl SessionPolicy for RemoveFirst {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let first = context
            .document()
            .node(context.document().root())
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0];
        Ok(IntentDisposition::Apply(EditPlan::new(
            Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::RemoveNode { node: first }),
            SelectionUpdate::AllDocument,
            None,
        )))
    }
}

#[test]
fn all_after_selection_recomputes_root_range_and_undo_restores_exact_original() {
    let (doc, _, _) = fixture();
    let before = DocumentSelection::all(&doc);
    let mut session =
        DocumentSession::new_with_policy(doc.clone(), before, Box::new(RemoveFirst)).unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert!(session.selection().is_all(session.document()));
    assert_ne!(session.selection(), before);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.selection(), before);
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    assert!(session.selection().is_all(session.document()));
}

#[test]
fn closed_single_table_roundtrip_keeps_root_projection_instead_of_cell_tsv() {
    let mut b = NodeStoreBuilder::new();
    let mut cells = Vec::new();
    for text in ["one\ntwo", "three"] {
        let p = b
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        cells.push(
            b.insert(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([p]),
            )
            .unwrap(),
        );
    }
    let row = b
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap();
    let table = b
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let all = DocumentSelection::all(&doc);
    let slice = DocumentSession::new(doc, all)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert!(slice.is_closed());
    assert_eq!(slice.plain_text(), "one\ntwo\nthree");
    assert_eq!(
        decode_metadata(slice.plain_text(), &encode_metadata(&slice).unwrap()),
        Some(slice)
    );
}
