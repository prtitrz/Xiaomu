//! Native CellRange carriers must not enter the default whole-table fitter.

use crate::block_view::{ClipboardCopy, ClipboardPaste, Redo, SharedSession};
use crate::document_view::DocumentView;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use gpui::{AppContext as _, TestAppContext, WindowHandle};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::clipboard::{
    ClipboardCellRangeRoot, ClipboardExportPurpose, ClipboardExportSpec, ClipboardMetadataDecode,
    ClipboardSourceBoundary, ClipboardTextProjection, decode_metadata_checked, encode_metadata,
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, EditIntent, PolicyError, SessionContext,
    SessionPolicy,
};

struct ExportPolicy(Rc<Cell<usize>>);

impl SessionPolicy for ExportPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        assert_eq!(purpose, ClipboardExportPurpose::Copy);
        self.0.set(self.0.get() + 1);
        Ok(Some(
            ClipboardExportSpec::new()
                .with_closed_cell_ranges()
                .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
        ))
    }
}

type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

struct Fixture {
    document: XiaomuDocument,
    intro: NodeId,
    cells: Vec<NodeId>,
}

fn inline(builder: &mut NodeStoreBuilder, kind: NodeKind, text: &str) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn fixture(kind: NodeKind, with_table: bool, label: &str) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let intro = inline(&mut builder, kind, label);
    let mut roots = vec![intro];
    let mut cells = Vec::new();
    if with_table {
        let mut rows = Vec::new();
        for row in 0..2 {
            let mut row_cells = Vec::new();
            for column in 0..2 {
                let leaf = inline(
                    &mut builder,
                    NodeKind::Paragraph,
                    &format!("{label}-{row}-{column}"),
                );
                let cell = builder
                    .insert(
                        NodeKind::TableCell,
                        NodeAttrs::empty(),
                        NodeContent::children([leaf]),
                    )
                    .unwrap();
                row_cells.push(cell);
                cells.push(cell);
            }
            rows.push(
                builder
                    .insert(
                        NodeKind::TableRow,
                        NodeAttrs::empty(),
                        NodeContent::children(row_cells),
                    )
                    .unwrap(),
            );
        }
        roots.push(
            builder
                .insert(
                    NodeKind::Table,
                    NodeAttrs::empty(),
                    NodeContent::children(rows),
                )
                .unwrap(),
        );
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(roots),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        cells,
    }
}

fn mount(cx: &mut TestAppContext, editor: &EditorInstance) -> WindowHandle<DocumentView> {
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
    window
}

fn start_cell(root_form: ClipboardCellRangeRoot) -> usize {
    match root_form {
        ClipboardCellRangeRoot::Rows => 1,
        ClipboardCellRangeRoot::Table => 0,
    }
}

fn copied_carrier(
    cx: &mut TestAppContext,
    root_form: ClipboardCellRangeRoot,
) -> gpui::ClipboardItem {
    // All cells have unit geometry so legacy generic table fitting can succeed.
    let source = fixture(NodeKind::Paragraph, true, "source");
    let exports = Rc::new(Cell::new(0));
    let editor = EditorInstance::new_with_policy(
        source.document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(source.intro)),
        EditorHooks::default(),
        Box::new(ExportPolicy(exports.clone())),
    )
    .unwrap();
    editor
        .session()
        .borrow_mut()
        .set_cell_range_selection(source.cells[start_cell(root_form)], source.cells[3])
        .unwrap();
    let window = mount(cx, &editor);
    window
        .update(cx, |view, window, cx| view.copy(&ClipboardCopy, window, cx))
        .unwrap();
    assert_eq!(exports.get(), 1);
    let item = cx.update(|cx| cx.read_from_clipboard().unwrap());
    let metadata = item.metadata().unwrap();
    assert!(metadata.starts_with("xiaomu.clipboard.v14\n"));
    let ClipboardMetadataDecode::Valid(slice) =
        decode_metadata_checked(&item.text().unwrap(), metadata)
    else {
        panic!("Copy must publish a valid native v14 carrier");
    };
    assert_eq!(&encode_metadata(&slice).unwrap(), metadata);
    let boundary = ClipboardSourceBoundary::CellRange { root_form };
    assert_eq!(slice.source_boundary(), Some(boundary));
    assert_eq!(boundary.open_depths(), Some((1, 1)));
    assert!(!slice.is_closed());
    assert_eq!(
        slice.text_projection(),
        Some(ClipboardTextProjection::TextBetweenLfV1)
    );
    // A Table-shaped ClipboardNode is only the carrier here. Neither Rows nor
    // whole-table CellRange provenance turns it into closed WholeRoots data.
    let [table] = slice.roots() else {
        panic!("CellRange export must have exactly one Table carrier");
    };
    assert_eq!(table.kind(), &NodeKind::Table);
    let rows = table.content().as_table().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.len() == 2 - start_cell(root_form))
    );
    item
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    notifications: (usize, usize),
}

impl Snapshot {
    fn capture(session: &SharedSession, counts: &Counts) -> Self {
        let session = session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            notifications: counts.get(),
        }
    }

    fn assert_unchanged(&self, session: &SharedSession, counts: &Counts) {
        let session = session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(counts.get(), self.notifications);
    }
}

fn rejects_carrier(
    cx: &mut TestAppContext,
    kind: NodeKind,
    cell_range: bool,
    root_form: ClipboardCellRangeRoot,
) {
    cx.update(bind_default_editor_keys);
    let item = copied_carrier(cx, root_form);
    let target = fixture(kind, cell_range, "target");
    let counts = Rc::new(Cell::new((0, 0)));
    // The receiving EditorInstance deliberately has no SessionPolicy or
    // command router. Rejection must come from the default native path.
    let editor = EditorInstance::new(
        target.document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(target.intro)),
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..EditorHooks::default()
        },
    )
    .unwrap();
    let session = editor.session().clone();
    let (redo_document, redo_selection) = {
        let mut session = session.borrow_mut();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".into(),
            })
            .unwrap();
        let expected = (session.document().clone(), session.selection());
        session.undo().unwrap();
        if cell_range {
            session
                .set_cell_range_selection(target.cells[start_cell(root_form)], target.cells[3])
                .unwrap();
        } else {
            session
                .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
                .unwrap();
            assert_eq!(
                session.stored_marks(),
                Some(&MarkSet::new([Mark::Italic]).unwrap())
            );
        }
        assert_eq!(session.history_depths(), (0, 1));
        expected
    };
    let window = mount(cx, &editor);
    let before = Snapshot::capture(&session, &counts);
    cx.simulate_keystrokes(window.into(), "ctrl-v");
    before.assert_unchanged(&session, &counts);
    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
    window
        .update(cx, |view, window, cx| {
            view.paste(&ClipboardPaste, window, cx)
        })
        .unwrap();
    before.assert_unchanged(&session, &counts);
    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);

    // Verify the retained redo entry's contents, not just stack depths.
    window
        .update(cx, |view, window, cx| view.redo_entry(&Redo, window, cx))
        .unwrap();
    let session = session.borrow();
    assert_eq!(session.document().store(), redo_document.store());
    assert_eq!(session.selection(), redo_selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(counts.get().0, before.notifications.0 + 1);
    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
}

#[gpui::test]
fn rows_cell_carrier_paste_into_unconfigured_paragraph_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(cx, NodeKind::Paragraph, false, ClipboardCellRangeRoot::Rows);
}

#[gpui::test]
fn table_cell_carrier_paste_into_unconfigured_paragraph_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(
        cx,
        NodeKind::Paragraph,
        false,
        ClipboardCellRangeRoot::Table,
    );
}

#[gpui::test]
fn rows_cell_carrier_paste_into_unconfigured_code_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(cx, NodeKind::CodeBlock, false, ClipboardCellRangeRoot::Rows);
}

#[gpui::test]
fn table_cell_carrier_paste_into_unconfigured_code_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(
        cx,
        NodeKind::CodeBlock,
        false,
        ClipboardCellRangeRoot::Table,
    );
}

#[gpui::test]
fn rows_cell_carrier_paste_into_unconfigured_cell_range_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(cx, NodeKind::Paragraph, true, ClipboardCellRangeRoot::Rows);
}

#[gpui::test]
fn table_cell_carrier_paste_into_unconfigured_cell_range_is_noop(cx: &mut TestAppContext) {
    rejects_carrier(cx, NodeKind::Paragraph, true, ClipboardCellRangeRoot::Table);
}
