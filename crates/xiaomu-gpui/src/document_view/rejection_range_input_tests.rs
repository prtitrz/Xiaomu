//! All/node/cell-range native feedback reuses the owning editor subscription.

use super::*;

#[gpui::test]
fn all_node_and_cell_range_native_rejections_forward_without_changing_selection(
    cx: &mut TestAppContext,
) {
    for selection in 0..3 {
        for mode in [Mode::Preflight, Mode::Candidate] {
            for callback in callbacks() {
                let m = open(cx, false);
                {
                    let mut session = m.session.borrow_mut();
                    match selection {
                        0 => {
                            let all = DocumentSelection::all(session.document());
                            session.set_document_selection(all).unwrap();
                        }
                        1 => {
                            session.set_node_selection(m.image).unwrap();
                        }
                        _ => {
                            session
                                .set_cell_range_selection(m.cells[0], m.cells[1])
                                .unwrap();
                        }
                    }
                }
                m.window
                    .update(cx, |view, window, cx| {
                        view.focus_selection(window, cx);
                        assert!(view.range_input.is_some());
                    })
                    .unwrap();
                cx.background_executor.run_until_parked();
                let before = Snapshot::capture(&m);
                let (events, _subscription) = watch(&m, Some(before.clone()), cx);
                m.mode.set(Mode::NoChange);
                invoke(&m, callback, cx);
                assert!(events.borrow().is_empty());
                before.assert_session(&m.session, &m.counts);
                // Default All/node typing is already fail-closed before final
                // validation. Keep that error classification and policy scope.
                let reason = match (mode, selection) {
                    (Mode::Candidate, 0) => EditorRejectionReason::InvalidSelection,
                    (Mode::Candidate, 1) => EditorRejectionReason::UnsupportedEdit,
                    _ => EditorRejectionReason::Policy,
                };
                let item = gpui::ClipboardItem::new_string("previous clipboard".into());
                cx.update(|cx| cx.write_to_clipboard(item.clone()));
                m.mode.set(mode);
                for _ in 0..2 {
                    invoke(&m, callback, cx);
                    assert_eq!(events.borrow().len(), 1);
                    assert_eq!(
                        events.borrow()[0].stage(),
                        EditorRejectionStage::NativeInput
                    );
                    assert_eq!(events.borrow()[0].reason(), reason);
                    before.assert_session(&m.session, &m.counts);
                    assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
                    events.borrow_mut().clear();
                }
            }
        }
    }
}

#[gpui::test]
fn retired_input_cannot_emit_into_an_in_place_replacement_view(cx: &mut TestAppContext) {
    for range in [false, true] {
        for same_session in [false, true] {
            for queued in [false, true] {
                let m = open(cx, false);
                let replacement = open(cx, false);
                if range {
                    m.session.borrow_mut().set_node_selection(m.image).unwrap();
                    m.window
                        .update(cx, |view, window, cx| view.focus_selection(window, cx))
                        .unwrap();
                }
                let source_before = Snapshot::capture(&m);
                let replacement_before = Snapshot::capture(&replacement);
                assert_eq!(
                    source_before.document.revision(),
                    replacement_before.document.revision()
                );
                let (events, _subscription) = watch(&m, None, cx);
                m.mode.set(Mode::Preflight);
                replacement.mode.set(Mode::Preflight);
                let new_session = if same_session {
                    m.session.clone()
                } else {
                    replacement.session.clone()
                };
                let retained = m
                    .window
                    .update(cx, |view, window, cx| {
                        let retained = input(view);
                        if queued {
                            retained
                                .update(cx, |view, cx| perform(view, Callback::Typing, window, cx));
                        }
                        // A native input handler may keep the old child alive even
                        // though the host has reused the parent GPUI Entity.
                        *view = DocumentView::new(new_session);
                        view.focus_selection(window, cx);
                        cx.notify();
                        retained
                    })
                    .unwrap();
                cx.background_executor.run_until_parked();
                if !queued {
                    m.window
                        .update(cx, |_, window, cx| {
                            retained
                                .update(cx, |view, cx| perform(view, Callback::Typing, window, cx))
                        })
                        .unwrap();
                }
                cx.background_executor.run_until_parked();
                assert!(
                    events.borrow().is_empty(),
                    "retired child must not impersonate the replacement view"
                );
                source_before.assert_session(&m.session, &m.counts);
                replacement_before.assert_session(&replacement.session, &replacement.counts);
                invoke(&m, Callback::Typing, cx);
                assert_event(&events, EditorRejectionStage::NativeInput);
            }
        }
    }
}
