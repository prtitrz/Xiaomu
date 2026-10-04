//! Full document selections consume Core mixed structural maps without projection.

#[allow(dead_code)]
mod mixed_structure_support;
use mixed_structure_support::*;
use xiaomu_core::document::{
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, XiaomuDocument,
};
use xiaomu_core::selection::NodeGap;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, EditPlan, IntentDisposition,
    PolicyError, SelectionUpdate, SessionContext, SessionPolicy,
};

// Exercise the public intent pipeline while selecting MapExisting explicitly;
// the default split/join caret policies are covered by mixed_structure_session.
struct MappedPlan(Transaction);
impl SessionPolicy for MappedPlan {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        Ok(IntentDisposition::Apply(EditPlan::new(
            self.0.clone(),
            SelectionUpdate::MapExisting,
            None,
        )))
    }
}

fn session(
    document: &XiaomuDocument,
    selection: DocumentSelection,
    step: TransactionStep,
) -> DocumentSession {
    DocumentSession::new_with_policy(
        document.clone(),
        selection,
        Box::new(MappedPlan(
            Transaction::new(TransactionOrigin::UserInput).with_step(step),
        )),
    )
    .unwrap()
}

#[test]
fn reverse_range_maps_across_same_boundary_split_and_join_without_losing_orientation() {
    let mut builder = NodeStoreBuilder::new();
    let first = block(&mut builder, "ab", &[(1, true), (1, true), (1, false)]);
    let document = finish(builder, &[first]);
    let selection = DocumentSelection::new(point(first, 1, 3), point(first, 1, 0));
    let mut session = session(
        &document,
        selection,
        TransactionStep::SplitInlineNode {
            at: point(first, 1, 1),
        },
    );
    let events = listen(&mut session);
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let tail = children(session.document(), document.root())[1];
    assert_eq!(
        session.selection(),
        DocumentSelection::new(point(tail, 0, 2), point(first, 1, 0))
    );
    round_trip(&mut session, &document, selection, &events);

    let split_document = session.document().clone();
    let selection = session.selection();
    let mut session = self::session(
        &split_document,
        selection,
        TransactionStep::JoinNodes {
            first,
            second: tail,
        },
    );
    let events = listen(&mut session);
    session.apply_intent(&EditIntent::JoinWithPrevious).unwrap();
    assert_eq!(
        session.selection(),
        DocumentSelection::new(point(first, 1, 3), point(first, 1, 0))
    );
    assert_eq!(session.document().store(), document.store());
    round_trip(&mut session, &split_document, selection, &events);
}

#[test]
fn exact_split_gap_uses_range_endpoint_bias_and_preserves_affinity() {
    let mut builder = NodeStoreBuilder::new();
    let first = block(&mut builder, "ab", &[(1, true), (1, false)]);
    let document = finish(builder, &[first]);
    for reverse in [false, true] {
        let start = point(first, 0, 0);
        let end = point(first, 1, 1);
        let selection = if reverse {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        let mut session = session(
            &document,
            selection,
            TransactionStep::SplitInlineNode { at: end },
        );
        let events = listen(&mut session);
        session.apply_intent(&EditIntent::SplitBlock).unwrap();
        let tail = children(session.document(), document.root())[1];
        let end = point(tail, 0, 0);
        let expected = if reverse {
            DocumentSelection::new(end, start)
        } else {
            DocumentSelection::new(start, end)
        };
        assert_eq!(session.selection(), expected);
        round_trip(&mut session, &document, selection, &events);
    }
    let selection = DocumentSelection::collapsed(point(first, 1, 1));
    let mut session = session(
        &document,
        selection,
        TransactionStep::SplitInlineNode {
            at: point(first, 1, 1),
        },
    );
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    assert_eq!(
        session.selection(),
        selection,
        "collapsed MapExisting uses Start bias"
    );
}

#[test]
fn gap_ranges_and_atomic_block_selections_survive_mixed_split_and_join() {
    let mut builder = NodeStoreBuilder::new();
    let first = block(&mut builder, "a", &[(1, true), (1, false)]);
    let second = block(&mut builder, "b", &[(0, true), (0, false)]);
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let document = finish(builder, &[first, second, rule]);
    let root = document.root();
    let gap = |index| DocumentPosition::Gap(NodeGap::new(root, index));
    for joining in [false, true] {
        let step = if joining {
            TransactionStep::JoinNodes { first, second }
        } else {
            TransactionStep::SplitInlineNode {
                at: point(first, 1, 1),
            }
        };
        let after_end = if joining { 2 } else { 4 };
        for (selection, expected) in [
            (
                DocumentSelection::new(gap(3), gap(0)),
                DocumentSelection::new(gap(after_end), gap(0)),
            ),
            (
                DocumentSelection::collapsed(gap(1)),
                DocumentSelection::collapsed(gap(1)),
            ),
            (
                DocumentSelection::collapsed(rule),
                DocumentSelection::collapsed(rule),
            ),
        ] {
            let mut session = session(&document, selection, step.clone());
            let events = listen(&mut session);
            session
                .apply_intent(if joining {
                    &EditIntent::JoinWithPrevious
                } else {
                    &EditIntent::SplitBlock
                })
                .unwrap();
            assert_eq!(session.selection(), expected);
            round_trip(&mut session, &document, selection, &events);
        }
    }
}

fn table() -> (XiaomuDocument, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = block(&mut builder, "a", &[(1, true), (1, true), (1, false)]);
    let second = block(&mut builder, "b", &[(0, false), (0, true)]);
    let cell_a = container(&mut builder, NodeKind::TableCell, &[first, second]);
    let other = block(&mut builder, "", &[(0, true)]);
    let cell_b = container(&mut builder, NodeKind::TableCell, &[other]);
    let row = container(&mut builder, NodeKind::TableRow, &[cell_a, cell_b]);
    let table = container(&mut builder, NodeKind::Table, &[row]);
    (finish(builder, &[table]), cell_a, cell_b, first, second)
}

#[test]
fn cell_rectangle_keeps_cells_and_parked_ordinal_through_mixed_split_and_join() {
    for joining in [false, true] {
        let (document, cell_a, cell_b, first, second) = table();
        let park = if joining {
            point(second, 0, 1)
        } else {
            point(first, 1, 3)
        };
        let selection = DocumentSelection::cell_range(cell_a, cell_b, park.into());
        let step = if joining {
            TransactionStep::JoinNodes { first, second }
        } else {
            TransactionStep::SplitInlineNode {
                at: point(first, 1, 1),
            }
        };
        let mut session = session(&document, selection, step);
        let events = listen(&mut session);
        session
            .apply_intent(if joining {
                &EditIntent::JoinWithPrevious
            } else {
                &EditIntent::SplitBlock
            })
            .unwrap();
        let expected_park = if joining {
            point(first, 1, 4)
        } else {
            let tail = children(session.document(), cell_a)[1];
            point(tail, 0, 2)
        };
        assert_eq!(
            session.selection(),
            DocumentSelection::cell_range(cell_a, cell_b, expected_park.into())
        );
        assert_eq!(
            session
                .selection()
                .active_cell_range()
                .unwrap()
                .cells(session.document())
                .unwrap(),
            vec![vec![cell_a, cell_b]]
        );
        round_trip(&mut session, &document, selection, &events);
    }
}

#[test]
fn default_cell_split_and_backspace_keep_sibling_cells_and_payloads_exact() {
    let (document, cell_a, cell_b, first, _) = table();
    let selection = DocumentSelection::collapsed(point(first, 1, 2));
    let mut session = DocumentSession::new(document.clone(), selection).unwrap();
    let events = listen(&mut session);
    session.apply_intent(&EditIntent::SplitBlock).unwrap();
    let tail = focus(&session).node_id();
    assert_eq!(session.document().parent_of(tail), Some(cell_a));
    assert_eq!(session.document().node(cell_b), document.node(cell_b));
    assert_payloads(&document, session.document(), &atoms(&document, first));
    round_trip(&mut session, &document, selection, &events);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(focus(&session), point(first, 1, 2));
    assert_eq!(session.document().store(), document.store());
}
