//! Synthetic platform-callback probes, plus the mounted central-command path.
use super::*;

#[gpui::test]
fn ignored_or_refused_callbacks_preserve_timeout_and_high_water(cx: &mut TestAppContext) {
    for (next_time, depth) in [(1500, 1), (1501, 2)] {
        let opened = open(cx);
        opened.clock.milliseconds.set(1000);
        cx.simulate_input(opened.handle.into(), "a");
        assert_eq!(opened.clock.samples.get(), 1);
        let snapshot = Snapshot::capture(&opened);
        opened.clock.milliseconds.set(5000);
        opened
            .handle
            .update(cx, |view, window, cx| {
                let input = child(view, opened.fixture.before);
                // Ordinary empty input, host NoChange, preflight rejection and
                // candidate rejection all sample but must not publish this time.
                input.update(cx, |input, cx| {
                    for (range, text) in [
                        (None, ""),
                        (None, "?"),
                        (Some(0..1), "!"),
                        (Some(0..1), "#"),
                    ] {
                        input.replace_text_in_range(range, text, window, cx);
                    }
                });
                view.apply_edit_intent(EditIntent::InsertText { text: "?".into() }, window, cx);
                view.apply_edit_intent(EditIntent::InsertText { text: "!".into() }, window, cx);
                view.apply_edit_intent(EditIntent::InsertText { text: "#".into() }, window, cx);
            })
            .unwrap();
        assert_eq!(opened.clock.samples.get(), 8);
        snapshot.assert_unchanged(&opened);
        // A t=5000 high-water leak would isolate both; a timeout refresh
        // would merge both. Exactly one must rejoin t=1000's original edit.
        opened.clock.milliseconds.set(next_time);
        cx.simulate_input(opened.handle.into(), "b");
        assert_eq!(text(&opened.session, opened.fixture.before), "ab");
        assert_eq!(opened.session.borrow().history_depths(), (depth, 0));
        opened.session.borrow_mut().undo().unwrap();
        assert_eq!(
            text(&opened.session, opened.fixture.before),
            if depth == 1 { "" } else { "a" }
        );
    }
}

#[gpui::test]
fn preedit_cancel_and_guarded_central_command_never_sample(cx: &mut TestAppContext) {
    for (next_time, depth) in [(1500, 1), (1501, 2)] {
        let opened = open(cx);
        opened.clock.milliseconds.set(1000);
        cx.simulate_input(opened.handle.into(), "a");
        let snapshot = Snapshot::capture(&opened);
        opened.clock.milliseconds.set(5000);
        opened
            .handle
            .update(cx, |view, window, cx| {
                let input = child(view, opened.fixture.before);
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                    input.replace_and_mark_text_in_range(None, "nihao", Some(5..5), window, cx);
                });
                assert!(view.has_active_composition(cx));
                view.apply_edit_intent(
                    EditIntent::InsertText {
                        text: "blocked".into(),
                    },
                    window,
                    cx,
                );
                input.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "", window, cx);
                    input.replace_and_mark_text_in_range(None, "x", Some(1..1), window, cx);
                    input.replace_and_mark_text_in_range(None, "", None, window, cx);
                    // No composition remains for Linux unmark to commit.
                    input.unmark_text(window, cx);
                });
                assert!(!view.has_active_composition(cx));
            })
            .unwrap();
        assert_eq!(opened.clock.samples.get(), 1);
        snapshot.assert_unchanged(&opened);
        opened.clock.milliseconds.set(next_time);
        cx.simulate_input(opened.handle.into(), "b");
        assert_eq!(opened.session.borrow().history_depths(), (depth, 0));
        assert_eq!(text(&opened.session, opened.fixture.before), "ab");
    }
}

#[gpui::test]
fn explicit_platform_replacement_keeps_atomic_target_barrier(cx: &mut TestAppContext) {
    let opened = open(cx);
    opened.clock.milliseconds.set(1000);
    cx.simulate_input(opened.handle.into(), "a");
    let original_selection = opened.session.borrow().selection();
    opened.clock.milliseconds.set(1001);
    opened
        .handle
        .update(cx, |view, window, cx| {
            child(view, opened.fixture.before).update(cx, |input, cx| {
                input.replace_text_in_range(Some(0..1), "中🙂", window, cx);
            });
        })
        .unwrap();
    assert_eq!(
        opened.clock.samples.get(),
        2,
        "one stamp for a multi-scalar replacement callback"
    );
    assert_eq!(text(&opened.session, opened.fixture.before), "中🙂");
    assert_eq!(
        opened.counts.get(),
        (2, 0),
        "target selection remains tentative"
    );
    assert_eq!(opened.session.borrow().history_depths(), (2, 0));
    opened.clock.milliseconds.set(1002);
    cx.simulate_input(opened.handle.into(), "b");
    assert_eq!(opened.session.borrow().history_depths(), (3, 0));
    for expected in ["中🙂", "a", ""] {
        opened.session.borrow_mut().undo().unwrap();
        assert_eq!(text(&opened.session, opened.fixture.before), expected);
        if expected == "a" {
            assert_eq!(opened.session.borrow().selection(), original_selection);
        }
    }
    for expected in ["a", "中🙂", "中🙂b"] {
        opened.session.borrow_mut().redo().unwrap();
        assert_eq!(text(&opened.session, opened.fixture.before), expected);
    }
}

#[gpui::test]
fn exact_platform_caret_echo_still_groups_in_one_clock_domain(cx: &mut TestAppContext) {
    let opened = open(cx);
    opened.clock.milliseconds.set(1000);
    cx.simulate_input(opened.handle.into(), "a");
    opened.clock.milliseconds.set(1500);
    opened
        .handle
        .update(cx, |view, window, cx| {
            child(view, opened.fixture.before).update(cx, |input, cx| {
                input.replace_text_in_range(Some(1..1), "中🙂", window, cx);
            });
        })
        .unwrap();
    assert_eq!(opened.clock.samples.get(), 2);
    assert_eq!(opened.session.borrow().history_depths(), (1, 0));
    assert_eq!(text(&opened.session, opened.fixture.before), "a中🙂");
    opened.session.borrow_mut().undo().unwrap();
    assert_eq!(text(&opened.session, opened.fixture.before), "");
}

#[gpui::test]
fn revoked_table_callback_and_central_guard_do_not_sample(cx: &mut TestAppContext) {
    let opened = open(cx);
    opened.clock.milliseconds.set(1000);
    cx.simulate_input(opened.handle.into(), "a");
    let caret = opened.session.borrow().selection();
    opened.clock.milliseconds.set(5000);
    opened
        .handle
        .update(cx, |view, window, cx| {
            let stale = child(view, opened.fixture.cell_texts[0]);
            view.set_measured_table_layout(false);
            let hidden = InlinePoint::at_start_of(opened.fixture.cell_texts[0]);
            opened
                .session
                .borrow_mut()
                .set_inline_selection(hidden, hidden)
                .unwrap();
            let snapshot = Snapshot::capture(&opened);
            stale.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "preedit", None, window, cx);
                input.replace_text_in_range(None, "blocked", window, cx);
            });
            view.apply_edit_intent(
                EditIntent::InsertText {
                    text: "blocked".into(),
                },
                window,
                cx,
            );
            snapshot.assert_unchanged(&opened);
            opened
                .session
                .borrow_mut()
                .set_document_selection(caret)
                .unwrap();
            view.focus_selection(window, cx);
        })
        .unwrap();
    assert_eq!(opened.clock.samples.get(), 1);
    opened.clock.milliseconds.set(1500);
    cx.background_executor.run_until_parked();
    cx.simulate_input(opened.handle.into(), "b");
    assert_eq!(text(&opened.session, opened.fixture.before), "ab");
    assert_eq!(opened.session.borrow().history_depths(), (1, 0));
}
