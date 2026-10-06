use super::*;
use std::cell::RefCell;
use xiaomu_runtime::session::DocumentSession;

#[gpui::test]
fn later_document_views_reuse_the_instances_clock_origin(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor(clock.clone());
    let first = mount(&editor, cx);
    clock.set(1000);
    cx.simulate_input(first.into(), "a");

    // Build after input has already established the session's time anchor.
    let second = mount(&editor, cx);
    assert_eq!(clock.samples(), 1);
    clock.set(1500);
    cx.simulate_input(second.into(), "b");
    assert_eq!(editor.session().borrow().history_depths(), (1, 0));
    focus(first, cx);
    clock.set(2001);
    cx.simulate_input(first.into(), "c");
    round_trip(first, editor.session(), node, &["", "ab", "abc"], cx);
    assert_eq!(clock.samples(), 3);
}

#[gpui::test]
fn newly_materialized_split_child_keeps_the_instances_clock(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, first_node) = editor(clock.clone());
    let session = editor.session();
    let window = mount(&editor, cx);
    clock.set(1000);
    cx.simulate_input(window.into(), "a");
    clock.set(u64::MAX);
    cx.simulate_keystrokes(window.into(), "enter");
    assert_eq!(
        clock.samples(),
        2,
        "one native insertion and one split intent"
    );
    let child = session
        .borrow()
        .selection()
        .as_single_node()
        .unwrap()
        .focus()
        .node_id();
    assert_ne!(child, first_node);
    let owner = window
        .update(cx, |view, window, cx| {
            view.accessibility_projection(window, cx)
                .unwrap()
                .focus_owner()
        })
        .unwrap();
    assert_eq!(owner, Some(child));
    for (millis, input) in [(1100, "b"), (1600, "c"), (2101, "d")] {
        clock.set(millis);
        cx.simulate_input(window.into(), input);
    }
    assert_eq!(text(session, first_node), "a");
    assert_eq!(text(session, child), "bcd");
    assert_eq!(session.borrow().history_depths(), (4, 0));
    let document = session.borrow().document().clone();
    let selection = session.borrow().selection();
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert_eq!(text(session, child), "bc");
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert_eq!(text(session, child), "");
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert!(session.borrow().document().node(child).is_none());
    assert_eq!(text(session, first_node), "a");
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    assert_eq!(text(session, first_node), "");
    assert_eq!(session.borrow().history_depths(), (0, 4));
    cx.simulate_keystrokes(
        window.into(),
        "ctrl-shift-z ctrl-shift-z ctrl-shift-z ctrl-shift-z",
    );
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (4, 0));
    assert_eq!(clock.samples(), 5);
}

#[gpui::test]
fn separate_editors_keep_independent_clocks_and_history(cx: &mut TestAppContext) {
    let clock_a = Rc::new(ManualClock::default());
    let clock_b = Rc::new(ManualClock::default());
    let (editor_a, node_a) = editor(clock_a.clone());
    let (editor_b, node_b) = editor(clock_b.clone());
    let window_a = mount(&editor_a, cx);
    let window_b = mount(&editor_b, cx);
    focus(window_a, cx);
    clock_a.set(1000);
    cx.simulate_input(window_a.into(), "a");
    assert_eq!(clock_a.samples(), 1);
    assert_eq!(clock_b.samples(), 0);
    assert_eq!(text(editor_b.session(), node_b), "");
    focus(window_b, cx);
    clock_b.set(1_000_000);
    cx.simulate_input(window_b.into(), "x");
    focus(window_a, cx);
    clock_a.set(1500);
    cx.simulate_input(window_a.into(), "b");
    focus(window_b, cx);
    clock_b.set(1_000_501);
    cx.simulate_input(window_b.into(), "y");
    assert_eq!(editor_a.session().borrow().history_depths(), (1, 0));
    assert_eq!(editor_b.session().borrow().history_depths(), (2, 0));
    round_trip(window_b, editor_b.session(), node_b, &["", "x", "xy"], cx);
    assert_eq!(text(editor_a.session(), node_a), "ab");
    focus(window_a, cx);
    round_trip(window_a, editor_a.session(), node_a, &["", "ab"], cx);
    assert_eq!(clock_a.samples(), 2);
    assert_eq!(clock_b.samples(), 2);
}

#[gpui::test]
fn legacy_editor_constructors_keep_default_timeless_grouping(cx: &mut TestAppContext) {
    for use_policy in [false, true] {
        let (document, node, selection) = fixture();
        let editor = if !use_policy {
            EditorInstance::new(document, selection, EditorHooks::default())
        } else {
            EditorInstance::new_with_policy(
                document,
                selection,
                EditorHooks::default(),
                Box::new(Options {
                    history: HistoryOptions::new(),
                    marks: DefaultTextInputMarks::PreservePending,
                }),
            )
        }
        .unwrap();
        let window = mount(&editor, cx);
        cx.simulate_input(window.into(), "ab");
        round_trip(window, editor.session(), node, &["", "ab"], cx);
    }
}

#[gpui::test]
fn document_view_clock_constructor_and_legacy_fallback_are_independent(cx: &mut TestAppContext) {
    for use_clock in [false, true] {
        let (document, node, selection) = fixture();
        let session = Rc::new(RefCell::new(
            DocumentSession::new_with_policy(
                document,
                selection,
                Box::new(Options {
                    history: timed(),
                    marks: DefaultTextInputMarks::PreservePending,
                }),
            )
            .unwrap(),
        ));
        let clock = Rc::new(ManualClock::default());
        let view = if use_clock {
            DocumentView::new_with_history_clock(session.clone(), clock.clone())
        } else {
            DocumentView::new(session.clone())
        };
        let window = mount_view(view, cx);
        clock.set(1000);
        cx.simulate_input(window.into(), "a");
        clock.set(1500);
        cx.simulate_input(window.into(), "b");
        if use_clock {
            round_trip(window, &session, node, &["", "ab"], cx);
        } else {
            round_trip(window, &session, node, &["", "a", "ab"], cx);
        }
        assert_eq!(clock.samples(), if use_clock { 2 } else { 0 });
    }
}

#[gpui::test]
fn a_clock_alone_does_not_enable_timed_runtime_grouping(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor_with(
        clock.clone(),
        HistoryOptions::new(),
        DefaultTextInputMarks::PreservePending,
    );
    let window = mount(&editor, cx);
    clock.set(0);
    cx.simulate_input(window.into(), "a");
    clock.set(u64::MAX);
    cx.simulate_input(window.into(), "b");
    round_trip(window, editor.session(), node, &["", "ab"], cx);
    assert_eq!(clock.samples(), 2);
}
