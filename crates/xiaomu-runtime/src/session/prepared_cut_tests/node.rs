//! Generic whole-node preparation, using the same private-state assertions as cells.

use super::*;
use crate::clipboard::ClipboardSourceBoundary;
use crate::session::DocumentPosition;
use xiaomu_core::selection::{CursorAffinity, NodeGap};

#[derive(Clone, Copy, Debug)]
enum NodeFault {
    None,
    NoPlan,
    Allocate,
    AllocateAdmission,
    Prepare,
    Export,
    MissingExport,
    Empty,
    Core,
    StaleSelection,
    GapAfter,
    NodeAfter,
    AtomicAfter,
    RangeAfter,
    Admission,
}

struct NodeFixture {
    document: XiaomuDocument,
    selected: NodeId,
    tail: NodeId,
    survivor: NodeId,
}

fn node_fixture(kind: NodeKind, large: bool) -> NodeFixture {
    let mut builder = NodeStoreBuilder::new();
    let content = match kind {
        NodeKind::Paragraph => NodeContent::Inline(
            InlineContent::new([
                TextRun::new("whole", MarkSet::new([Mark::Bold]).unwrap()).unwrap()
            ])
            .unwrap(),
        ),
        NodeKind::Quote => {
            let child = paragraph(&mut builder, "nested");
            NodeContent::children([child])
        }
        NodeKind::Table => {
            let p = paragraph(&mut builder, "cell");
            let cell = builder
                .insert(
                    NodeKind::TableHeader,
                    NodeAttrs::empty(),
                    NodeContent::children([p]),
                )
                .unwrap();
            let row = builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children([cell]),
                )
                .unwrap();
            NodeContent::children([row])
        }
        _ => NodeContent::Atomic,
    };
    let selected = builder
        .insert(
            kind,
            attrs(
                "payload",
                if large {
                    AttrValue::String("x".repeat(3 * 1024 * 1024))
                } else {
                    AttrValue::List(vec![AttrValue::Null, AttrValue::String("exact".into())])
                },
            ),
            content,
        )
        .unwrap();
    let tail = paragraph(&mut builder, "tail");
    let survivor = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([selected, tail, survivor]),
        )
        .unwrap();
    NodeFixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        selected,
        tail,
        survivor,
    }
}

struct NodePolicy {
    selected: NodeId,
    tail: NodeId,
    survivor: NodeId,
    fault: NodeFault,
}

impl SessionPolicy for NodePolicy {
    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        match self.fault {
            NodeFault::NoPlan => return Ok(None),
            NodeFault::Prepare => return Err(PolicyError::new("node Cut policy refusal")),
            _ => {}
        }
        let mut tx = transaction();
        if matches!(
            self.fault,
            NodeFault::Allocate | NodeFault::AllocateAdmission
        ) {
            tx.push_step(TransactionStep::InsertNode {
                parent: context.document().root(),
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: attrs("cut-replacement", AttrValue::Bool(true)),
                content: NodeContent::empty_inline(),
            });
        }
        if !matches!(self.fault, NodeFault::Empty) {
            tx.push_step(TransactionStep::RemoveNode {
                node: self.selected,
            });
        }
        if matches!(self.fault, NodeFault::Core) {
            tx.push_step(TransactionStep::RemoveNode {
                node: context.document().root(),
            });
        }
        let selection = match self.fault {
            NodeFault::StaleSelection => {
                DocumentSelection::collapsed(DocumentPosition::Atomic(self.selected))
            }
            NodeFault::GapAfter => {
                DocumentSelection::collapsed(NodeGap::new(context.document().root(), 0))
            }
            NodeFault::NodeAfter => {
                // Build a genuinely valid final node selection, not stale gaps.
                let candidate = tx.apply(context.document()).unwrap();
                DocumentSelection::node(&candidate, self.tail).unwrap()
            }
            NodeFault::AtomicAfter => {
                DocumentSelection::collapsed(DocumentPosition::Atomic(self.survivor))
            }
            NodeFault::RangeAfter => {
                let offset = context
                    .document()
                    .node(self.tail)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(1)
                    .unwrap();
                DocumentSelection::new(
                    InlinePoint::at_start_of(self.tail),
                    InlinePoint::new(self.tail, offset, 0, CursorAffinity::After),
                )
            }
            _ => DocumentSelection::collapsed(InlinePoint::at_start_of(self.tail)),
        };
        let selection_update = if matches!(
            self.fault,
            NodeFault::Allocate | NodeFault::AllocateAdmission
        ) {
            SelectionUpdate::CaretAtLastInsertedOffset { offset: 0 }
        } else {
            SelectionUpdate::Exact { selection }
        };
        Ok(Some(EditPlan::new(tx, selection_update, None)))
    }

    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(
            purpose,
            ClipboardExportPurpose::Cut,
            "preparation must not substitute Copy"
        );
        match self.fault {
            NodeFault::Export => Err(PolicyError::new("node export refusal")),
            NodeFault::MissingExport => Ok(None),
            _ => Ok(Some(
                ClipboardExportSpec::new()
                    .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
            )),
        }
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        assert!(
            !matches!(intent, EditIntent::Delete),
            "dedicated Cut must not dispatch Delete"
        );
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if matches!(
            self.fault,
            NodeFault::Admission | NodeFault::AllocateAdmission
        ) && document.node(self.selected).is_none()
        {
            return Err(PolicyError::new("node candidate refusal"));
        }
        Ok(())
    }
}

fn node_session(f: &NodeFixture, atomic: bool, fault: NodeFault) -> DocumentSession {
    let mut s = DocumentSession::new_with_policy(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.tail)),
        Box::new(NodePolicy {
            selected: f.selected,
            tail: f.tail,
            survivor: f.survivor,
            fault,
        }),
    )
    .unwrap();
    insert(&mut s, "first");
    s.apply(&transaction()).unwrap();
    s.undo().unwrap();
    let selection = if atomic {
        DocumentSelection::collapsed(DocumentPosition::Atomic(f.selected))
    } else {
        DocumentSelection::node(s.document(), f.selected).unwrap()
    };
    s.set_document_selection(selection).unwrap();
    s
}

fn sentinels(s: &mut DocumentSession) {
    // Deliberately adversarial, individually valid transients; setters normally clear these.
    s.stored_marks = Some(MarkSet::new([Mark::Italic]).unwrap());
    s.history.restore_typing_group(true);
    let selection = s.selection();
    let spec = InputRuleUndoSpec::new(transaction(), selection).unwrap();
    s.input_rule_undo = Some(
        s.prepare_input_rule_undo(spec, s.document(), selection)
            .unwrap(),
    );
}

#[test]
fn prepared_node_and_atomic_cut_publish_exact_subtrees_and_isolated_history() {
    for (kind, atomic) in [
        (NodeKind::Image, false),
        (NodeKind::Image, true),
        (NodeKind::HorizontalRule, false),
        (NodeKind::HorizontalRule, true),
        (NodeKind::Paragraph, false),
        (NodeKind::Quote, false),
        (NodeKind::Table, false),
    ] {
        let f = node_fixture(kind, false);
        let mut s = node_session(&f, atomic, NodeFault::None);
        sentinels(&mut s);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let copied = s
            .clipboard_slice_for(ClipboardExportPurpose::Cut)
            .unwrap()
            .unwrap();
        assert_eq!(
            copied.source_boundary(),
            Some(if atomic {
                ClipboardSourceBoundary::Open
            } else {
                ClipboardSourceBoundary::WholeRoots
            })
        );
        assert_eq!(copied.roots().len(), 1);
        assert_eq!(
            copied.roots()[0].attrs(),
            s.document().node(f.selected).unwrap().attrs()
        );
        let mut writer = Writer::seeded();
        assert_eq!(
            publish_through_writer(&mut s, &mut writer),
            Ok(Some(SessionOutcome::DocumentChanged))
        );
        assert_eq!(writer.calls, 1);
        assert_eq!(
            decode_metadata_checked(&writer.text, &writer.metadata),
            ClipboardMetadataDecode::Valid(copied)
        );
        assert!(s.document().node(f.selected).is_none());
        assert_eq!(s.document().node(f.tail), before.document.node(f.tail));
        assert_eq!(
            s.document().node(f.survivor),
            before.document.node(f.survivor)
        );
        assert_eq!(
            s.selection(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(f.tail))
        );
        assert_eq!(s.stored_marks(), None);
        assert!(s.input_rule_undo.is_none());
        assert!(!s.history.typing_group_open());
        assert_eq!(s.history_depths(), (2, 0));
        assert_eq!(events.borrow().len(), 1);
        let history = history_image(&mut s);
        assert_eq!(history.undo[0].group, HistoryGroup::Isolated);
        assert_eq!(history.undo[0].before, before.selection);
        assert_eq!(history.undo[0].after, s.selection());
        let after = s.document().clone();
        let after_selection = s.selection();
        s.undo().unwrap();
        assert_eq!(s.document().store(), before.document.store());
        assert_eq!(s.selection(), before.selection);
        s.redo().unwrap();
        assert_eq!(s.document().store(), after.store());
        assert_eq!(s.selection(), after_selection);
        insert(&mut s, "next");
        assert_eq!(s.history_depths(), (3, 0));
        assert_eq!(writer.calls, 1);
    }
}

#[test]
fn node_cut_rejections_preserve_full_session_writer_and_future_allocator() {
    for fault in [
        NodeFault::Prepare,
        NodeFault::Export,
        NodeFault::MissingExport,
        NodeFault::Empty,
        NodeFault::Core,
        NodeFault::StaleSelection,
        NodeFault::GapAfter,
        NodeFault::NodeAfter,
        NodeFault::AtomicAfter,
        NodeFault::RangeAfter,
        NodeFault::Admission,
    ] {
        for atomic in [false, true] {
            let f = node_fixture(NodeKind::Image, false);
            let mut s = node_session(&f, atomic, fault);
            let mut control = node_session(&f, atomic, fault);
            sentinels(&mut s);
            sentinels(&mut control);
            let events = listen(&mut s);
            let before = Snapshot::capture(&mut s, &events);
            let mut writer = Writer::seeded();
            assert!(
                publish_through_writer(&mut s, &mut writer).is_err(),
                "{fault:?}"
            );
            assert_eq!(writer, Writer::seeded());
            before.assert_unchanged(&mut s, &events);
            assert_future_allocation_matches(&mut s, &mut control);
        }
    }
}

#[test]
fn node_cut_drop_preserves_transients_and_allocator() {
    for atomic in [false, true] {
        let f = node_fixture(NodeKind::Image, false);
        let mut s = node_session(&f, atomic, NodeFault::None);
        let mut control = node_session(&f, atomic, NodeFault::None);
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let prepared = s.prepare_cut().unwrap().unwrap();
        assert_eq!(prepared.clipboard_slice().roots().len(), 1);
        drop(prepared);
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn node_export_budget_and_unknown_projection_reject_before_publication() {
    for (kind, large) in [
        (NodeKind::Image, true),
        (NodeKind::Custom("opaque".into()), false),
    ] {
        let f = node_fixture(kind, large);
        let mut s = node_session(&f, true, NodeFault::None);
        let mut control = node_session(&f, true, NodeFault::None);
        sentinels(&mut s);
        sentinels(&mut control);
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        assert!(publish_through_writer(&mut s, &mut writer).is_err());
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
        assert_future_allocation_matches(&mut s, &mut control);
    }
}

#[test]
fn no_dedicated_plan_preserves_node_and_atomic_legacy_export() {
    for atomic in [false, true] {
        let f = node_fixture(NodeKind::Image, false);
        for policy in [None, Some(Box::new(ExportOnly) as Box<dyn SessionPolicy>)] {
            let mut s = DocumentSession::new(
                f.document.clone(),
                DocumentSelection::collapsed(InlinePoint::at_start_of(f.tail)),
            )
            .unwrap();
            if let Some(policy) = policy {
                s = DocumentSession::new_with_policy(f.document.clone(), s.selection(), policy)
                    .unwrap();
            }
            s.set_document_selection(if atomic {
                DocumentSelection::collapsed(DocumentPosition::Atomic(f.selected))
            } else {
                DocumentSelection::node(s.document(), f.selected).unwrap()
            })
            .unwrap();
            let events = listen(&mut s);
            let before = Snapshot::capture(&mut s, &events);
            let expected = s.clipboard_slice_for(ClipboardExportPurpose::Cut).unwrap();
            let mut writer = Writer::seeded();
            assert_eq!(publish_through_writer(&mut s, &mut writer), Ok(None));
            assert_eq!(
                s.clipboard_slice_for(ClipboardExportPurpose::Cut).unwrap(),
                expected
            );
            assert_eq!(writer, Writer::seeded());
            before.assert_unchanged(&mut s, &events);
        }
        let mut s = node_session(&f, atomic, NodeFault::NoPlan);
        assert!(s.prepare_cut().unwrap().is_none());
    }
}

#[test]
fn dedicated_cut_does_not_infer_node_identity_from_gaps_all_or_text() {
    let f = node_fixture(NodeKind::Image, false);
    let explicit = DocumentSelection::node(&f.document, f.selected).unwrap();
    let end = f
        .document
        .node(f.tail)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(1)
        .unwrap();
    for selection in [
        DocumentSelection::new(explicit.anchor(), explicit.focus()),
        DocumentSelection::new(explicit.focus(), explicit.anchor()),
        DocumentSelection::all(&f.document),
        DocumentSelection::collapsed(NodeGap::new(f.document.root(), 0)),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.tail)),
        DocumentSelection::new(
            InlinePoint::at_start_of(f.tail),
            InlinePoint::new(f.tail, end, 0, CursorAffinity::After),
        ),
        DocumentSelection::new(
            DocumentPosition::Atomic(f.selected),
            DocumentPosition::Atomic(f.survivor),
        ),
    ] {
        let mut s = node_session(&f, false, NodeFault::None);
        s.set_document_selection(selection).unwrap();
        let events = listen(&mut s);
        let before = Snapshot::capture(&mut s, &events);
        let mut writer = Writer::seeded();
        assert!(publish_through_writer(&mut s, &mut writer).is_err());
        assert_eq!(writer, Writer::seeded());
        before.assert_unchanged(&mut s, &events);
    }
}

#[path = "node_allocation.rs"]
mod allocation;
