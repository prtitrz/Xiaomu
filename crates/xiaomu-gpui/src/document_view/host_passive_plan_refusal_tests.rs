//! All composition surfaces and checked-plan errors leave the receiver untouched.

use super::*;
use xiaomu_runtime::session::{PolicyError, SessionPolicy};

#[gpui::test]
fn passive_composition_guard_covers_focused_and_nonfocused_children_and_ranges(
    cx: &mut TestAppContext,
) {
    for range in [false, true] {
        for focused in [false, true] {
            let original = document(2);
            let counts = Rc::new(Cell::new(0));
            let a = editor(original.clone(), counts.clone());
            let b = editor(original, Default::default());
            let session = a.session().clone();
            // Leave an existing Redo entry and explicit typing marks to protect.
            session
                .borrow_mut()
                .apply_intent(&EditIntent::InsertText {
                    text: "seed".into(),
                })
                .unwrap();
            session.borrow_mut().undo().unwrap();
            let document = session.borrow().document().clone();
            if range {
                session
                    .borrow_mut()
                    .set_document_selection(DocumentSelection::all(&document))
                    .unwrap();
            } else {
                session
                    .borrow_mut()
                    .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                        children(&document)[1],
                    )))
                    .unwrap();
                session
                    .borrow_mut()
                    .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
                    .unwrap();
            }
            let handle = open_panes(cx, &a, &b);
            if focused {
                handle
                    .update(cx, |panes, window, cx| {
                        panes
                            .a
                            .update(cx, |view, cx| view.focus_selection(window, cx));
                    })
                    .unwrap();
                cx.background_executor.run_until_parked();
            }
            let plan = replacement(&document, 2, SelectionUpdate::CaretAtDocumentEnd);
            handle
                .update(cx, |panes, window, cx| {
                    panes.a.update(cx, |view, cx| {
                        let input = if range {
                            view.range_input.as_ref().unwrap().1.clone()
                        } else {
                            view.children[1].1.clone()
                        };
                        assert_eq!(input.read(cx).focus_handle(cx).is_focused(window), focused);
                        input.update(cx, |input, cx| {
                            input.replace_and_mark_text_in_range(
                                None,
                                "ni",
                                Some(1..1),
                                window,
                                cx,
                            );
                            assert_eq!(input.marked_text_range(window, cx), Some(0..2));
                        });
                        // A nonselected, nonfocused paragraph must also block import.
                        if !range && !focused {
                            session
                                .borrow_mut()
                                .set_document_selection(DocumentSelection::collapsed(
                                    InlinePoint::at_start_of(children(&document)[0]),
                                ))
                                .unwrap();
                        }
                        let selection = session.borrow().selection();
                        let marks = session.borrow().stored_marks().cloned();
                        let history = session.borrow().history_depths();
                        let notifications = counts.get();
                        let epoch = view.epoch.get();
                        let geometry = view.registry.borrow().clone();
                        let owner = window.focused(cx);
                        view.is_dragging = true;
                        view.cell_drag_anchor = Some(children(&document)[0]);
                        assert_eq!(
                            view.apply_passive_edit_plan(&plan, window, cx).unwrap(),
                            None
                        );
                        assert_eq!(view.epoch.get(), epoch);
                        assert_eq!(*view.registry.borrow(), geometry);
                        assert_eq!(window.focused(cx), owner);
                        assert!(view.is_dragging);
                        assert_eq!(view.cell_drag_anchor, Some(children(&document)[0]));
                        assert_eq!(session.borrow().document().store(), document.store());
                        assert_eq!(session.borrow().document().revision(), document.revision());
                        assert_eq!(session.borrow().selection(), selection);
                        assert_eq!(session.borrow().stored_marks(), marks.as_ref());
                        assert_eq!(session.borrow().history_depths(), history);
                        assert_eq!(counts.get(), notifications);
                        input.update(cx, |input, cx| {
                            assert_eq!(input.marked_text_range(window, cx), Some(0..2));
                            assert_eq!(
                                input.selected_text_range(false, window, cx).unwrap().range,
                                1..1
                            );
                            input.replace_and_mark_text_in_range(None, "", None, window, cx);
                        });
                        assert!(!view.has_active_composition(cx));
                        assert_eq!(
                            view.apply_passive_edit_plan(&plan, window, cx).unwrap(),
                            Some(SessionOutcome::DocumentChanged)
                        );
                    });
                })
                .unwrap();
        }
    }
}

struct RejectReplacement(NodeId);
impl SessionPolicy for RejectReplacement {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.node(self.0).is_none() {
            Err(PolicyError::new("replacement refused"))
        } else {
            Ok(())
        }
    }
}

#[gpui::test]
fn passive_invalid_selection_and_policy_refusal_preserve_session_geometry_and_focus(
    cx: &mut TestAppContext,
) {
    for policy_refusal in [false, true] {
        let original = document(2);
        let selection =
            DocumentSelection::collapsed(InlinePoint::at_start_of(children(&original)[0]));
        let counts = Rc::new(Cell::new(0));
        let a = EditorInstance::new_with_policy(
            original.clone(),
            selection,
            EditorHooks {
                listener: Some(Box::new(Listener(counts.clone()))),
                ..Default::default()
            },
            Box::new(RejectReplacement(children(&original)[0])),
        )
        .unwrap();
        let b = editor(original.clone(), Default::default());
        let session = a.session().clone();
        session
            .borrow_mut()
            .apply_intent(&EditIntent::InsertText {
                text: "seed".into(),
            })
            .unwrap();
        session.borrow_mut().undo().unwrap();
        session
            .borrow_mut()
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        let document = session.borrow().document().clone();
        let handle = open_panes(cx, &a, &b);
        let after = if policy_refusal {
            SelectionUpdate::CaretAtDocumentEnd
        } else {
            SelectionUpdate::Exact { selection }
        };
        let plan = replacement(&document, 2, after);
        handle
            .update(cx, |panes, window, cx| {
                panes.a.update(cx, |view, cx| {
                    let epoch = view.epoch.get();
                    let geometry = view.registry.borrow().clone();
                    let ids: Vec<_> = view
                        .children
                        .iter()
                        .map(|(_, child)| child.entity_id())
                        .collect();
                    let owner = window.focused(cx);
                    let notifications = counts.get();
                    let marks = session.borrow().stored_marks().cloned();
                    let history = session.borrow().history_depths();
                    view.is_dragging = true;
                    view.desired_x =
                        Some((InlinePoint::at_start_of(children(&document)[0]), px(9.0)));
                    assert!(view.apply_passive_edit_plan(&plan, window, cx).is_err());
                    assert_eq!(view.epoch.get(), epoch);
                    assert_eq!(*view.registry.borrow(), geometry);
                    assert_eq!(
                        view.children
                            .iter()
                            .map(|(_, child)| child.entity_id())
                            .collect::<Vec<_>>(),
                        ids
                    );
                    assert_eq!(window.focused(cx), owner);
                    assert!(view.is_dragging);
                    assert!(view.desired_x.is_some());
                    assert_eq!(session.borrow().document().store(), document.store());
                    assert_eq!(session.borrow().document().revision(), document.revision());
                    assert_eq!(session.borrow().selection(), selection);
                    assert_eq!(session.borrow().stored_marks(), marks.as_ref());
                    assert_eq!(session.borrow().history_depths(), history);
                    assert_eq!(counts.get(), notifications);
                });
            })
            .unwrap();
    }
}
