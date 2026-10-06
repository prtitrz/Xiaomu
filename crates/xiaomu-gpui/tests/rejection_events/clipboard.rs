use super::*;
use xiaomu_core::document::AttrValue;

#[gpui::test]
fn native_metadata_rejection_emits_once_without_pasting_its_text(cx: &mut TestAppContext) {
    let (m, _) = configured(cx);
    let before = Snapshot::capture(&m);
    let (events, _subscription) = watch(&m, Some(before.clone()), cx);
    let item = gpui::ClipboardItem::new_string_with_metadata(
        PRIVATE.into(),
        "xiaomu.clipboard.v14\n{".into(),
    );
    cx.update(|cx| cx.write_to_clipboard(item.clone()));
    press(&m, "ctrl-v", cx);
    assert_event(&events, Stage::ClipboardPaste, Reason::ClipboardMetadata);
    before.assert_session(&m.session, &m.counts);
    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
}

#[gpui::test]
fn empty_foreign_clipboard_and_collapsed_copy_cut_are_not_rejections(cx: &mut TestAppContext) {
    let (m, _) = configured(cx);
    let before = Snapshot::capture(&m);
    let (events, _subscription) = watch(&m, None, cx);
    for key in ["ctrl-v", "ctrl-c", "ctrl-x"] {
        install_text(cx, "");
        press(&m, key, cx);
    }
    before.assert_session(&m.session, &m.counts);
    assert!(events.borrow().is_empty());
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
            "fallback".into(),
            "foreign metadata".into(),
        ))
    });
    press(&m, "ctrl-v", cx);
    assert_eq!(m.session.borrow().history_depths(), (1, 0));
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn copy_and_cut_projection_refusals_preserve_state_and_clipboard(cx: &mut TestAppContext) {
    for (key, stage) in [
        ("ctrl-c", Stage::ClipboardCopy),
        ("ctrl-x", Stage::ClipboardCut),
    ] {
        let (m, mode) = configured(cx);
        {
            let mut session = m.session.borrow_mut();
            let all = DocumentSelection::all(session.document());
            session.set_document_selection(all).unwrap();
        }
        m.window
            .update(cx, |view, window, cx| view.focus_selection(window, cx))
            .unwrap();
        cx.background_executor.run_until_parked();
        let before = Snapshot::capture(&m);
        let (events, _subscription) = watch(&m, Some(before.clone()), cx);
        mode.set(Mode::RejectExport);
        install_text(cx, "previous clipboard");
        press(&m, key, cx);
        assert_event(&events, stage, Reason::Policy);
        before.assert_session(&m.session, &m.counts);
        assert_eq!(
            cx.update(|cx| cx.read_from_clipboard().unwrap().text())
                .as_deref(),
            Some("previous clipboard")
        );
    }
}

fn oversized_atomic(cx: &mut TestAppContext, whole: bool) -> Mounted {
    let mut payload = AttrValue::String(PRIVATE.into());
    for _ in 0..160 {
        payload = AttrValue::List(vec![payload]);
    }
    let mut builder = NodeStoreBuilder::new();
    let intro = paragraph(&mut builder, "intro");
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new([("extension".into(), payload)].into()).unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, image]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let counts = Rc::new(Cell::new((0, 0)));
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(intro)),
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..EditorHooks::default()
        },
    )
    .unwrap();
    {
        let mut session = editor.session().borrow_mut();
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
            session.set_atomic_selection(image).unwrap();
        }
        assert!(
            xiaomu_runtime::clipboard::encode_metadata(
                &session.clipboard_slice().unwrap().unwrap()
            )
            .is_err()
        );
    }
    mount(editor, counts, cx)
}

#[gpui::test]
fn lossless_copy_and_cut_transport_failures_emit_but_legacy_copy_fallback_is_silent(
    cx: &mut TestAppContext,
) {
    for (whole, key, expected) in [
        (true, "ctrl-c", Some(Stage::ClipboardCopy)),
        (false, "ctrl-x", Some(Stage::ClipboardCut)),
        (false, "ctrl-c", None),
    ] {
        let m = oversized_atomic(cx, whole);
        let before = Snapshot::capture(&m);
        let (events, _subscription) = watch(&m, Some(before.clone()), cx);
        install_text(cx, "previous clipboard");
        press(&m, key, cx);
        if let Some(stage) = expected {
            assert_event(&events, stage, Reason::ClipboardMetadata);
            assert_eq!(
                cx.update(|cx| cx.read_from_clipboard().unwrap().text())
                    .as_deref(),
                Some("previous clipboard")
            );
        } else {
            assert!(events.borrow().is_empty());
            let item = cx.update(|cx| cx.read_from_clipboard().unwrap());
            assert!(item.metadata().is_none());
            assert_eq!(
                item.text().as_deref(),
                Some(
                    m.session
                        .borrow()
                        .clipboard_slice()
                        .unwrap()
                        .unwrap()
                        .plain_text()
                )
            );
        }
        before.assert_session(&m.session, &m.counts);
    }
}
