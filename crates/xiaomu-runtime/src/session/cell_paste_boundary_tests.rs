//! A CellRange Table carrier is not an ordinary table-root paste source.

use super::*;
use crate::clipboard::{
    ClipboardCellRangeRoot, ClipboardExportPurpose, ClipboardExportSpec, ClipboardSlice,
    ClipboardSourceBoundary, ClipboardTextProjection, decode_metadata, encode_metadata,
};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

struct Export;
impl SessionPolicy for Export {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(
            ClipboardExportSpec::new()
                .with_closed_cell_ranges()
                .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
        ))
    }
}

fn fixture(count: usize) -> (XiaomuDocument, NodeId, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let mut text = |value: &str| {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(value, Default::default()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    };
    let intro = text("target");
    let leaves = (0..count)
        .map(|index| text(&format!("cell{index}")))
        .collect::<Vec<_>>();
    let mut cells = Vec::new();
    for leaf in leaves {
        cells.push(
            builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([leaf]),
                )
                .unwrap(),
        );
    }
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells.clone()),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        cells,
    )
}

fn source(whole: bool, native: bool) -> ClipboardSlice {
    let (document, intro, cells) = fixture(2);
    let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(intro));
    let mut session = if native {
        DocumentSession::new_with_policy(document, selection, Box::new(Export)).unwrap()
    } else {
        DocumentSession::new(document, selection).unwrap()
    };
    session
        .set_cell_range_selection(cells[0], cells[usize::from(whole)])
        .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    let metadata = encode_metadata(&slice).unwrap();
    let slice = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert!(!slice.is_closed());
    if native {
        assert_eq!(
            slice.source_boundary(),
            Some(ClipboardSourceBoundary::CellRange {
                root_form: if whole {
                    ClipboardCellRangeRoot::Table
                } else {
                    ClipboardCellRangeRoot::Rows
                }
            })
        );
        assert_eq!(slice.source_boundary().unwrap().open_depths(), Some((1, 1)));
        assert!(metadata.starts_with("xiaomu.clipboard.v14\n"));
    } else {
        assert_eq!(slice.source_boundary(), None);
    }
    slice
}

fn target(whole: bool, rectangular: bool) -> (DocumentSession, NodeId) {
    let (document, intro, cells) = fixture(if whole { 2 } else { 1 });
    let mut session = DocumentSession::new(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(intro)),
    )
    .unwrap();
    if rectangular {
        session
            .set_cell_range_selection(cells[0], *cells.last().unwrap())
            .unwrap();
    }
    (session, intro)
}

#[test]
fn native_cell_carriers_do_not_enter_generic_paragraph_or_rectangle_paste() {
    let mut observed = Vec::new();
    for whole in [false, true] {
        for rectangular in [false, true] {
            let (mut session, _) = target(whole, rectangular);
            let outcome = session.apply_intent(&EditIntent::PasteSlice {
                slice: source(whole, true),
            });
            observed.push((whole, rectangular, outcome));
        }
    }
    assert_eq!(
        observed,
        vec![
            (false, false, Err(SessionError::UnsupportedTableOperation)),
            (false, true, Err(SessionError::UnsupportedTableOperation)),
            (true, false, Err(SessionError::UnsupportedTableOperation)),
            (true, true, Err(SessionError::UnsupportedTableOperation)),
        ]
    );
}

struct Listener(Rc<Cell<usize>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

fn unchanged_rejection(session: &mut DocumentSession, slice: ClipboardSlice) {
    let document = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let history = session.history_depths();
    let typing = session.history.typing_group_open();
    let events = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Listener(events.clone())));
    assert_eq!(
        session.apply_intent(&EditIntent::PasteSlice { slice }),
        Err(SessionError::UnsupportedTableOperation)
    );
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(session.history_depths(), history);
    assert_eq!(session.history.typing_group_open(), typing);
    assert_eq!(events.get(), 0);
}

#[test]
fn native_cell_rejection_preserves_pending_marks_typing_group_and_redo() {
    for whole in [false, true] {
        let (mut session, _) = target(whole, false);
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "a".into() })
            .unwrap();
        assert!(session.history.typing_group_open());
        unchanged_rejection(&mut session, source(whole, true));
        session
            .apply_intent(&EditIntent::InsertText { text: "b".into() })
            .unwrap();
        assert_eq!(
            session.history_depths(),
            (1, 0),
            "rejected paste must not split typing"
        );
        let redo_document = session.document().clone();
        session.undo().unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
            .unwrap();
        assert_eq!(session.history_depths(), (0, 1));
        unchanged_rejection(&mut session, source(whole, true));
        session.redo().unwrap();
        assert_eq!(session.document().store(), redo_document.store());
        let (mut rectangle, _) = target(whole, true);
        unchanged_rejection(&mut rectangle, source(whole, true));
    }
}

#[test]
fn historical_unit_cell_payloads_keep_existing_generic_paste_semantics() {
    for whole in [false, true] {
        for rectangular in [false, true] {
            let (mut session, _) = target(whole, rectangular);
            assert_eq!(
                session.apply_intent(&EditIntent::PasteSlice {
                    slice: source(whole, false)
                }),
                Ok(SessionOutcome::DocumentChanged)
            );
            assert_eq!(session.history_depths(), (1, 0));
        }
    }
}

#[test]
fn ordinary_v14_open_text_keeps_default_fitting_but_whole_roots_does_not() {
    let (document, intro, _) = fixture(1);
    let end = document
        .node(intro)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at("target".len())
        .unwrap();
    let selection = DocumentSelection::new(
        DocumentPosition::Inline(InlinePoint::at_start_of(intro)),
        DocumentPosition::Inline(InlinePoint::new(intro, end, 0, CursorAffinity::After)),
    );
    let source =
        DocumentSession::new_with_policy(document.clone(), selection, Box::new(Export)).unwrap();
    let slice = source.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.source_boundary(), Some(ClipboardSourceBoundary::Open));
    assert!(slice.allows_default_fitting());
    let slice = decode_metadata(slice.plain_text(), &encode_metadata(&slice).unwrap()).unwrap();
    let (mut target, _) = target(false, false);
    assert_eq!(
        target.apply_intent(&EditIntent::PasteSlice { slice }),
        Ok(SessionOutcome::DocumentChanged)
    );
    let all = DocumentSelection::all(&document);
    let whole = DocumentSession::new_with_policy(document, all, Box::new(Export))
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert_eq!(
        whole.source_boundary(),
        Some(ClipboardSourceBoundary::WholeRoots)
    );
    assert!(!whole.allows_default_fitting());
    assert_eq!(
        target.apply_intent(&EditIntent::PasteSlice { slice: whole }),
        Err(SessionError::ClipboardClosedUnsupported)
    );
}

struct ExplicitPolicy;
impl SessionPolicy for ExplicitPolicy {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::PasteSlice { slice } = intent {
            assert!(matches!(
                slice.source_boundary(),
                Some(ClipboardSourceBoundary::CellRange { .. })
            ));
            let node = context
                .selection()
                .as_same_node_inline()
                .unwrap()
                .1
                .node_id();
            return Ok(IntentDisposition::Apply(EditPlan::new(
                Transaction::new(TransactionOrigin::UserInput).with_step(
                    TransactionStep::SetNodeAttrs {
                        node,
                        attrs: NodeAttrs::new(
                            [("policy-handled".into(), AttrValue::Bool(true))].into(),
                        )
                        .unwrap(),
                    },
                ),
                SelectionUpdate::MapExisting,
                None,
            )));
        }
        Ok(IntentDisposition::Continue)
    }
}

#[test]
fn explicit_host_plan_precedes_cell_source_rejection() {
    for whole in [false, true] {
        let (document, intro, _) = fixture(1);
        let selection = DocumentSelection::collapsed(InlinePoint::at_start_of(intro));
        let mut session =
            DocumentSession::new_with_policy(document.clone(), selection, Box::new(ExplicitPolicy))
                .unwrap();
        assert_eq!(
            session.apply_intent(&EditIntent::PasteSlice {
                slice: source(whole, true)
            }),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_eq!(
            session
                .document()
                .node(intro)
                .unwrap()
                .attrs()
                .get("policy-handled"),
            Some(&AttrValue::Bool(true))
        );
        assert_eq!(session.history_depths(), (1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().store(), document.store());
        assert_eq!(session.selection(), selection);
    }
}
