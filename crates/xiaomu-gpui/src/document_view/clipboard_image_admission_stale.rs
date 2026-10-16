//! Imported assets may only be applied to their exact original source.
use super::*;

#[gpui::test]
fn image_admission_stale_selection_marks_and_revision_never_retarget(cx: &mut TestAppContext) {
    for mutation in 0..5 {
        let m = mount(cx, Target::Caret, Decision::Allow, false, false);
        let session = m.session.clone();
        let head = m.head;
        let changed = Rc::new(RefCell::new(None));
        let observed = changed.clone();
        *m.host.during_import.borrow_mut() = Some(Box::new(move || {
            let mut session = session.borrow_mut();
            match mutation {
                0 => {
                    let point = InlinePoint::at_start_of(head);
                    session.set_inline_selection(point, point).unwrap();
                }
                1 => {
                    session
                        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                        .unwrap();
                }
                2 => {
                    session
                        .apply_intent(&EditIntent::InsertText {
                            text: "changed".into(),
                        })
                        .unwrap();
                }
                4 => {
                    let selection = session.selection();
                    let marks = session.stored_marks().cloned();
                    let revision = session.document().revision();
                    session
                        .apply(
                            &Transaction::new(TransactionOrigin::UserInput).with_step(
                                TransactionStep::SetNodeAttrs {
                                    node: head,
                                    attrs: NodeAttrs::new(
                                        [(
                                            "synthetic".into(),
                                            xiaomu_core::document::AttrValue::Bool(true),
                                        )]
                                        .into(),
                                    )
                                    .unwrap(),
                                },
                            ),
                        )
                        .unwrap();
                    assert_eq!(session.selection(), selection);
                    assert_eq!(session.stored_marks(), marks.as_ref());
                    assert_ne!(session.document().revision(), revision);
                }
                _ => {
                    let offset = session
                        .document()
                        .node(head)
                        .unwrap()
                        .content()
                        .as_inline()
                        .unwrap()
                        .offset_at(1)
                        .unwrap();
                    let point = InlinePoint::new(head, offset, 0, CursorAffinity::Before);
                    session.set_inline_selection(point, point).unwrap();
                }
            }
            *observed.borrow_mut() = Some((
                session.document().clone(),
                session.selection(),
                session.stored_marks().cloned(),
            ));
        }));
        let (events, _subscription) = events(&m, cx);
        paste(&m, cx);
        assert_eq!(m.seen.borrow().imports, 1);
        assert!(m.seen.borrow().planned.is_empty());
        let changed = changed.borrow();
        let (document, selection, marks) = changed.as_ref().unwrap();
        assert_eq!(m.session.borrow().document().store(), document.store());
        assert_eq!(
            m.session.borrow().document().revision(),
            document.revision()
        );
        assert_eq!(m.session.borrow().selection(), *selection);
        assert_eq!(m.session.borrow().stored_marks(), marks.as_ref());
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(
            events.borrow()[0].stage(),
            EditorRejectionStage::ClipboardPaste
        );
        assert_eq!(
            events.borrow()[0].reason(),
            EditorRejectionReason::InvalidSelection
        );
        assert_eq!(
            m.session.borrow().history_depths(),
            (usize::from(matches!(mutation, 2 | 4)), 0)
        );
    }
}

#[gpui::test]
fn image_admission_candidate_rejection_is_atomic_after_one_import(cx: &mut TestAppContext) {
    let m = mount(cx, Target::Node, Decision::Allow, false, false);
    m.seen.borrow_mut().reject_candidate = true;
    let before = m.session.borrow().document().clone();
    let selection = m.session.borrow().selection();
    paste(&m, cx);
    assert_eq!(m.seen.borrow().imports, 1);
    assert_eq!(m.seen.borrow().planned, [selection]);
    assert_eq!(m.seen.borrow().publications, 0);
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}
#[gpui::test]
fn image_admission_replaced_session_silently_cancels(cx: &mut TestAppContext) {
    let m = mount(cx, Target::Caret, Decision::Allow, false, false);
    let session = m.session.clone();
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    *m.host.during_import.borrow_mut() = Some(Box::new(move || {
        let mut live = session.borrow_mut();
        *live = xiaomu_runtime::session::DocumentSession::new(
            live.document().clone(),
            live.selection(),
        )
        .unwrap();
    }));
    let (events, _subscription) = events(&m, cx);
    paste(&m, cx);
    assert_eq!(m.seen.borrow().imports, 1);
    assert!(m.seen.borrow().planned.is_empty());
    assert!(events.borrow().is_empty());
    assert_eq!(m.session.borrow().document().store(), before.store());
    assert_eq!(m.session.borrow().selection(), selection);
    assert_eq!(m.session.borrow().history_depths(), (0, 0));
}
#[gpui::test]
fn image_admission_retained_import_borrow_silently_cancels(cx: &mut TestAppContext) {
    for mutable in [false, true] {
        let m = mount(cx, Target::Caret, Decision::Allow, false, false);
        // An intentionally leaked test-only Rc supplies the static lifetime
        // required by AssetService. The retained guards are released below.
        let session: &'static SharedSession = Box::leak(Box::new(m.session.clone()));
        let immutable_guard = Rc::new(RefCell::new(None));
        let mutable_guard = Rc::new(RefCell::new(None));
        let read = immutable_guard.clone();
        let write = mutable_guard.clone();
        *m.host.during_import.borrow_mut() = Some(Box::new(move || {
            if mutable {
                *write.borrow_mut() = Some(session.borrow_mut());
            } else {
                *read.borrow_mut() = Some(session.borrow());
            }
        }));
        let (events, _subscription) = events(&m, cx);
        image_clipboard(cx);
        // Direct action dispatch avoids later layout borrowing the deliberately
        // busy fixture; it is the same handler reached by the key tests above.
        m.window
            .update(cx, |view, window, cx| {
                view.paste(&ClipboardPaste, window, cx)
            })
            .unwrap();
        immutable_guard.borrow_mut().take();
        mutable_guard.borrow_mut().take();
        cx.background_executor.run_until_parked();
        assert_eq!(m.seen.borrow().imports, 1);
        assert!(m.seen.borrow().planned.is_empty());
        assert!(events.borrow().is_empty());
    }
}
