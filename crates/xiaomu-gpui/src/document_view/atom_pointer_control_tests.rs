//! Native controls retain their own down/up gesture beside clamped atom hits.

use super::*;

fn control_fixture() -> (Fixture, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("[reference]").unwrap()),
        )
        .unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(" trailing text", MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let inline =
        InlineContent::new([TextRun::new("keep selection", MarkSet::empty()).unwrap()]).unwrap();
    let start = inline.offset_at(2).unwrap();
    let end = inline.offset_at(8).unwrap();
    let selected = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    // A real rule is sufficient task content. With no task text underneath
    // the control, the editor's ordinary nearest-text hit clamps to the chip.
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let item = builder
        .insert(
            NodeKind::TaskItem,
            NodeAttrs::new([("checked".into(), AttrValue::Bool(false))].into()).unwrap(),
            NodeContent::children([rule]),
        )
        .unwrap();
    let list = builder
        .insert(
            NodeKind::TaskList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([selected, paragraph, list]),
        )
        .unwrap();
    (
        Fixture {
            document: XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
            selected,
            atoms: [atom, atom],
            selection: DocumentSelection::new(
                InlinePoint::new(selected, end, 0, CursorAffinity::Before),
                InlinePoint::new(selected, start, 0, CursorAffinity::After),
            ),
        },
        item,
    )
}

fn checked(session: &SharedSession, item: NodeId) -> bool {
    matches!(
        session
            .borrow()
            .document()
            .node(item)
            .unwrap()
            .attrs()
            .get("checked"),
        Some(AttrValue::Bool(true))
    )
}

#[gpui::test]
fn atom_pointer_task_checkbox_owns_clamped_chip_hit(cx: &mut TestAppContext) {
    let (f, item) = control_fixture();
    let host = Rc::new(CapturingFallbackHost::default());
    let (handle, session) = open(&f, Some(host.clone()), cx);
    handle
        .update(cx, |view, _, cx| {
            view.set_block_alignment_provider(Some(Rc::new(LeftAligned)));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let selector = Box::leak(format!("task-checkbox-{item:?}").into_boxed_str());
    let bounds = VisualTestContext::from_window(handle.into(), cx)
        .debug_bounds(selector)
        .expect("native task checkbox was painted");
    let position = bounds.center();
    handle
        .update(cx, |view, _, cx| {
            assert!(view.scroll_handle.bounds().contains(&position));
            let hit = view
                .hit_test(position, cx)
                .expect("nearest text hit exists");
            assert_eq!(hit.point.node_id(), f.paragraph);
            assert_eq!(hit.chip.unwrap().node, f.atoms[0]);
            assert!(
                !hit.inside_visible_bounds,
                "control fixture must resolve a clamped chip"
            );
        })
        .unwrap();

    down(handle, position, Modifiers::default(), cx);
    assert!(
        host.probes.borrow().is_empty(),
        "checkbox press belongs to the control"
    );
    assert!(host.actions.borrow().is_empty());
    unchanged(&f, &session);
    assert!(
        !checked(&session, item),
        "checkbox acts only on completed click"
    );
    handle
        .update(cx, |view, _, _| {
            assert!(view.atom_pointer.is_none());
            assert!(!view.is_dragging);
        })
        .unwrap();
    up(handle, position, Modifiers::default(), cx);
    assert!(checked(&session, item));
    assert_eq!(session.borrow().selection(), f.selection);
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert!(host.probes.borrow().is_empty());
    assert!(host.actions.borrow().is_empty());
    assert_focus(handle, f.selected, cx);

    session.borrow_mut().undo().unwrap();
    assert!(!checked(&session, item));
    assert_eq!(session.borrow().document().store(), f.document.store());
    assert_eq!(session.borrow().selection(), f.selection);
    assert_eq!(session.borrow().history_depths(), (0, 1));
    session.borrow_mut().redo().unwrap();
    assert!(checked(&session, item));
    assert_eq!(session.borrow().selection(), f.selection);
    assert_eq!(session.borrow().history_depths(), (1, 0));
}
