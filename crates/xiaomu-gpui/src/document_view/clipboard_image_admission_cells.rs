//! Synthetic root-insertion policy proves transport identity only. It is not a
//! cell insertion planner or downstream CellSelection semantics acceptance.
use super::*;

fn mount_cells(cx: &mut TestAppContext) -> (Mounted, [NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let cells = std::array::from_fn(|_| {
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        builder
            .insert(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap()
    });
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let tail = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([table, tail]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let selection =
        DocumentSelection::cell_range(cells[1], cells[0], InlinePoint::at_start_of(tail).into());
    let seen = Rc::new(RefCell::new(Seen::default()));
    let host = Rc::new(Host {
        seen: seen.clone(),
        during_import: RefCell::new(None),
        remote: Cell::new(false),
    });
    let editor = EditorInstance::new_with_policy(
        document,
        selection,
        EditorHooks {
            asset_service: Some(host.clone()),
            ..Default::default()
        },
        Box::new(Planner {
            seen: seen.clone(),
            tail,
            reject: false,
        }),
    )
    .unwrap()
    .with_command_router(Rc::new(Router {
        decision: Decision::Allow,
        seen: seen.clone(),
    }));
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (
        Mounted {
            window,
            session,
            host,
            seen,
            head: tail,
        },
        cells,
    )
}

#[gpui::test]
fn image_admission_full_cell_range_and_park_reach_synthetic_planner(cx: &mut TestAppContext) {
    let (m, _) = mount_cells(cx);
    let document = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    assert!(selection.active_cell_range().is_some());
    paste(&m, cx);
    assert_eq!(m.seen.borrow().admitted, [(selection, None)]);
    assert_eq!(m.seen.borrow().planned, [selection]);
    assert_eq!(m.seen.borrow().imports, 1);
    assert_eq!(m.session.borrow().history_depths(), (1, 0));
    m.session.borrow_mut().undo().unwrap();
    assert_eq!(m.session.borrow().document().store(), document.store());
    assert_eq!(m.session.borrow().selection(), selection);
}

#[gpui::test]
fn image_admission_changed_active_range_with_unchanged_park_is_stale(cx: &mut TestAppContext) {
    let (m, cells) = mount_cells(cx);
    let original = m.session.borrow().selection();
    let changed = DocumentSelection::cell_range(cells[1], cells[1], original.focus());
    assert_eq!(original.anchor(), changed.anchor());
    assert_eq!(original.focus(), changed.focus());
    let session = m.session.clone();
    *m.host.during_import.borrow_mut() = Some(Box::new(move || {
        session
            .borrow_mut()
            .set_document_selection(changed)
            .unwrap();
    }));
    let (events, _subscription) = events(&m, cx);
    paste(&m, cx);
    assert_eq!(m.seen.borrow().imports, 1);
    assert!(m.seen.borrow().planned.is_empty());
    assert_eq!(m.session.borrow().selection(), changed);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(
        events.borrow()[0].reason(),
        EditorRejectionReason::InvalidSelection
    );
}
