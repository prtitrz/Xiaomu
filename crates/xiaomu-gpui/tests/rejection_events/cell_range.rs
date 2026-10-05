use super::*;
use xiaomu_runtime::{clipboard::encode_metadata, session::DocumentSession};

fn table_document() -> (XiaomuDocument, NodeId, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    for row in 0..2 {
        let mut row_cells = Vec::new();
        for col in 0..2 {
            let leaf = paragraph(&mut builder, &format!("{row}-{col}"));
            let cell = builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([leaf]),
                )
                .unwrap();
            cells.push(cell);
            row_cells.push(cell);
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
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        cells,
    )
}

struct ExactDimensions;
impl SessionPolicy for ExactDimensions {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::PasteSlice { slice } = intent
            && let Some(range) = context.selection().active_cell_range()
            && let Some(rows) = slice
                .roots()
                .first()
                .and_then(|node| node.content().as_table())
        {
            let target = range.cells(context.document()).unwrap();
            if rows.len() != target.len() || rows[0].len() != target[0].len() {
                return Err(PolicyError::new(format!(
                    "CellRange dimensions differ: {PRIVATE}"
                )));
            }
        }
        Ok(IntentDisposition::Continue)
    }
}

#[gpui::test]
fn wrong_dimension_cell_range_ctrl_v_emits_one_policy_or_runtime_event(cx: &mut TestAppContext) {
    for with_policy in [false, true] {
        let (document, intro, cells) = table_document();
        let caret = DocumentSelection::collapsed(InlinePoint::at_start_of(intro));
        let mut source = DocumentSession::new(document.clone(), caret).unwrap();
        source.set_cell_range_selection(cells[0], cells[3]).unwrap();
        let slice = source.clipboard_slice().unwrap().unwrap();
        let item = gpui::ClipboardItem::new_string_with_metadata(
            slice.plain_text().into(),
            encode_metadata(&slice).unwrap(),
        );
        let counts = Rc::new(Cell::new((0, 0)));
        let hooks = EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..EditorHooks::default()
        };
        let editor = if with_policy {
            EditorInstance::new_with_policy(document, caret, hooks, Box::new(ExactDimensions))
        } else {
            EditorInstance::new(document, caret, hooks)
        }
        .unwrap();
        {
            let mut session = editor.session().borrow_mut();
            session
                .apply_intent(&EditIntent::InsertText {
                    text: "history".into(),
                })
                .unwrap();
            session.undo().unwrap();
            session
                .set_cell_range_selection(cells[0], cells[1])
                .unwrap();
            assert_eq!(
                session
                    .selection()
                    .active_cell_range()
                    .unwrap()
                    .cells(session.document())
                    .unwrap()
                    .len(),
                1
            );
        }
        let m = mount(editor, counts, cx);
        let before = Snapshot::capture(&m);
        let (events, _subscription) = watch(&m, Some(before.clone()), cx);
        cx.update(|cx| cx.write_to_clipboard(item.clone()));
        press(&m, "ctrl-v", cx);
        assert_event(
            &events,
            Stage::Intent,
            if with_policy {
                Reason::Policy
            } else {
                Reason::ClipboardTable
            },
        );
        before.assert_session(&m.session, &m.counts);
        assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
    }
}
