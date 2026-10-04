//! Logical CellRange origins are distinct from covered slots and intersections.
//!
//! The origin-in-rect rule follows actual prosemirror-tables 1.8.5
//! TableMap.cellsInRect. These are headless Runtime contracts, not GUI or
//! product-specific Backspace/typing/clipboard parity claims.

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineContent, LinkMark, Mark, MarkKind, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TableRect, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{InlinePoint, NodeGap};
use xiaomu_core::text::{TextBuffer, TextOffset, TextRange};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CellRange, DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, PolicyError,
    SessionError, SessionOutcome, SessionPolicy,
};

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    cells: BTreeMap<&'static str, NodeId>,
    texts: BTreeMap<&'static str, NodeId>,
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
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn fixture(rows: &[&[(&'static str, i64, i64, bool)]]) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let mut cells = BTreeMap::new();
    let mut texts = BTreeMap::new();
    let mut row_ids = Vec::new();
    for row in rows {
        let mut row_cells = Vec::new();
        for &(label, rowspan, colspan, header) in *row {
            let text = paragraph(&mut builder, label);
            let cell = builder
                .insert(
                    if header {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    attrs(&[
                        ("rowspan", AttrValue::Integer(rowspan)),
                        ("colspan", AttrValue::Integer(colspan)),
                        ("backgroundColor", AttrValue::String(label.into())),
                        ("colwidth", AttrValue::Null),
                    ]),
                    NodeContent::children([text]),
                )
                .unwrap();
            cells.insert(label, cell);
            texts.insert(label, text);
            row_cells.push(cell);
        }
        row_ids.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(row_cells),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(row_ids),
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
        texts,
    }
}

/// A A B C / A A D E / F G G H / I G G J.
fn spanning() -> Fixture {
    fixture(&[
        &[("A", 2, 2, true), ("B", 1, 1, false), ("C", 1, 1, false)],
        &[("D", 1, 1, true), ("E", 1, 1, false)],
        &[("F", 1, 1, false), ("G", 2, 2, true), ("H", 1, 1, false)],
        &[("I", 1, 1, false), ("J", 1, 1, false)],
    ])
}

fn session(f: &Fixture) -> DocumentSession {
    DocumentSession::new(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro)),
    )
    .unwrap()
}

fn selection(f: &Fixture, anchor: &str, focus: &str) -> DocumentSelection {
    let cell = f.cells[anchor];
    let row = f.document.parent_of(cell).unwrap();
    let index = children(&f.document, row)
        .iter()
        .position(|id| *id == cell)
        .unwrap();
    DocumentSelection::cell_range(cell, f.cells[focus], NodeGap::new(row, index).into())
}

fn range(f: &Fixture, anchor: &str, focus: &str) -> CellRange {
    selection(f, anchor, focus).active_cell_range().unwrap()
}

fn children(document: &XiaomuDocument, node: NodeId) -> &[NodeId] {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
}

fn inline(document: &XiaomuDocument, node: NodeId) -> &InlineContent {
    document.node(node).unwrap().content().as_inline().unwrap()
}

fn subtree_ids(document: &XiaomuDocument, root: NodeId) -> Vec<NodeId> {
    let mut pending = vec![root];
    let mut ids = Vec::new();
    while let Some(id) = pending.pop() {
        ids.push(id);
        let content = document.node(id).unwrap().content();
        if let Some(children) = content.as_children() {
            pending.extend(children.iter().copied());
        }
        if let Some(inline) = content.as_inline() {
            pending.extend(inline.atoms().iter().map(|atom| atom.atom()));
        }
    }
    ids
}

#[test]
fn unit_matrix_contract_and_header_origins_are_unchanged() {
    let f = fixture(&[
        &[("A", 1, 1, true), ("B", 1, 1, false)],
        &[("C", 1, 1, false), ("D", 1, 1, true)],
    ]);
    for (anchor, focus) in [("A", "D"), ("D", "A")] {
        let range = range(&f, anchor, focus);
        assert_eq!(
            range.cells(&f.document).unwrap(),
            vec![
                vec![f.cells["A"], f.cells["B"]],
                vec![f.cells["C"], f.cells["D"]]
            ]
        );
        assert_eq!(
            range.unique_origins(&f.document).unwrap(),
            [f.cells["A"], f.cells["B"], f.cells["C"], f.cells["D"]]
        );
        assert_eq!(
            range.logical_rect(&f.document).unwrap(),
            TableRect::new(0, 0, 2, 2).unwrap()
        );
        assert!(range.is_closed_rect(&f.document).unwrap());
    }
}

#[test]
fn reverse_span_endpoints_bound_complete_cells_without_repeating_slots() {
    let f = spanning();
    for (anchor, focus) in [("A", "G"), ("G", "A")] {
        let range = range(&f, anchor, focus);
        assert_eq!(
            range.logical_rect(&f.document).unwrap(),
            TableRect::new(0, 0, 4, 3).unwrap()
        );
        assert_eq!(
            range.unique_origins(&f.document).unwrap(),
            ["A", "B", "D", "F", "G", "I"].map(|label| f.cells[label])
        );
        assert!(range.is_closed_rect(&f.document).unwrap());
        assert_eq!(
            range.cells(&f.document),
            Err(SessionError::UnsupportedTableOperation)
        );
    }
}

#[test]
fn origin_inside_differs_from_intersection_at_top_and_left_boundaries() {
    let f = spanning();
    let grid = f.document.table_grid(f.table).unwrap();
    for (anchor, focus, expected, intersections) in [
        ("D", "F", vec!["D", "F", "G"], vec!["A", "D", "F", "G"]),
        (
            "D",
            "J",
            vec!["D", "E", "H", "J"],
            vec!["D", "E", "G", "H", "J"],
        ),
    ] {
        for (anchor, focus) in [(anchor, focus), (focus, anchor)] {
            let range = range(&f, anchor, focus);
            let rect = range.logical_rect(&f.document).unwrap();
            assert_eq!(
                range.unique_origins(&f.document).unwrap(),
                expected
                    .iter()
                    .map(|label| f.cells[label])
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                grid.unique_cells_in(rect).unwrap(),
                intersections
                    .iter()
                    .map(|label| f.cells[label])
                    .collect::<Vec<_>>()
            );
            assert!(!range.is_closed_rect(&f.document).unwrap());
        }
    }
}

#[test]
fn rowspan_covered_empty_row_still_has_one_closed_origin() {
    let f = fixture(&[&[("A", 2, 2, true)], &[]]);
    let range = range(&f, "A", "A");
    assert_eq!(
        range.logical_rect(&f.document).unwrap(),
        TableRect::new(0, 0, 2, 2).unwrap()
    );
    assert_eq!(range.unique_origins(&f.document).unwrap(), [f.cells["A"]]);
    assert!(range.is_closed_rect(&f.document).unwrap());
    let row = f.document.table_grid(f.table).unwrap().row_id(1).unwrap();
    assert!(children(&f.document, row).is_empty());
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["A"], f.cells["A"])
        .unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert!(children(session.document(), row).is_empty());
    assert_eq!(children(session.document(), f.cells["A"]).len(), 1);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
}

#[test]
fn all_logical_queries_reject_endpoints_from_another_table() {
    let mut f = spanning();
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTable {
            parent: f.document.root(),
            index: 0,
            rows: 1,
            columns: 1,
        })
        .apply(&f.document)
        .unwrap();
    let other = children(&f.document, f.document.root())[0];
    let cell = f.document.table_grid(other).unwrap().slot(0, 0).unwrap();
    let invalid = DocumentSelection::cell_range(
        f.cells["A"],
        cell,
        NodeGap::new(f.document.parent_of(f.cells["A"]).unwrap(), 0).into(),
    )
    .active_cell_range()
    .unwrap();
    assert_eq!(
        invalid.logical_rect(&f.document),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(
        invalid.unique_origins(&f.document),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(
        invalid.is_closed_rect(&f.document),
        Err(SessionError::SelectionInvalid)
    );
    let mut session = session(&f);
    let before = session.selection();
    assert_eq!(
        session.set_cell_range_selection(f.cells["A"], cell),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.selection(), before);
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

#[test]
fn nonclosed_clear_edits_selected_origins_once_with_exact_undo_redo() {
    for (anchor, focus, selected) in [
        ("F", "D", vec!["D", "F", "G"]),
        ("J", "D", vec!["D", "E", "H", "J"]),
    ] {
        for intent in [EditIntent::Backspace, EditIntent::Delete] {
            let mut f = spanning();
            add_nested_content(&mut f);
            let mut session = session(&f);
            session
                .set_cell_range_selection(f.cells[anchor], f.cells[focus])
                .unwrap();
            let before_selection = session.selection();
            let notifications = Rc::new(Cell::new(0));
            session.add_listener(Box::new(Notifications(notifications.clone())));
            assert_eq!(
                session.apply_intent(&intent).unwrap(),
                SessionOutcome::DocumentChanged
            );
            assert_eq!(notifications.get(), 1);
            assert_eq!(session.history_depths(), (1, 0));
            assert_eq!(session.selection(), before_selection);
            assert_eq!(
                session.document().table_grid(f.table).unwrap(),
                f.document.table_grid(f.table).unwrap()
            );
            for (label, cell) in &f.cells {
                let node = session.document().node(*cell).unwrap();
                assert_eq!(node.kind(), f.document.node(*cell).unwrap().kind());
                assert_eq!(node.attrs(), f.document.node(*cell).unwrap().attrs());
                if selected.contains(label) {
                    for id in subtree_ids(&f.document, *cell).into_iter().skip(1) {
                        assert!(session.document().node(id).is_none());
                    }
                    let blocks = children(session.document(), *cell);
                    assert_eq!(blocks.len(), 1);
                    assert_eq!(inline(session.document(), blocks[0]).len_bytes(), 0);
                } else {
                    for id in subtree_ids(&f.document, *cell) {
                        assert_eq!(session.document().node(id), f.document.node(id));
                    }
                }
            }
            let changed = session.document().clone();
            let revision = changed.revision();
            assert_eq!(
                session.apply_intent(&intent).unwrap(),
                SessionOutcome::NoChange
            );
            assert_eq!(session.document().revision(), revision);
            assert_eq!(notifications.get(), 1);
            session.undo().unwrap();
            assert_eq!(session.document().store(), f.document.store());
            assert_eq!(session.selection(), before_selection);
            session.redo().unwrap();
            assert_eq!(session.document().store(), changed.store());
            assert_eq!(session.selection(), before_selection);
            assert_eq!(notifications.get(), 3);
        }
    }
}

fn add_nested_content(f: &mut Fixture) -> (NodeId, NodeId) {
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTable {
            parent: f.cells["D"],
            index: 1,
            rows: 1,
            columns: 1,
        })
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::at_start_of(f.texts["D"]),
            kind: AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::hard_break()
                .with_marks(MarkSet::new([Mark::Italic]).unwrap()),
        })
        .apply(&f.document)
        .unwrap();
    let nested_table = children(&f.document, f.cells["D"])[1];
    let nested_cell = f
        .document
        .table_grid(nested_table)
        .unwrap()
        .slot(0, 0)
        .unwrap();
    let nested_text = children(&f.document, nested_cell)[0];
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: nested_text,
            range: TextRange::new(
                inline(&f.document, nested_text).offset_at(0).unwrap(),
                inline(&f.document, nested_text).offset_at(0).unwrap(),
            )
            .unwrap(),
            replacement: "nested".into(),
        })
        .apply(&f.document)
        .unwrap();
    (nested_cell, nested_text)
}

#[test]
fn marks_visit_selected_subtrees_and_atoms_once_with_one_global_toggle_decision() {
    let mut f = spanning();
    let (nested_cell, nested_text) = add_nested_content(&mut f);
    let atom = inline(&f.document, f.texts["D"]).atoms()[0].atom();
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["F"], f.cells["D"])
        .unwrap();
    let before_selection = session.selection();
    let origins = before_selection
        .active_cell_range()
        .unwrap()
        .unique_origins(session.document())
        .unwrap();
    assert_eq!(origins, [f.cells["D"], f.cells["F"], f.cells["G"]]);
    assert!(!origins.contains(&nested_cell));
    let notifications = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Notifications(notifications.clone())));
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(notifications.get(), 1);
    assert_eq!(session.history_depths(), (1, 0));
    for text in [f.texts["D"], f.texts["F"], f.texts["G"], nested_text] {
        assert!(
            inline(session.document(), text)
                .runs()
                .iter()
                .all(|run| run.marks().contains(MarkKind::Bold))
        );
    }
    let marks = session
        .document()
        .node(atom)
        .unwrap()
        .content()
        .as_inline_atom()
        .unwrap()
        .marks();
    assert!(marks.contains(MarkKind::Bold));
    assert!(marks.contains(MarkKind::Italic));
    for label in ["A", "B", "C", "E", "H", "I", "J"] {
        assert_eq!(
            session.document().node(f.texts[label]),
            f.document.node(f.texts[label])
        );
    }
    for cell in f.cells.values() {
        assert_eq!(session.document().node(*cell), f.document.node(*cell));
    }
    assert_eq!(session.selection(), before_selection);
    let changed = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before_selection);
    session.redo().unwrap();
    assert_eq!(session.document().store(), changed.store());
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.history_depths(), (2, 0));
    let revision = session.document().revision();
    let count = notifications.get();
    assert_eq!(
        session
            .apply_intent(&EditIntent::RemoveMark {
                kind: MarkKind::Bold
            })
            .unwrap(),
        SessionOutcome::NoChange
    );
    assert_eq!(session.document().revision(), revision);
    assert_eq!(notifications.get(), count);
}

#[test]
fn explicit_marks_replace_exact_values_and_empty_ranges_are_noops() {
    let f = spanning();
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["F"], f.cells["D"])
        .unwrap();
    for url in [
        "https://example.invalid/first",
        "https://example.invalid/second",
    ] {
        let mark = Mark::Link(LinkMark::new(url, None));
        session
            .apply_intent(&EditIntent::SetMark { mark: mark.clone() })
            .unwrap();
        for label in ["D", "F", "G"] {
            assert_eq!(
                inline(session.document(), f.texts[label]).runs()[0]
                    .marks()
                    .as_slice(),
                std::slice::from_ref(&mark)
            );
        }
        let revision = session.document().revision();
        assert_eq!(
            session.apply_intent(&EditIntent::SetMark { mark }).unwrap(),
            SessionOutcome::NoChange
        );
        assert_eq!(session.document().revision(), revision);
    }
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Link,
        })
        .unwrap();
    assert_eq!(session.document().store(), f.document.store());
    session.apply_intent(&EditIntent::Delete).unwrap();
    let empty = session.document().clone();
    let depths = session.history_depths();
    for intent in [
        EditIntent::ToggleMark { mark: Mark::Bold },
        EditIntent::SetMark { mark: Mark::Italic },
        EditIntent::RemoveMark {
            kind: MarkKind::Link,
        },
    ] {
        assert_eq!(
            session.apply_intent(&intent).unwrap(),
            SessionOutcome::NoChange
        );
        assert_eq!(session.document().store(), empty.store());
        assert_eq!(session.document().revision(), empty.revision());
        assert_eq!(session.history_depths(), depths);
    }
}

#[test]
fn mixed_cell_marks_toggle_to_one_shared_value_and_restore_exactly() {
    let mut f = spanning();
    let text = f.texts["D"];
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::AddMark {
            node: text,
            range: TextRange::new(
                inline(&f.document, text).offset_at(0).unwrap(),
                inline(&f.document, text).offset_at(1).unwrap(),
            )
            .unwrap(),
            mark: Mark::Bold,
        })
        .apply(&f.document)
        .unwrap();
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["F"], f.cells["D"])
        .unwrap();
    let before = session.selection();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    for label in ["D", "F", "G"] {
        assert!(
            inline(session.document(), f.texts[label]).runs()[0]
                .marks()
                .contains(MarkKind::Bold)
        );
    }
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
}

#[test]
fn nonclosed_span_copy_stays_explicitly_unsupported() {
    let f = spanning();
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["F"], f.cells["D"])
        .unwrap();
    let before = session.selection();
    let notifications = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Notifications(notifications.clone())));
    assert_eq!(
        session.clipboard_slice(),
        Err(SessionError::UnsupportedTableOperation)
    );
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.selection(), before);
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(notifications.get(), 0);
}

fn text_replacements(text: &str) -> [EditIntent; 3] {
    [
        EditIntent::InsertText { text: text.into() },
        EditIntent::PasteText { text: text.into() },
        EditIntent::CommitComposition {
            range: TextRange::empty(TextOffset::ZERO),
            text: text.into(),
        },
    ]
}

fn image_only(f: &mut Fixture, label: &str) {
    let cell = f.cells[label];
    let mut transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::InsertNode {
            parent: cell,
            index: 0,
            kind: NodeKind::Image,
            attrs: attrs(&[
                (
                    "src",
                    AttrValue::String("https://example.invalid/cell.png".into()),
                ),
                ("alt", AttrValue::String("cell image".into())),
            ]),
            content: NodeContent::Atomic,
        });
    for node in children(&f.document, cell) {
        transaction.push_step(TransactionStep::RemoveNode { node: *node });
    }
    f.document = transaction.apply(&f.document).unwrap();
}

#[test]
fn logical_text_replacement_preserves_crossing_cells_and_uses_the_gesture_anchor() {
    let mut f = spanning();
    add_nested_content(&mut f);
    image_only(&mut f, "F");
    image_only(&mut f, "G");
    for (anchor, focus) in [("F", "D"), ("D", "F"), ("J", "D"), ("D", "J"), ("G", "G")] {
        for text in ["中🙂\nnext", ""] {
            for intent in text_replacements(text) {
                let mut session = session(&f);
                session
                    .set_cell_range_selection(f.cells[anchor], f.cells[focus])
                    .unwrap();
                let before_selection = session.selection();
                let selected = before_selection
                    .active_cell_range()
                    .unwrap()
                    .unique_origins(&f.document)
                    .unwrap();
                let notifications = Rc::new(Cell::new(0));
                session.add_listener(Box::new(Notifications(notifications.clone())));
                assert_eq!(
                    session.apply_intent(&intent).unwrap(),
                    SessionOutcome::DocumentChanged
                );
                assert_eq!(
                    session.document().revision(),
                    f.document.revision().next().unwrap()
                );
                assert_eq!(notifications.get(), 1);
                assert_eq!(session.history_depths(), (1, 0));
                assert_eq!(
                    session.document().table_grid(f.table).unwrap(),
                    f.document.table_grid(f.table).unwrap()
                );
                for cell in f.cells.values() {
                    assert_eq!(
                        session.document().node(*cell).unwrap().kind(),
                        f.document.node(*cell).unwrap().kind()
                    );
                    assert_eq!(
                        session.document().node(*cell).unwrap().attrs(),
                        f.document.node(*cell).unwrap().attrs()
                    );
                    if selected.contains(cell) {
                        for id in subtree_ids(&f.document, *cell).into_iter().skip(1) {
                            assert!(session.document().node(id).is_none());
                        }
                        let blocks = children(session.document(), *cell);
                        assert_eq!(blocks.len(), 1);
                        assert_eq!(
                            session.document().node(blocks[0]).unwrap().kind(),
                            &NodeKind::Paragraph
                        );
                        let inline = inline(session.document(), blocks[0]);
                        let expected = if *cell == f.cells[anchor] { text } else { "" };
                        assert_eq!(
                            inline
                                .runs()
                                .iter()
                                .map(|run| run.text().as_str())
                                .collect::<String>(),
                            expected
                        );
                        assert!(inline.atoms().is_empty());
                        assert!(inline.runs().iter().all(|run| run.marks().is_empty()));
                    } else {
                        for id in subtree_ids(&f.document, *cell) {
                            assert_eq!(session.document().node(id), f.document.node(id));
                        }
                    }
                }
                let caret = session.text_selection().unwrap();
                assert!(caret.is_collapsed());
                assert_eq!(
                    caret.focus().node_id(),
                    children(session.document(), f.cells[anchor])[0]
                );
                assert_eq!(caret.focus().offset().as_usize(), text.len());
                assert!(session.selection().active_cell_range().is_none());
                let changed = session.document().clone();
                let after_selection = session.selection();
                session.undo().unwrap();
                assert_eq!(session.document().store(), f.document.store());
                assert_eq!(session.selection(), before_selection);
                session.redo().unwrap();
                assert_eq!(session.document().store(), changed.store());
                assert_eq!(session.selection(), after_selection);
                assert_eq!(notifications.get(), 3);
            }
        }
    }
}

#[test]
fn cell_proxy_composition_commits_once_and_immediate_typing_uses_the_new_anchor_caret() {
    let f = spanning();
    let mut session = session(&f);
    let before = session.selection();
    let notifications = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Notifications(notifications.clone())));
    session
        .apply_intent_with_selection(
            selection(&f, "G", "A"),
            &EditIntent::CommitComposition {
                range: TextRange::empty(TextOffset::ZERO),
                text: "你好🙂".into(),
            },
        )
        .unwrap();
    assert_eq!(
        session.document().revision(),
        f.document.revision().next().unwrap()
    );
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(notifications.get(), 1);
    let committed = session.document().clone();
    let committed_selection = session.selection();
    let anchor_text = children(session.document(), f.cells["G"])[0];
    session
        .apply_intent(&EditIntent::InsertText { text: "!".into() })
        .unwrap();
    assert_eq!(
        inline(session.document(), anchor_text)
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "你好🙂!"
    );
    assert_eq!(session.history_depths(), (2, 0));
    assert_eq!(notifications.get(), 2);
    session.undo().unwrap();
    assert_eq!(session.document().store(), committed.store());
    assert_eq!(session.selection(), committed_selection);
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), committed.store());
    assert_eq!(session.selection(), committed_selection);
}

#[test]
fn unit_and_fully_covered_row_proxy_replacement_accepts_zero_composition() {
    for mut f in [
        fixture(&[&[("A", 1, 1, true)]]),
        fixture(&[&[("A", 2, 2, true)], &[]]),
    ] {
        image_only(&mut f, "A");
        for intent in text_replacements("中🙂") {
            let mut session = session(&f);
            session
                .set_cell_range_selection(f.cells["A"], f.cells["A"])
                .unwrap();
            let before = session.selection();
            session.apply_intent(&intent).unwrap();
            assert_eq!(session.history_depths(), (1, 0));
            assert_eq!(
                session.document().table_grid(f.table).unwrap(),
                f.document.table_grid(f.table).unwrap()
            );
            let paragraph = children(session.document(), f.cells["A"])[0];
            assert_eq!(
                inline(session.document(), paragraph).runs()[0]
                    .text()
                    .as_str(),
                "中🙂"
            );
            assert_eq!(
                session
                    .text_selection()
                    .unwrap()
                    .focus()
                    .offset()
                    .as_usize(),
                "中🙂".len()
            );
            session.undo().unwrap();
            assert_eq!(session.document().store(), f.document.store());
            assert_eq!(session.selection(), before);
        }
    }
}

#[test]
fn cell_proxy_rejects_nonzero_or_nonempty_ime_ranges_for_unit_and_spanning_tables() {
    let offsets = TextBuffer::from("123456789");
    for mut f in [
        spanning(),
        fixture(&[
            &[("A", 1, 1, true), ("B", 1, 1, false)],
            &[("C", 1, 1, false), ("D", 1, 1, true)],
        ]),
    ] {
        let text = f.texts["D"];
        f.document = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::ReplaceText {
                node: text,
                range: TextRange::new(
                    TextOffset::ZERO,
                    inline(&f.document, text).offset_at(1).unwrap(),
                )
                .unwrap(),
                replacement: "中🙂x".into(),
            })
            .apply(&f.document)
            .unwrap();
        add_nested_content(&mut f);
        // Includes a split multibyte boundary, a valid nonzero byte seam,
        // ranges covering the atom/emoji, and a stale out-of-bounds endpoint.
        for (start, end) in [(1, 1), (3, 3), (0, 1), (0, 7), (3, 7), (0, 9)] {
            let mut session = session(&f);
            session
                .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
                .unwrap();
            session
                .apply_intent(&EditIntent::InsertText { text: "x".into() })
                .unwrap();
            let before = session.document().clone();
            let before_selection = session.selection();
            let marks = session.stored_marks().cloned();
            let depths = session.history_depths();
            let input_rule = session.input_rule_undo_available();
            let notifications = Rc::new(Cell::new(0));
            session.add_listener(Box::new(Notifications(notifications.clone())));
            assert_eq!(
                session.apply_intent_with_selection(
                    selection(&f, "D", "A"),
                    &EditIntent::CommitComposition {
                        range: TextRange::new(
                            offsets.offset_at(start).unwrap(),
                            offsets.offset_at(end).unwrap()
                        )
                        .unwrap(),
                        text: "bad".into(),
                    }
                ),
                Err(SessionError::SelectionInvalid)
            );
            assert_eq!(session.document().store(), before.store());
            assert_eq!(session.document().revision(), before.revision());
            assert_eq!(session.selection(), before_selection);
            assert_eq!(session.stored_marks(), marks.as_ref());
            assert_eq!(session.history_depths(), depths);
            assert_eq!(session.input_rule_undo_available(), input_rule);
            assert_eq!(notifications.get(), 0);
            session
                .apply_intent(&EditIntent::InsertText { text: "y".into() })
                .unwrap();
            assert_eq!(session.history_depths(), depths);
            session.undo().unwrap();
            assert_eq!(session.document().store(), f.document.store());
        }
    }
}

#[test]
fn ordinary_inline_composition_retains_its_nonzero_range_contract_inside_a_span() {
    let mut f = spanning();
    let text = f.texts["A"];
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceText {
            node: text,
            range: TextRange::new(
                TextOffset::ZERO,
                inline(&f.document, text).offset_at(1).unwrap(),
            )
            .unwrap(),
            replacement: "中🙂x".into(),
        })
        .apply(&f.document)
        .unwrap();
    let mut session = DocumentSession::new(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(text)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::CommitComposition {
            range: TextRange::new(
                inline(&f.document, text).offset_at(3).unwrap(),
                inline(&f.document, text).offset_at(7).unwrap(),
            )
            .unwrap(),
            text: "文".into(),
        })
        .unwrap();
    assert_eq!(
        inline(session.document(), text)
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "中文x"
    );
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
}

struct ProtectTable(XiaomuDocument, Vec<NodeId>);
impl SessionPolicy for ProtectTable {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if self
            .1
            .iter()
            .any(|id| document.node(*id) != self.0.node(*id))
        {
            return Err(PolicyError::new("table changes denied"));
        }
        Ok(())
    }
}

#[test]
fn rejected_content_and_marks_roll_back_atomic_target_marks_history_and_notifications() {
    for intent in [
        EditIntent::Backspace,
        EditIntent::Delete,
        EditIntent::ToggleMark { mark: Mark::Bold },
        EditIntent::SetMark { mark: Mark::Italic },
        EditIntent::InsertText {
            text: "blocked".into(),
        },
        EditIntent::PasteText {
            text: "blocked".into(),
        },
        EditIntent::CommitComposition {
            range: TextRange::empty(TextOffset::ZERO),
            text: "blocked".into(),
        },
    ] {
        let f = spanning();
        let mut session = DocumentSession::new_with_policy(
            f.document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro)),
            Box::new(ProtectTable(
                f.document.clone(),
                subtree_ids(&f.document, f.table),
            )),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::SetMark { mark: Mark::Italic })
            .unwrap();
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        let before = session.document().clone();
        let before_selection = session.selection();
        let marks = session.stored_marks().cloned();
        let depths = session.history_depths();
        let notifications = Rc::new(Cell::new(0));
        session.add_listener(Box::new(Notifications(notifications.clone())));
        assert!(matches!(
            session.apply_intent_with_selection(selection(&f, "F", "D"), &intent),
            Err(SessionError::Policy(_))
        ));
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), before_selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), depths);
        assert_eq!(notifications.get(), 0);
        // Failed targeting must also restore the open typing group.
        session
            .apply_intent(&EditIntent::InsertText { text: "y".into() })
            .unwrap();
        assert_eq!(session.history_depths(), depths);
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
    }
}

#[test]
fn atomic_clear_undo_restores_the_original_outside_selection() {
    let f = spanning();
    let mut session = session(&f);
    let before = session.selection();
    let target = selection(&f, "F", "D");
    session
        .apply_intent_with_selection(target, &EditIntent::Delete)
        .unwrap();
    assert_eq!(session.selection(), target);
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.selection(), target);
}

#[test]
fn raw_merge_maps_absorbed_range_endpoints_to_survivor_and_undo_restores_exact_range() {
    let f = fixture(&[
        &[("A", 1, 1, true), ("B", 1, 1, false)],
        &[("C", 1, 1, false), ("D", 1, 1, true)],
    ]);
    // Both endpoints may be absorbed, only one may be absorbed, and gesture
    // direction must never choose the survivor instead of logical top-left.
    for (anchor, focus) in [("D", "B"), ("B", "D"), ("A", "D"), ("D", "A")] {
        let mut session = session(&f);
        session
            .set_cell_range_selection(f.cells[anchor], f.cells[focus])
            .unwrap();
        let before_selection = session.selection();
        let notifications = Rc::new(Cell::new(0));
        session.add_listener(Box::new(Notifications(notifications.clone())));
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::MergeTableCells {
                table: f.table,
                rect: TableRect::new(0, 0, 2, 2).unwrap(),
            },
        );
        assert_eq!(
            session.apply(&transaction).unwrap(),
            SessionOutcome::DocumentChanged
        );
        assert_eq!(session.history_depths(), (1, 0));
        assert_eq!(notifications.get(), 1);
        let mapped = session.selection().active_cell_range().unwrap();
        assert_eq!(
            (mapped.anchor(), mapped.focus()),
            (f.cells["A"], f.cells["A"])
        );
        assert_eq!(
            mapped.unique_origins(session.document()).unwrap(),
            [f.cells["A"]]
        );
        assert_eq!(
            mapped.logical_rect(session.document()).unwrap(),
            TableRect::new(0, 0, 2, 2).unwrap()
        );
        let merged = session.document().clone();
        let merged_selection = session.selection();
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
        assert_eq!(session.selection(), before_selection);
        session.redo().unwrap();
        assert_eq!(session.document().store(), merged.store());
        assert_eq!(session.selection(), merged_selection);
        assert_eq!(notifications.get(), 3);
    }
}

#[test]
fn merge_mapping_keeps_an_unaffected_endpoint_and_selection_direction() {
    let f = fixture(&[
        &[("A", 1, 1, true), ("B", 1, 1, false)],
        &[("C", 1, 1, false), ("D", 1, 1, true)],
    ]);
    for (anchor, focus, after_anchor, after_focus) in [("D", "B", "D", "A"), ("B", "D", "A", "D")] {
        let mut session = session(&f);
        session
            .set_cell_range_selection(f.cells[anchor], f.cells[focus])
            .unwrap();
        let before = session.selection();
        session
            .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
                TransactionStep::MergeTableCells {
                    table: f.table,
                    rect: TableRect::new(0, 0, 1, 2).unwrap(),
                },
            ))
            .unwrap();
        let mapped = session.selection().active_cell_range().unwrap();
        assert_eq!(
            (mapped.anchor(), mapped.focus()),
            (f.cells[after_anchor], f.cells[after_focus])
        );
        session.undo().unwrap();
        assert_eq!(session.document().store(), f.document.store());
        assert_eq!(session.selection(), before);
    }
}

#[test]
fn merge_then_remove_table_maps_absorbed_endpoints_to_the_final_structural_seam() {
    let f = fixture(&[
        &[("A", 1, 1, true), ("B", 1, 1, false)],
        &[("C", 1, 1, false), ("D", 1, 1, true)],
    ]);
    let mut session = session(&f);
    session
        .set_cell_range_selection(f.cells["D"], f.cells["B"])
        .unwrap();
    let before = session.selection();
    session
        .apply(
            &Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::MergeTableCells {
                    table: f.table,
                    rect: TableRect::new(0, 0, 2, 2).unwrap(),
                })
                .with_step(TransactionStep::RemoveNode { node: f.table })
                .with_step(TransactionStep::InsertNode {
                    parent: f.document.root(),
                    index: 0,
                    kind: NodeKind::Paragraph,
                    attrs: NodeAttrs::empty(),
                    content: NodeContent::Inline(InlineContent::empty()),
                }),
        )
        .unwrap();
    assert_eq!(
        session.selection(),
        DocumentSelection::collapsed(NodeGap::new(f.document.root(), 2))
    );
    assert_eq!(session.history_depths(), (1, 0));
    let changed = session.document().clone();
    let after = session.selection();
    session.undo().unwrap();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.selection(), before);
    session.redo().unwrap();
    assert_eq!(session.document().store(), changed.store());
    assert_eq!(session.selection(), after);
}
