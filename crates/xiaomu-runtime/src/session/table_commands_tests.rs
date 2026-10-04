//! Typed merge/split commands through the public, atomic Session entry point.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, TableRect, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_core::text::TextOffset;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::*;

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    cells: Vec<NodeId>,
    paragraphs: Vec<NodeId>,
}

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap()
}

fn fixture(spans: bool) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let shape: &[&[&str]] = if spans {
        &[&["A", "B"], &["C"], &["D", "E", "F"]]
    } else {
        // Blank source blocks are deliberate: generic merge must retain them.
        &[&["", "B"], &["", "D"]]
    };
    let mut cells = Vec::new();
    let mut paragraphs = Vec::new();
    let mut rows = Vec::new();
    for (row_index, values) in shape.iter().enumerate() {
        let mut children = Vec::new();
        for (column_index, value) in values.iter().enumerate() {
            let p = paragraph(&mut builder, value);
            let first = row_index == 0 && column_index == 0;
            let cell_attrs = if first {
                let mut values = vec![("opaque", AttrValue::Null)];
                if spans {
                    values.extend([
                        ("rowspan", AttrValue::Integer(2)),
                        ("colspan", AttrValue::Integer(2)),
                        (
                            "colwidth",
                            AttrValue::List(vec![AttrValue::Integer(0), AttrValue::Integer(120)]),
                        ),
                    ]);
                }
                attrs(&values)
            } else {
                NodeAttrs::empty()
            };
            let cell = builder
                .insert(
                    if first {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    cell_attrs,
                    NodeContent::children([p]),
                )
                .unwrap();
            paragraphs.push(p);
            cells.push(cell);
            children.push(cell);
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(children),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        table,
        cells,
        paragraphs,
    }
}

fn caret(node: NodeId) -> DocumentSelection {
    DocumentSelection::collapsed(InlinePoint::at_start_of(node))
}

fn at(f: &Fixture, node: NodeId) -> DocumentSession {
    DocumentSession::new(f.document.clone(), caret(node)).unwrap()
}

fn range(f: &Fixture, anchor: usize, focus: usize) -> DocumentSelection {
    DocumentSelection::cell_range(
        f.cells[anchor],
        f.cells[focus],
        InlinePoint::at_start_of(f.paragraphs[anchor]).into(),
    )
}

struct Notifications(Rc<Cell<usize>>);
impl DocumentChangeListener for Notifications {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

fn listen(session: &mut DocumentSession) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Notifications(count.clone())));
    count
}

struct State {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    typing_group: bool,
    input_rule: bool,
}
impl State {
    fn capture(session: &DocumentSession) -> Self {
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            typing_group: session.history.typing_group_open(),
            input_rule: session.input_rule_undo_available(),
        }
    }
    fn assert_unchanged(&self, session: &DocumentSession) {
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(session.history.typing_group_open(), self.typing_group);
        assert_eq!(session.input_rule_undo_available(), self.input_rule);
    }
}

fn assert_no_changes(session: &mut DocumentSession, intents: &[EditIntent]) {
    let before = State::capture(session);
    let count = listen(session);
    for intent in intents {
        assert_eq!(session.apply_intent(intent), Ok(SessionOutcome::NoChange));
        before.assert_unchanged(session);
        assert_eq!(count.get(), 0);
    }
}

#[test]
fn merge_live_reversed_range_preserves_all_blocks_and_exact_history() {
    let f = fixture(false);
    let mut session = at(&f, f.intro);
    session
        .set_cell_range_selection(f.cells[3], f.cells[0])
        .unwrap();
    let before = session.selection();
    session.stored_marks = Some(MarkSet::new([Mark::Bold]).unwrap());
    let count = listen(&mut session);
    assert_eq!(
        session.apply_intent(&EditIntent::MergeTableCells),
        Ok(SessionOutcome::DocumentChanged)
    );
    assert_eq!(count.get(), 1);
    assert_eq!(session.history_depths(), (1, 0));
    assert!(!session.history.typing_group_open());
    assert!(session.stored_marks().is_none());
    let survivor = session.document().node(f.cells[0]).unwrap();
    assert_eq!(survivor.kind(), &NodeKind::TableHeader);
    assert_eq!(survivor.attrs().get("opaque"), Some(&AttrValue::Null));
    assert_eq!(
        survivor.content().as_children().unwrap(),
        f.paragraphs.as_slice()
    );
    for paragraph in &f.paragraphs {
        assert_eq!(
            session.document().node(*paragraph),
            f.document.node(*paragraph)
        );
    }
    let selected = session.selection().active_cell_range().unwrap();
    assert_eq!(
        (selected.anchor(), selected.focus()),
        (f.cells[0], f.cells[0])
    );
    assert_eq!(
        selected.logical_rect(session.document()).unwrap(),
        TableRect::new(0, 0, 2, 2).unwrap()
    );
    let changed = session.document().clone();
    let after = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), changed.store());
    assert_eq!(session.selection(), after);
    assert_eq!(count.get(), 3);
}

#[test]
fn merge_accepts_closed_ranges_with_existing_spans_and_fully_covered_rows() {
    let f = fixture(true);
    let mut session = DocumentSession::new(f.document.clone(), range(&f, 0, 5)).unwrap();
    session.apply_intent(&EditIntent::MergeTableCells).unwrap();
    let grid = session.document().table_grid(f.table).unwrap();
    assert_eq!(grid.origins().len(), 1);
    assert_eq!((grid.rows(), grid.columns()), (3, 3));
    assert_eq!(
        session
            .document()
            .node(f.cells[0])
            .unwrap()
            .content()
            .as_children()
            .unwrap(),
        f.paragraphs.as_slice()
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), range(&f, 0, 5));
}

#[test]
fn split_single_cell_range_keeps_only_survivor_and_redo_allocated_identities() {
    let f = fixture(true);
    let mut session = at(&f, f.intro);
    session
        .set_cell_range_selection(f.cells[0], f.cells[0])
        .unwrap();
    let before = session.selection();
    let count = listen(&mut session);
    session.apply_intent(&EditIntent::SplitTableCell).unwrap();
    assert_eq!(
        session.selection(),
        before,
        "range and park retain exact identities"
    );
    let selected = session.selection().active_cell_range().unwrap();
    assert_eq!(
        selected.logical_rect(session.document()).unwrap(),
        TableRect::new(0, 0, 1, 1).unwrap()
    );
    let grid = session.document().table_grid(f.table).unwrap();
    for row in 0..2 {
        for column in 0..2 {
            let cell = grid.slot(row, column).unwrap();
            let node = session.document().node(cell).unwrap();
            assert_eq!(node.kind(), &NodeKind::TableHeader);
            assert_eq!(node.attrs().get("opaque"), Some(&AttrValue::Null));
            assert_eq!(
                node.attrs().get("colwidth"),
                Some(&AttrValue::List(vec![AttrValue::Integer(if column == 0 {
                    0
                } else {
                    120
                })]))
            );
            if row != 0 || column != 0 {
                let paragraph = node.content().as_children().unwrap()[0];
                assert_eq!(
                    session
                        .document()
                        .node(paragraph)
                        .unwrap()
                        .content()
                        .as_inline()
                        .unwrap()
                        .len_bytes(),
                    0
                );
            }
        }
    }
    assert_eq!(
        session.document().node(f.paragraphs[0]),
        f.document.node(f.paragraphs[0])
    );
    let changed = session.document().clone();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(
        session.document().store(),
        changed.store(),
        "redo restores cells and empty paragraph IDs"
    );
    assert_eq!(session.selection(), before);
    assert_eq!(count.get(), 3);
}

#[test]
fn split_preserves_mixed_inline_caret_and_structural_gap_caret() {
    let mut f = fixture(true);
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::at_start_of(f.paragraphs[0]),
            kind: AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::hard_break(),
        })
        .apply(&f.document)
        .unwrap();
    let mixed = InlinePoint::new(f.paragraphs[0], TextOffset::ZERO, 1, CursorAffinity::After);
    for selection in [
        DocumentSelection::collapsed(mixed),
        DocumentSelection::collapsed(NodeGap::new(f.cells[0], 1)),
    ] {
        let mut session = DocumentSession::new(f.document.clone(), selection).unwrap();
        session.stored_marks = Some(MarkSet::new([Mark::Italic]).unwrap());
        session.apply_intent(&EditIntent::SplitTableCell).unwrap();
        assert_eq!(session.selection(), selection);
        assert!(session.stored_marks().is_none());
        session.undo().unwrap();
        assert_eq!(session.selection(), selection);
        assert_eq!(session.document().store(), f.document.store());
    }
}

#[test]
fn ineligible_commands_preserve_typing_marks_group_and_coalescing_in_sequence() {
    let f = fixture(false);
    for node in [f.intro, f.paragraphs[1]] {
        let mut session = at(&f, node);
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        assert!(session.history.typing_group_open());
        assert!(session.stored_marks().is_some());
        assert_no_changes(
            &mut session,
            &[
                EditIntent::MergeTableCells,
                EditIntent::SplitTableCell,
                EditIntent::SplitTableCell,
                EditIntent::MergeTableCells,
            ],
        );
        session
            .apply_intent(&EditIntent::InsertText { text: "y".into() })
            .unwrap();
        assert_eq!(
            session.history_depths(),
            (1, 0),
            "no-op commands must not split adjacent typing"
        );
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
    }
}

#[test]
fn nonclosed_and_single_origin_merge_and_multicell_split_are_noops() {
    let f = fixture(true);
    // C -> D starts below A's 2-row span, so this rectangle is not closed.
    for selection in [range(&f, 2, 3), range(&f, 3, 2)] {
        let mut session = DocumentSession::new(f.document.clone(), selection).unwrap();
        assert!(
            !selection
                .active_cell_range()
                .unwrap()
                .is_closed_rect(&f.document)
                .unwrap()
        );
        assert_no_changes(
            &mut session,
            &[EditIntent::MergeTableCells, EditIntent::SplitTableCell],
        );
    }
    let mut single = DocumentSession::new(f.document.clone(), range(&f, 0, 0)).unwrap();
    assert_no_changes(
        &mut single,
        &[EditIntent::MergeTableCells, EditIntent::MergeTableCells],
    );
    let mut multiple = DocumentSession::new(f.document.clone(), range(&f, 0, 5)).unwrap();
    assert_no_changes(
        &mut multiple,
        &[EditIntent::SplitTableCell, EditIntent::SplitTableCell],
    );
    let mut unit = DocumentSession::new(f.document.clone(), range(&f, 1, 1)).unwrap();
    assert_no_changes(
        &mut unit,
        &[EditIntent::MergeTableCells, EditIntent::SplitTableCell],
    );
}

#[test]
fn ordinary_text_and_whole_node_selections_never_split_the_focus_cell() {
    let f = fixture(true);
    for selection in [
        DocumentSelection::new(
            InlinePoint::at_start_of(f.paragraphs[0]),
            InlinePoint::at_start_of(f.paragraphs[1]),
        ),
        DocumentSelection::node(&f.document, f.table).unwrap(),
        DocumentSelection::all(&f.document),
    ] {
        let mut session = DocumentSession::new(f.document.clone(), selection).unwrap();
        assert_no_changes(
            &mut session,
            &[EditIntent::MergeTableCells, EditIntent::SplitTableCell],
        );
    }
}

#[test]
fn merge_then_split_remain_separate_history_units_with_exact_range_restoration() {
    let f = fixture(false);
    let before = range(&f, 3, 0);
    let mut session = DocumentSession::new(f.document.clone(), before).unwrap();
    session.apply_intent(&EditIntent::MergeTableCells).unwrap();
    let merged = session.document().clone();
    let merged_selection = session.selection();
    session.apply_intent(&EditIntent::SplitTableCell).unwrap();
    let split = session.document().clone();
    let split_selection = session.selection();
    assert_eq!(session.history_depths(), (2, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), merged.store());
    assert_eq!(session.selection(), merged_selection);
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), merged.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), split.store());
    assert_eq!(session.selection(), split_selection);
}

struct ExactCommandPolicy {
    intent: EditIntent,
    expected_target: DocumentSelection,
    table: NodeId,
    after: DocumentSelection,
}
impl SessionPolicy for ExactCommandPolicy {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if intent != &self.intent {
            return Ok(IntentDisposition::Continue);
        }
        assert_eq!(
            context.selection(),
            self.expected_target,
            "policy receives live target before default routing"
        );
        let range = context.selection().active_cell_range().unwrap();
        let step = match intent {
            EditIntent::MergeTableCells => TransactionStep::MergeTableCells {
                table: self.table,
                rect: range.logical_rect(context.document()).unwrap(),
            },
            EditIntent::SplitTableCell => TransactionStep::SplitTableCell {
                table: self.table,
                cell: range.anchor(),
            },
            _ => unreachable!(),
        };
        Ok(IntentDisposition::Apply(EditPlan::new(
            Transaction::new(TransactionOrigin::UserInput).with_step(step),
            SelectionUpdate::Exact {
                selection: self.after,
            },
            None,
        )))
    }
}

#[test]
fn real_typed_commands_reach_policy_first_and_accept_exact_plans_at_atomic_targets() {
    for (spans, intent) in [
        (false, EditIntent::MergeTableCells),
        (true, EditIntent::SplitTableCell),
    ] {
        let f = fixture(spans);
        let target = range(&f, 0, if spans { 0 } else { 3 });
        let before = caret(f.intro);
        let after = caret(f.paragraphs[0]);
        let mut session = DocumentSession::new_with_policy(
            f.document.clone(),
            before,
            Box::new(ExactCommandPolicy {
                intent: intent.clone(),
                expected_target: target,
                table: f.table,
                after,
            }),
        )
        .unwrap();
        let count = listen(&mut session);
        assert_eq!(
            session.apply_intent_with_selection(target, &intent),
            Ok(SessionOutcome::DocumentChanged)
        );
        assert_eq!(
            session.selection(),
            after,
            "host exact caret overrides generic range semantics"
        );
        assert_eq!(
            count.get(),
            1,
            "tentative target is never separately published"
        );
        let changed = session.document().clone();
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
        assert_eq!(
            session.selection(),
            before,
            "undo restores pre-action selection, not tentative target"
        );
        session.redo().unwrap();
        assert_eq!(session.document().store(), changed.store());
        assert_eq!(session.selection(), after);
    }
}

#[test]
fn invalid_policy_exact_selection_rolls_back_real_merge_atomically() {
    let f = fixture(false);
    let target = range(&f, 0, 3);
    let mut session = DocumentSession::new_with_policy(
        f.document.clone(),
        caret(f.intro),
        Box::new(ExactCommandPolicy {
            intent: EditIntent::MergeTableCells,
            expected_target: target,
            table: f.table,
            // Absorbed endpoint is invalid after merge; no candidate may escape.
            after: target,
        }),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = State::capture(&session);
    let count = listen(&mut session);
    assert_eq!(
        session.apply_intent_with_selection(target, &EditIntent::MergeTableCells),
        Err(SessionError::SelectionInvalid)
    );
    before.assert_unchanged(&session);
    assert_eq!(count.get(), 0);
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
}

struct RejectShapeChange {
    table: NodeId,
    count: usize,
}
impl SessionPolicy for RejectShapeChange {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.table_grid(self.table).unwrap().origins().len() == self.count {
            Ok(())
        } else {
            Err(PolicyError::new("cell-count change rejected"))
        }
    }
}

#[test]
fn default_commands_roll_back_candidate_policy_errors_and_preserve_redo() {
    for (spans, intent) in [
        (false, EditIntent::MergeTableCells),
        (true, EditIntent::SplitTableCell),
    ] {
        let f = fixture(spans);
        let mut session = DocumentSession::new_with_policy(
            f.document.clone(),
            caret(f.intro),
            Box::new(RejectShapeChange {
                table: f.table,
                count: f.cells.len(),
            }),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        session.undo().unwrap();
        let target = range(&f, 0, if spans { 0 } else { 3 });
        let before = State::capture(&session);
        let count = listen(&mut session);
        assert_eq!(
            session.apply_intent_with_selection(target, &intent),
            Err(SessionError::Policy(PolicyError::new(
                "cell-count change rejected"
            )))
        );
        before.assert_unchanged(&session);
        assert_eq!(count.get(), 0);
        session.redo().unwrap();
        assert_eq!(session.history_depths(), (1, 0));
    }
}

#[test]
fn real_core_split_resource_error_rolls_back_marks_group_history_and_listeners() {
    let mut builder = NodeStoreBuilder::new();
    let p = paragraph(&mut builder, "A");
    let cell = builder
        .insert(
            NodeKind::TableCell,
            attrs(&[("colspan", AttrValue::Integer(100_001))]),
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
            NodeContent::children([table]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mut session = DocumentSession::new(document.clone(), caret(p)).unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "x".into() })
        .unwrap();
    let before = State::capture(&session);
    let count = listen(&mut session);
    assert_eq!(
        session.apply_intent(&EditIntent::SplitTableCell),
        Err(SessionError::Core(xiaomu_core::Error::TableResourceLimit))
    );
    before.assert_unchanged(&session);
    assert_eq!(count.get(), 0);
    session
        .apply_intent(&EditIntent::InsertText { text: "y".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), document.store());
}
