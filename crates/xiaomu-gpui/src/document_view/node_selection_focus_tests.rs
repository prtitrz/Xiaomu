//! Two panes share a native window, but never share selection or input focus.

use super::*;

struct Panes {
    a: Entity<DocumentView>,
    b: Entity<DocumentView>,
}
impl gpui::Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
            .flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().child(self.a.clone()))
            .child(div().flex_1().min_w_0().h_full().child(self.b.clone()))
    }
}

#[gpui::test]
fn node_selection_nochange_reclaims_focus_invalid_does_not_and_background_commit_does_not_steal(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let a = EditorInstance::new_with_policy(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro)),
        EditorHooks::default(),
        Box::new(ReplaceNode {
            seen: seen.clone(),
            reject_prepare: false,
            reject_final: false,
        }),
    )
    .unwrap();
    let b = EditorInstance::new(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro)),
        EditorHooks::default(),
    )
    .unwrap();
    let session_a = a.session().clone();
    let session_b = b.session().clone();
    let selection_b = session_b.borrow().selection();
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let a = cx.new(|_| a.build_view());
            let b = cx.new(|_| b.build_view());
            cx.new(|_| Panes { a, b })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, window, cx| {
            window.activate_window();
            panes.a.update(cx, |view, cx| {
                assert_eq!(
                    view.select_node(f.quote, window, cx).unwrap(),
                    Some(SessionOutcome::SelectionChanged)
                );
            });
            panes
                .b
                .update(cx, |view, cx| view.focus_selection(window, cx));
            assert!(
                panes.b.read(cx).children[0]
                    .1
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
            panes.a.update(cx, |view, cx| {
                assert_eq!(
                    view.select_node(f.document.root(), window, cx),
                    Err(SessionError::SelectionInvalid)
                );
                assert!(!view.range_input_is_focused(window, cx));
                assert_eq!(
                    view.select_node(f.quote, window, cx).unwrap(),
                    Some(SessionOutcome::NoChange)
                );
                assert!(view.range_input_is_focused(window, cx));
            });
            panes
                .b
                .update(cx, |view, cx| view.focus_selection(window, cx));
            // A late callback can finish a canonical edit after this pane lost
            // native focus. Passive render must not reclaim B's keyboard input.
            let input = panes.a.read(cx).range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                input.replace_text_in_range(None, "late", window, cx)
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, window, cx| {
            assert!(panes.a.read(cx).range_input.is_none());
            assert!(
                panes.b.read(cx).children[0]
                    .1
                    .read(cx)
                    .focus_handle(cx)
                    .is_focused(window)
            );
        })
        .unwrap();
    assert_eq!(session_b.borrow().selection(), selection_b);
    assert_eq!(session_b.borrow().document().store(), f.document.store());
    assert_eq!(session_b.borrow().history_depths(), (0, 0));
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
    assert_eq!(seen.borrow()[0].0.as_node_selection(), Some(f.quote));
    cx.simulate_input(handle.into(), "B");
    cx.background_executor.run_until_parked();
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
    assert_eq!(session_b.borrow().history_depths(), (1, 0));
}
