//! Real clipboard actions preserve purpose, export descriptors and source state.

use crate::block_view::{ClipboardCopy, ClipboardCut, SharedSession};
use crate::document_view::DocumentView;
use crate::editor::{EditorHooks, EditorInstance};
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};
use gpui::{AppContext as _, TestAppContext, WindowHandle};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::document::{
    AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::clipboard::{
    ClipboardCellRangeRoot, ClipboardExportPurpose, ClipboardExportSpec, ClipboardMetadataDecode,
    ClipboardNodeContent, ClipboardSlice, ClipboardSourceBoundary, ClipboardTextProjection,
    decode_metadata_checked, encode_metadata,
};
use xiaomu_runtime::session::{
    DocumentSelection, EditIntent, IntentDisposition, PolicyError, SessionContext, SessionPolicy,
};

#[derive(Default)]
struct Seen {
    exports: Vec<ClipboardExportPurpose>,
    deletes: usize,
    cut_preparations: usize,
    cut_candidates: usize,
    cut_candidate_writes: Vec<usize>,
}

struct ExportPolicy {
    seen: Rc<RefCell<Seen>>,
    enabled: bool,
}

impl SessionPolicy for ExportPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.seen.borrow_mut().exports.push(purpose);
        Ok(self.enabled.then(|| {
            ClipboardExportSpec::new()
                .with_closed_cell_ranges()
                .with_text_projection(ClipboardTextProjection::TextBetweenLfV1)
        }))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Delete) {
            self.seen.borrow_mut().deletes += 1;
        }
        Ok(IntentDisposition::Continue)
    }
}

fn attrs(values: impl IntoIterator<Item = (&'static str, AttrValue)>) -> NodeAttrs {
    NodeAttrs::new(values.into_iter().map(|(k, v)| (k.into(), v)).collect()).unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()
                ])
                .unwrap(),
            ),
        )
        .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    table: NodeId,
    cells: Vec<NodeId>,
}

fn fixture(rich: bool, attr_depth: usize) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let mut payload = AttrValue::String("exact".into());
    for _ in 0..attr_depth {
        payload = AttrValue::List(vec![payload]);
    }
    let mut cells = Vec::new();
    let mut rows = Vec::new();
    // A rich 2x3 grid has one 2x2 Header followed by two ordinary cells.
    // A unit grid keeps Cut's old deletion path available as a regression.
    for (row_index, values) in [vec!["甲", "乙"], vec!["丙"]].into_iter().enumerate() {
        let mut row = Vec::new();
        for (column_index, text) in values.into_iter().enumerate() {
            let leaf = paragraph(&mut builder, text);
            let children = if rich && row_index == 0 && column_index == 0 {
                let extra = paragraph(&mut builder, "内");
                let quote = builder
                    .insert(
                        NodeKind::Quote,
                        attrs([("quote-data", AttrValue::Null)]),
                        NodeContent::children([leaf, extra]),
                    )
                    .unwrap();
                vec![quote]
            } else {
                vec![leaf]
            };
            let (kind, cell_attrs) = if rich && row_index == 0 && column_index == 0 {
                (
                    NodeKind::TableHeader,
                    attrs([
                        ("rowspan", AttrValue::Integer(2)),
                        ("colspan", AttrValue::Integer(2)),
                        (
                            "colwidth",
                            AttrValue::List(vec![AttrValue::Integer(80), AttrValue::Integer(90)]),
                        ),
                        ("extension", payload.clone()),
                    ]),
                )
            } else {
                (NodeKind::TableCell, attrs([("extension", payload.clone())]))
            };
            let cell = builder
                .insert(kind, cell_attrs, NodeContent::children(children))
                .unwrap();
            cells.push(cell);
            row.push(cell);
        }
        // Fill the second row for a rectangular legacy unit-cell fixture.
        if !rich && row_index == 1 {
            let leaf = paragraph(&mut builder, "丁");
            let cell = builder
                .insert(
                    NodeKind::TableCell,
                    attrs([("extension", payload.clone())]),
                    NodeContent::children([leaf]),
                )
                .unwrap();
            row.push(cell);
            cells.push(cell);
        }
        rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs([("row-data", AttrValue::Integer(row_index as i64))]),
                    NodeContent::children(row),
                )
                .unwrap(),
        );
    }
    let table = builder
        .insert(
            NodeKind::Table,
            attrs([("table-data", AttrValue::Null)]),
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
    }
}

struct Mounted {
    window: WindowHandle<DocumentView>,
    session: SharedSession,
    seen: Rc<RefCell<Seen>>,
}

fn mount(cx: &mut TestAppContext, fixture: &Fixture, enabled: bool, whole: bool) -> Mounted {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let editor = EditorInstance::new_with_policy(
        fixture.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(fixture.intro)),
        EditorHooks::default(),
        Box::new(ExportPolicy {
            seen: seen.clone(),
            enabled,
        }),
    )
    .unwrap();
    let session = editor.session().clone();
    // Keep a redo entry so rejected actions cannot silently discard history.
    {
        let mut session = session.borrow_mut();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".into(),
            })
            .unwrap();
        session.undo().unwrap();
        if whole {
            let all = DocumentSelection::all(session.document());
            session.set_document_selection(all).unwrap();
        } else {
            session
                .set_cell_range_selection(fixture.cells[0], *fixture.cells.last().unwrap())
                .unwrap();
        }
    }
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Mounted {
        window,
        session,
        seen,
    }
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
}

impl Snapshot {
    fn capture(m: &Mounted) -> Self {
        let session = m.session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
        }
    }

    fn assert_unchanged(&self, m: &Mounted) {
        let session = m.session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
    }
}

fn action(m: &Mounted, purpose: ClipboardExportPurpose, cx: &mut TestAppContext) {
    m.window
        .update(cx, |view, window, cx| match purpose {
            ClipboardExportPurpose::Copy => view.copy(&ClipboardCopy, window, cx),
            ClipboardExportPurpose::Cut => view.cut(&ClipboardCut, window, cx),
        })
        .unwrap();
}

fn clipboard(cx: &mut TestAppContext) -> gpui::ClipboardItem {
    cx.update(|cx| cx.read_from_clipboard().unwrap())
}

fn install_previous(cx: &mut TestAppContext) -> gpui::ClipboardItem {
    let previous = gpui::ClipboardItem::new_string_with_metadata(
        "previous\r\ntext".into(),
        "previous structured metadata".into(),
    );
    cx.update(|cx| cx.write_to_clipboard(previous.clone()));
    previous
}

fn decoded(item: &gpui::ClipboardItem) -> ClipboardSlice {
    match decode_metadata_checked(&item.text().unwrap(), item.metadata().unwrap()) {
        ClipboardMetadataDecode::Valid(slice) => slice,
        _ => panic!("clipboard must retain its complete native descriptor"),
    }
}

#[gpui::test]
fn projected_rich_cell_and_whole_copy_use_lossless_v14_actions(cx: &mut TestAppContext) {
    for whole in [false, true] {
        let fixture = fixture(true, 2);
        let m = mount(cx, &fixture, true, whole);
        let before = Snapshot::capture(&m);
        let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
        assert!(expected.requires_lossless_transport());
        assert_eq!(
            expected.source_boundary(),
            Some(if whole {
                ClipboardSourceBoundary::WholeRoots
            } else {
                ClipboardSourceBoundary::CellRange {
                    root_form: ClipboardCellRangeRoot::Table,
                }
            })
        );
        assert_eq!(
            expected.text_projection(),
            Some(ClipboardTextProjection::TextBetweenLfV1)
        );
        m.seen.borrow_mut().exports.clear();
        action(&m, ClipboardExportPurpose::Copy, cx);
        let item = clipboard(cx);
        assert!(
            item.metadata()
                .unwrap()
                .starts_with("xiaomu.clipboard.v14\n")
        );
        assert_eq!(decoded(&item), expected);
        cx.update(|cx| {
            let Some(PlatformClipboardContent::Structured(actual)) =
                PlatformClipboard::new(cx).read_content()
            else {
                panic!("native platform read must retain projected content");
            };
            assert_eq!(actual, expected);
        });
        assert_eq!(
            item.text().as_deref(),
            Some(if whole {
                "intro\n甲\n内\n乙\n丙"
            } else {
                "甲\n内\n乙\n丙"
            })
        );
        assert_eq!(m.seen.borrow().exports, [ClipboardExportPurpose::Copy]);
        assert_eq!(m.seen.borrow().deletes, 0);
        let copied_table = &expected.roots()[usize::from(whole)];
        assert_eq!(
            copied_table.attrs(),
            fixture.document.node(fixture.table).unwrap().attrs()
        );
        let ClipboardNodeContent::Table { rows, row_attrs } = copied_table.content() else {
            panic!("table structure lost");
        };
        assert_eq!(rows.iter().map(Vec::len).collect::<Vec<_>>(), [2, 1]);
        assert_eq!(row_attrs.len(), 2);
        assert_eq!(rows[0][0].kind(), &NodeKind::TableHeader);
        assert_eq!(
            rows[0][0].attrs(),
            fixture.document.node(fixture.cells[0]).unwrap().attrs()
        );
        before.assert_unchanged(&m);
    }
}

#[gpui::test]
fn closed_cell_cut_is_rejected_before_clipboard_and_delete_policy(cx: &mut TestAppContext) {
    // Unit cells are important: a Delete policy would otherwise be legal.
    for rich in [false, true] {
        let fixture = fixture(rich, 1);
        let m = mount(cx, &fixture, true, false);
        let before = Snapshot::capture(&m);
        let previous = install_previous(cx);
        if !rich {
            m.window
                .update(cx, |view, _, _| {
                    assert!(!view.selection_has_hidden_table_endpoint())
                })
                .unwrap();
        }
        action(&m, ClipboardExportPurpose::Cut, cx);
        assert_eq!(clipboard(cx), previous);
        if rich {
            // The unmeasured spanning-table endpoint is now rejected before
            // any Cut projection, dedicated policy or clipboard operation.
            assert!(m.seen.borrow().exports.is_empty());
        } else {
            assert_eq!(m.seen.borrow().exports, [ClipboardExportPurpose::Cut]);
        }
        assert_eq!(m.seen.borrow().deletes, 0);
        before.assert_unchanged(&m);
    }
}

#[gpui::test]
fn partial_cell_copy_retains_rows_root_descriptor(cx: &mut TestAppContext) {
    let fixture = fixture(false, 1);
    let m = mount(cx, &fixture, true, false);
    m.session
        .borrow_mut()
        .set_cell_range_selection(fixture.cells[1], fixture.cells[3])
        .unwrap();
    let before = Snapshot::capture(&m);
    action(&m, ClipboardExportPurpose::Copy, cx);
    let copied = decoded(&clipboard(cx));
    assert_eq!(copied.plain_text(), "乙\n丁");
    assert_eq!(
        copied.source_boundary(),
        Some(ClipboardSourceBoundary::CellRange {
            root_form: ClipboardCellRangeRoot::Rows,
        })
    );
    assert_eq!(
        copied.text_projection(),
        Some(ClipboardTextProjection::TextBetweenLfV1)
    );
    assert!(copied.requires_lossless_transport());
    before.assert_unchanged(&m);
}

#[gpui::test]
fn projected_copy_budget_failure_preserves_clipboard_and_source(cx: &mut TestAppContext) {
    for whole in [false, true] {
        let fixture = fixture(false, 160);
        let m = mount(cx, &fixture, true, whole);
        assert!(m.session.borrow().clipboard_slice().is_err());
        let previous = install_previous(cx);
        let before = Snapshot::capture(&m);
        action(&m, ClipboardExportPurpose::Copy, cx);
        assert_eq!(clipboard(cx), previous);
        before.assert_unchanged(&m);
        assert_eq!(m.seen.borrow().deletes, 0);
    }
}

#[gpui::test]
fn legacy_open_copy_retains_plain_text_fallback_when_encode_fails(cx: &mut TestAppContext) {
    let fixture = fixture(false, 160);
    let m = mount(cx, &fixture, false, false);
    let expected = m.session.borrow().clipboard_slice().unwrap().unwrap();
    assert!(!expected.requires_lossless_transport());
    assert!(encode_metadata(&expected).is_err());
    let before = Snapshot::capture(&m);
    install_previous(cx);
    action(&m, ClipboardExportPurpose::Copy, cx);
    let item = clipboard(cx);
    assert_eq!(item.text().as_deref(), Some(expected.plain_text()));
    assert!(item.metadata().is_none());
    before.assert_unchanged(&m);
}

#[path = "clipboard_cut_prepared_tests.rs"]
mod prepared_cut_tests;
