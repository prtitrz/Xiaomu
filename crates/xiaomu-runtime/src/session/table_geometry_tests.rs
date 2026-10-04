//! Span foundation regressions: retain data and fail closed before legacy edits.

use std::{cell::Cell, rc::Rc};

use serde_json::{Value, json};
use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

use super::*;
use crate::clipboard::{
    ClipboardNode, ClipboardNodeContent, ClipboardSlice, decode_metadata, encode_metadata,
};

fn attrs(values: &[(&str, AttrValue)]) -> NodeAttrs {
    NodeAttrs::new(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone()))
            .collect(),
    )
    .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    cells: Vec<NodeId>,
    texts: Vec<NodeId>,
}

fn fixture(spans: bool, header: bool) -> Fixture {
    let mut b = NodeStoreBuilder::new();
    let intro = paragraph(&mut b, "intro");
    let mut cells = Vec::new();
    let mut texts = Vec::new();
    let mut rows = Vec::new();
    let shape: &[&[&str]] = if spans {
        &[&["A", "B"], &["C"], &["D", "E", "F"]]
    } else {
        &[&["A", "B"], &["C", "D"]]
    };
    for (r, values) in shape.iter().enumerate() {
        let mut row = Vec::new();
        for (c, text) in values.iter().enumerate() {
            let leaf = paragraph(&mut b, text);
            let kind = if header && r == 0 && c == 0 {
                NodeKind::TableHeader
            } else {
                NodeKind::TableCell
            };
            let cell_attrs = if spans && r == 0 && c == 0 {
                attrs(&[
                    ("rowspan", AttrValue::Integer(2)),
                    ("colspan", AttrValue::Integer(2)),
                    (
                        "colwidth",
                        AttrValue::List(vec![AttrValue::Integer(0), AttrValue::Integer(120)]),
                    ),
                    ("backgroundColor", AttrValue::String("unparsed(red)".into())),
                    (
                        "extension",
                        AttrValue::Object([("null".into(), AttrValue::Null)].into()),
                    ),
                ])
            } else {
                NodeAttrs::empty()
            };
            let cell = b
                .insert(kind, cell_attrs, NodeContent::children([leaf]))
                .unwrap();
            texts.push(leaf);
            cells.push(cell);
            row.push(cell);
        }
        rows.push(
            b.insert(
                NodeKind::TableRow,
                attrs(&[("rowTag", AttrValue::Integer(r as i64))]),
                NodeContent::children(row),
            )
            .unwrap(),
        );
    }
    let table = b
        .insert(
            NodeKind::Table,
            attrs(&[("tableTag", AttrValue::String("exact".into()))]),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, b.finish()).unwrap(),
        intro,
        table,
        cells,
        texts,
    }
}

fn paragraph(b: &mut NodeStoreBuilder, text: &str) -> NodeId {
    b.insert(
        NodeKind::Paragraph,
        NodeAttrs::empty(),
        NodeContent::Inline(
            InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
        ),
    )
    .unwrap()
}

fn session_at(f: &Fixture, node: NodeId) -> DocumentSession {
    DocumentSession::new(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
    )
    .unwrap()
}

fn closed_table(f: &Fixture) -> ClipboardSlice {
    let selection = DocumentSelection::node(&f.document, f.table).unwrap();
    DocumentSession::new(f.document.clone(), selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn open_table(f: &Fixture) -> ClipboardSlice {
    ClipboardSlice::from_table(closed_table(f).roots()[0].clone()).unwrap()
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

fn rejected_unchanged(session: &mut DocumentSession, intent: &EditIntent) {
    let document = session.document().clone();
    let selection = session.selection();
    let marks = session.stored_marks().cloned();
    let depths = session.history_depths();
    let group = session.history.typing_group_open();
    let input_rule = session.input_rule_undo_available();
    let notifications = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Notifications(notifications.clone())));
    assert_eq!(
        session.apply_intent(intent),
        Err(SessionError::UnsupportedTableOperation)
    );
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), marks.as_ref());
    assert_eq!(session.history_depths(), depths);
    assert_eq!(session.history.typing_group_open(), group);
    assert_eq!(session.input_rule_undo_available(), input_rule);
    assert_eq!(notifications.get(), 0);
}

#[test]
fn merged_tables_reject_every_legacy_row_column_and_cell_navigation_path() {
    for header in [false, true] {
        let f = fixture(true, header);
        let mut session = session_at(&f, f.texts[0]);
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        session.stored_marks = Some(MarkSet::new([Mark::Bold]).unwrap());
        for intent in [
            EditIntent::InsertTableRow {
                table: f.table,
                index: 1,
            },
            EditIntent::InsertTableColumn {
                table: f.table,
                index: 1,
            },
            // Removing this last physical row could leave a valid merged table.
            // A final document validator alone is not a semantic guard.
            EditIntent::DeleteTableRow {
                table: f.table,
                index: 2,
            },
            EditIntent::DeleteTableColumn {
                table: f.table,
                index: 1,
            },
            EditIntent::MoveToNextCell,
            EditIntent::MoveToPreviousCell,
        ] {
            rejected_unchanged(&mut session, &intent);
        }
        // The terminal Tab append route must also reject before changing history.
        session
            .set_inline_selection(
                InlinePoint::at_start_of(*f.texts.last().unwrap()),
                InlinePoint::at_start_of(*f.texts.last().unwrap()),
            )
            .unwrap();
        rejected_unchanged(&mut session, &EditIntent::MoveToNextCell);
    }
}

#[test]
fn merged_range_keeps_legacy_matrix_clipboard_and_physical_navigation_guards() {
    let f = fixture(true, true);
    let unit = fixture(false, false);
    let plain = ClipboardSlice::from_roots(vec![
        closed_table(&unit).roots()[0].content().as_table().unwrap()[0][0]
            .content()
            .as_children()
            .unwrap()[0]
            .clone(),
    ]);
    let mut session = session_at(&f, f.texts[0]);
    session
        .set_cell_range_selection(f.cells[0], *f.cells.last().unwrap())
        .unwrap();
    let range = session.selection().active_cell_range().unwrap();
    assert_eq!(
        range.cells(session.document()),
        Err(SessionError::UnsupportedTableOperation)
    );
    assert_eq!(
        session.clipboard_slice(),
        Err(SessionError::UnsupportedTableOperation)
    );
    for intent in [
        EditIntent::PasteSlice { slice: plain },
        EditIntent::PasteSlice {
            slice: open_table(&unit),
        },
        EditIntent::MoveToNextCell,
        EditIntent::MoveToPreviousCell,
    ] {
        rejected_unchanged(&mut session, &intent);
    }
}

#[test]
fn inline_typing_marks_and_exact_undo_remain_legal_in_header_and_spanning_body_cells() {
    for spans in [false, true] {
        for header in [false, true] {
            let f = fixture(spans, header);
            let mut session = session_at(&f, f.texts[0]);
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                .unwrap();
            session
                .apply_intent(&EditIntent::InsertText {
                    text: "typed".into(),
                })
                .unwrap();
            assert!(
                session
                    .document()
                    .node(f.texts[0])
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .runs()[0]
                    .marks()
                    .contains(xiaomu_core::document::MarkKind::Bold)
            );
            let changed = session.document().clone();
            session.undo().unwrap();
            assert_eq!(session.document().store(), f.document.store());
            session.redo().unwrap();
            assert_eq!(session.document().store(), changed.store());
            assert_eq!(
                session.document().node(f.cells[0]).unwrap().kind(),
                f.document.node(f.cells[0]).unwrap().kind()
            );
        }
    }
}

#[test]
fn v13_closed_copy_preserves_header_span_raw_attrs_rows_and_full_subtree() {
    let mut f = fixture(true, true);
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::at_start_of(f.texts[0]),
            kind: xiaomu_core::document::AtomKind::hard_break(),
            attrs: NodeAttrs::empty(),
            content: xiaomu_core::document::InlineAtomContent::hard_break()
                .with_marks(MarkSet::new([Mark::Bold]).unwrap()),
        })
        .with_step(TransactionStep::InsertNode {
            parent: f.cells[0],
            index: 1,
            kind: NodeKind::Image,
            attrs: attrs(&[(
                "src",
                AttrValue::String("https://example.invalid/header.png".into()),
            )]),
            content: NodeContent::Atomic,
        })
        .apply(&f.document)
        .unwrap();
    let slice = closed_table(&f);
    let encoded = encode_metadata(&slice).unwrap();
    let wire: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wire["version"], 13);
    assert_eq!(wire["closed"], true);
    assert_eq!(
        decode_metadata(slice.plain_text(), &encoded),
        Some(slice.clone())
    );
    let table = &slice.roots()[0];
    let rows = table.content().as_table().unwrap();
    assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [2, 1, 3]);
    assert_eq!(rows[0][0].kind(), &NodeKind::TableHeader);
    let blocks = rows[0][0].content().as_children().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[1].kind(), &NodeKind::Image);
    let atom = &blocks[0].content().as_inline().unwrap().atoms()[0];
    assert!(atom.kind().is_hard_break());
    assert!(
        atom.content()
            .marks()
            .contains(xiaomu_core::document::MarkKind::Bold)
    );
    assert_eq!(
        rows[0][0].attrs(),
        f.document.node(f.cells[0]).unwrap().attrs()
    );
    let ClipboardNodeContent::Table { row_attrs, .. } = table.content() else {
        panic!()
    };
    assert_eq!(row_attrs.len(), 3);
    assert_eq!(row_attrs[2].get("rowTag"), Some(&AttrValue::Integer(2)));
    for version in 4..=12 {
        let mut old = wire.clone();
        old["version"] = json!(version);
        if version < 11 {
            old.as_object_mut().unwrap().remove("closed");
        }
        assert!(decode_metadata(slice.plain_text(), &old.to_string()).is_none());
    }
}

#[test]
fn spanning_tsv_uses_logical_slots_with_blank_covered_cells() {
    let f = fixture(true, true);
    let slice = open_table(&f);
    assert_eq!(slice.plain_text(), "A\t\tB\n\t\tC\nD\tE\tF");
    let encoded = encode_metadata(&slice).unwrap();
    assert_eq!(
        decode_metadata(slice.plain_text(), &encoded),
        Some(slice.clone())
    );
    assert!(decode_metadata("A\tB\nC\nD\tE\tF", &encoded).is_none());
    let mut wire: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(wire["closed"], false);
    wire.as_object_mut().unwrap().remove("closed");
    assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
    wire["closed"] = Value::Null;
    assert!(decode_metadata(slice.plain_text(), &wire.to_string()).is_none());
}

#[test]
fn geometric_attr_presence_bumps_v13_without_filling_missing_defaults() {
    for attr in [
        attrs(&[("colspan", AttrValue::Integer(1))]),
        attrs(&[("rowspan", AttrValue::Integer(1))]),
        attrs(&[("colwidth", AttrValue::Null)]),
        attrs(&[("colwidth", AttrValue::List(vec![AttrValue::Integer(0)]))]),
    ] {
        let mut f = fixture(false, false);
        f.document = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::SetNodeAttrs {
                node: f.cells[0],
                attrs: attr.clone(),
            })
            .apply(&f.document)
            .unwrap();
        let slice = open_table(&f);
        let wire: Value = serde_json::from_str(&encode_metadata(&slice).unwrap()).unwrap();
        assert_eq!(wire["version"], 13);
        let decoded = decode_metadata(slice.plain_text(), &wire.to_string()).unwrap();
        let cell = &decoded.roots()[0].content().as_table().unwrap()[0][0];
        assert_eq!(cell.attrs(), &attr);
        for version in 5..=12 {
            let mut old = wire.clone();
            old["version"] = json!(version);
            if version < 11 {
                old.as_object_mut().unwrap().remove("closed");
            }
            if version == 11 {
                old["closed"] = json!(true);
            }
            assert!(decode_metadata(slice.plain_text(), &old.to_string()).is_none());
        }
    }
    // Unrelated attrs and ordinary unit tables retain the historical envelope.
    let f = fixture(false, false);
    assert_eq!(
        serde_json::from_str::<Value>(&encode_metadata(&open_table(&f)).unwrap()).unwrap()["version"],
        6
    );
    assert_eq!(
        serde_json::from_str::<Value>(&encode_metadata(&closed_table(&f)).unwrap()).unwrap()["version"],
        11
    );
}

#[test]
fn internal_spanning_slices_reject_before_any_generic_fitter_including_nested_children() {
    let source = fixture(true, true);
    let destination = fixture(false, false);
    let table = closed_table(&source).roots()[0].clone();
    let ClipboardNodeContent::Table { rows, row_attrs } = table.content() else {
        panic!()
    };
    let canonical_children = ClipboardNode::new(
        NodeKind::Table,
        table.attrs().clone(),
        ClipboardNodeContent::Children(
            rows.iter()
                .enumerate()
                .map(|(r, cells)| {
                    ClipboardNode::new(
                        NodeKind::TableRow,
                        row_attrs[r].clone(),
                        ClipboardNodeContent::Children(cells.clone()),
                    )
                })
                .collect(),
        ),
    );
    let nested = ClipboardSlice::from_roots(vec![ClipboardNode::new(
        NodeKind::Quote,
        NodeAttrs::empty(),
        ClipboardNodeContent::Children(vec![canonical_children]),
    )]);
    for slice in [closed_table(&source), open_table(&source), nested] {
        let mut session = session_at(&destination, destination.intro);
        session
            .apply_intent(&EditIntent::InsertText { text: "x".into() })
            .unwrap();
        rejected_unchanged(&mut session, &EditIntent::PasteSlice { slice });
    }
}

#[test]
fn unit_headers_navigate_and_rich_paste_preserves_header_kinds() {
    let source = fixture(false, true);
    let mut session = session_at(&source, source.texts[0]);
    session.apply_intent(&EditIntent::MoveToNextCell).unwrap();
    assert_eq!(
        session.selection().focus(),
        InlinePoint::at_start_of(source.texts[1]).into()
    );
    session
        .apply_intent(&EditIntent::MoveToPreviousCell)
        .unwrap();
    assert_eq!(
        session.selection().focus(),
        InlinePoint::at_start_of(source.texts[0]).into()
    );
    session
        .set_cell_range_selection(source.cells[0], source.cells[3])
        .unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "A\tB\nC\tD");
    assert_eq!(
        serde_json::from_str::<Value>(&encode_metadata(&slice).unwrap()).unwrap()["version"],
        13
    );
    let target = fixture(false, false);
    let mut pasted = session_at(&target, target.intro);
    pasted
        .apply_intent(&EditIntent::PasteSlice {
            slice: slice.clone(),
        })
        .unwrap();
    let copied = DocumentSession::new(
        pasted.document().clone(),
        DocumentSelection::all(pasted.document()),
    )
    .unwrap()
    .clipboard_slice()
    .unwrap()
    .unwrap();
    let inserted = &copied.roots()[1];
    assert_eq!(inserted, &slice.roots()[0]);
    pasted.undo().unwrap();
    assert_eq!(pasted.document().store(), target.document.store());
    pasted.redo().unwrap();
    let mut filled = session_at(&target, target.texts[0]);
    filled
        .set_cell_range_selection(target.cells[0], target.cells[3])
        .unwrap();
    filled
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    assert_eq!(
        filled.document().node(target.cells[0]).unwrap().kind(),
        &NodeKind::TableHeader
    );
    filled.undo().unwrap();
    assert_eq!(filled.document().store(), target.document.store());
}

#[test]
fn unit_header_payload_cannot_silently_enter_body_cell_content_only_route() {
    let source = fixture(false, true);
    let mut s = session_at(&source, source.texts[0]);
    s.set_cell_range_selection(source.cells[0], source.cells[0])
        .unwrap();
    let slice = s.clipboard_slice().unwrap().unwrap();
    let target = fixture(false, false);
    let mut destination = session_at(&target, target.texts[0]);
    rejected_unchanged(&mut destination, &EditIntent::PasteSlice { slice });
}

#[test]
fn header_kind_fits_existing_input_rule_payload_budget() {
    let f = fixture(false, true);
    let transaction =
        Transaction::new(TransactionOrigin::UserInput).with_step(TransactionStep::SetNodeKind {
            node: f.cells[0],
            kind: NodeKind::TableHeader,
        });
    assert!(
        InputRuleUndoSpec::new(
            transaction,
            DocumentSelection::collapsed(InlinePoint::at_start_of(f.texts[0]))
        )
        .is_ok()
    );
}

#[test]
fn fully_covered_physical_row_projects_as_blank_logical_row() {
    let f = fixture(true, true);
    let root = closed_table(&f).roots()[0].clone();
    let origin = root.content().as_table().unwrap()[0][0].clone();
    let table = ClipboardNode::new(
        NodeKind::Table,
        NodeAttrs::empty(),
        ClipboardNodeContent::Table {
            rows: vec![vec![origin], vec![]],
            row_attrs: vec![],
        },
    );
    let slice = ClipboardSlice::from_table(table).unwrap();
    assert_eq!(slice.plain_text(), "A\t\n\t");
    let metadata = encode_metadata(&slice).unwrap();
    assert_eq!(decode_metadata(slice.plain_text(), &metadata), Some(slice));
}

#[test]
fn v13_decode_rejects_invalid_spans_and_widths_instead_of_repairing_raw_attrs() {
    let f = fixture(true, true);
    let slice = open_table(&f);
    let wire: Value = serde_json::from_str(&encode_metadata(&slice).unwrap()).unwrap();
    for (key, value) in [
        ("colspan", json!({"type": "null"})),
        ("rowspan", json!({"type": "integer", "value": 0})),
        ("rowspan", json!({"type": "integer", "value": -1})),
        ("colspan", json!({"type": "string", "value": "2"})),
        (
            "colwidth",
            json!({"type": "list", "value": [{"type": "integer", "value": 20}]}),
        ),
        (
            "colwidth",
            json!({"type": "list", "value": [{"type": "integer", "value": -1}, {"type": "integer", "value": 20}]}),
        ),
        ("rowspan", json!({"type": "integer", "value": 1_000_001})),
    ] {
        let mut invalid = wire.clone();
        invalid["roots"][0]["content"]["value"]["rows"][0][0]["attrs"][key] = value;
        assert!(
            decode_metadata(slice.plain_text(), &invalid.to_string()).is_none(),
            "{key}"
        );
    }
}

#[test]
fn pre_v13_unit_table_wire_versions_remain_decodable() {
    let f = fixture(false, false);
    let table = closed_table(&f).roots()[0].clone();
    let slice = ClipboardSlice::from_table(ClipboardNode::new(
        NodeKind::Table,
        table.attrs().clone(),
        ClipboardNodeContent::Table {
            rows: table.content().as_table().unwrap().clone(),
            row_attrs: vec![],
        },
    ))
    .unwrap();
    let wire: Value = serde_json::from_str(&encode_metadata(&slice).unwrap()).unwrap();
    assert_eq!(wire["version"], 5);
    for version in 5..=12 {
        let mut historical = wire.clone();
        historical["version"] = json!(version);
        let expected = if version == 11 {
            historical["closed"] = json!(true);
            ClipboardSlice::from_closed_roots(slice.roots().to_vec())
        } else {
            if version == 12 {
                historical["closed"] = json!(false);
            }
            slice.clone()
        };
        assert_eq!(
            decode_metadata(expected.plain_text(), &historical.to_string()),
            Some(expected)
        );
    }
}

#[test]
fn header_range_endpoints_must_belong_to_the_same_table() {
    let mut f = fixture(false, true);
    f.document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertTable {
            parent: f.document.root(),
            index: 0,
            rows: 1,
            columns: 1,
        })
        .apply(&f.document)
        .unwrap();
    let other_table = f
        .document
        .node(f.document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let other_cell = f
        .document
        .table_grid(other_table)
        .unwrap()
        .slot(0, 0)
        .unwrap();
    let mut session = session_at(&f, f.texts[0]);
    let selection = session.selection();
    assert_eq!(
        session.set_cell_range_selection(f.cells[0], other_cell),
        Err(SessionError::SelectionInvalid)
    );
    assert_eq!(session.selection(), selection);
}
