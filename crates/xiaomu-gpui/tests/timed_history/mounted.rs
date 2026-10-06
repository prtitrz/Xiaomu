use super::*;
use xiaomu_core::{
    text::TextRange,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::{
    EditPlan, IntentDisposition, PolicyError, PrimaryEdit, SelectionUpdate, SessionContext,
};

#[gpui::test]
fn native_input_obeys_499_500_501_and_samples_once_per_callback(cx: &mut TestAppContext) {
    for gap in [499, 500, 501] {
        let clock = Rc::new(ManualClock::default());
        let (editor, node) = editor(clock.clone());
        let session = editor.session();
        let window = mount(&editor, cx);
        assert_eq!(clock.samples(), 0, "mount/focus/render must not sample");
        clock.set(1000);
        cx.simulate_input(window.into(), "a");
        clock.set(1000 + gap);
        cx.simulate_input(window.into(), "b");
        assert_eq!(clock.samples(), 2);
        if gap <= 500 {
            round_trip(window, session, node, &["", "ab"], cx);
        } else {
            round_trip(window, session, node, &["", "a", "ab"], cx);
        }
        assert_eq!(clock.samples(), 2, "traversal must not sample");
    }
}

#[gpui::test]
fn native_typing_timeout_slides_from_the_last_successful_input(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor(clock.clone());
    let window = mount(&editor, cx);
    for (millis, input) in [(1000, "a"), (1500, "b"), (2000, "c"), (2501, "d")] {
        clock.set(millis);
        cx.simulate_input(window.into(), input);
    }
    round_trip(window, editor.session(), node, &["", "abc", "abcd"], cx);
    assert_eq!(clock.samples(), 4);
}

#[gpui::test]
fn selection_away_and_back_neither_samples_nor_refreshes_last_edit_time(cx: &mut TestAppContext) {
    for gap in [499, 500, 501] {
        let clock = Rc::new(ManualClock::default());
        let (editor, node) = editor(clock.clone());
        let session = editor.session();
        let window = mount(&editor, cx);
        clock.set(1000);
        cx.simulate_input(window.into(), "a");
        let selection = session.borrow().selection();
        let revision = session.borrow().document().revision();
        clock.set(1499);
        cx.simulate_keystrokes(window.into(), "left right");
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().document().revision(), revision);
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(clock.samples(), 1);
        clock.set(1000 + gap);
        cx.simulate_input(window.into(), "b");
        if gap <= 500 {
            round_trip(window, session, node, &["", "ab"], cx);
        } else {
            round_trip(window, session, node, &["", "a", "ab"], cx);
        }
        assert_eq!(clock.samples(), 2);
    }
}

#[gpui::test]
fn late_selection_and_refocus_do_not_poison_the_shared_clock_anchor(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor(clock.clone());
    let window = mount(&editor, cx);
    clock.set(1000);
    cx.simulate_input(window.into(), "a");
    clock.set(u64::MAX);
    cx.simulate_keystrokes(window.into(), "left right");
    focus(window, cx);
    assert_eq!(clock.samples(), 1);
    clock.set(1200);
    cx.simulate_input(window.into(), "b");
    round_trip(window, editor.session(), node, &["", "ab"], cx);
    assert_eq!(clock.samples(), 2);
}

#[gpui::test]
fn unicode_scalar_input_preserves_both_pending_mark_modes(cx: &mut TestAppContext) {
    for mode in [
        DefaultTextInputMarks::PreservePending,
        DefaultTextInputMarks::ConsumePending,
    ] {
        let clock = Rc::new(ManualClock::default());
        let (editor, node) = editor_with(clock.clone(), timed(), mode);
        let session = editor.session();
        let window = mount(&editor, cx);
        clock.set(u64::MAX);
        cx.simulate_keystrokes(window.into(), "ctrl-b");
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        assert_eq!(session.borrow().stored_marks(), Some(&bold));
        assert_eq!(clock.samples(), 1, "one guarded central mark intent");
        clock.set(1000);
        cx.simulate_input(window.into(), "甲🙂");
        assert_eq!(
            session.borrow().stored_marks(),
            (mode == DefaultTextInputMarks::PreservePending).then_some(&bold)
        );
        clock.set(1500);
        cx.simulate_input(window.into(), "e\u{301}");
        assert_eq!(clock.samples(), 5, "mark command plus four Unicode scalars");
        let complete = "甲🙂e\u{301}";
        let borrowed = session.borrow();
        let (_, focus) = borrowed.selection().as_same_node_inline().unwrap();
        assert_eq!(focus.text_offset().as_usize(), complete.len());
        assert!(
            borrowed
                .document()
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .runs()
                .iter()
                .all(|run| run.marks() == &bold)
        );
        drop(borrowed);
        round_trip(window, session, node, &["", complete], cx);
        assert_eq!(clock.samples(), 5);
    }
}

#[gpui::test]
fn default_selection_close_remains_independent_of_the_typing_delay(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor_with(
        clock.clone(),
        HistoryOptions::new().with_typing_group_delay_ms(500),
        DefaultTextInputMarks::PreservePending,
    );
    let window = mount(&editor, cx);
    clock.set(1000);
    cx.simulate_input(window.into(), "a");
    cx.simulate_keystrokes(window.into(), "left right");
    clock.set(1100);
    cx.simulate_input(window.into(), "b");
    round_trip(window, editor.session(), node, &["", "a", "ab"], cx);
    assert_eq!(clock.samples(), 2);
}

#[gpui::test]
fn central_and_native_insertions_share_one_clock_and_sliding_anchor(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (editor, node) = editor(clock.clone());
    let window = mount(&editor, cx);
    clock.set(1000);
    cx.simulate_input(window.into(), "a");
    clock.set(1500);
    window
        .update(cx, |view, window, cx| {
            view.apply_edit_intent(EditIntent::InsertText { text: "H".into() }, window, cx);
        })
        .unwrap();
    assert_eq!(clock.samples(), 2, "one guarded central insertion");
    clock.set(2000);
    cx.simulate_input(window.into(), "bc");
    clock.set(2501);
    cx.simulate_input(window.into(), "d");
    round_trip(window, editor.session(), node, &["", "aHbc", "aHbcd"], cx);
    assert_eq!(clock.samples(), 5);
}

struct Host;

impl SessionPolicy for Host {
    fn history_options(&self) -> HistoryOptions {
        timed()
    }

    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        let EditIntent::InsertText { text } = intent else {
            return Ok(IntentDisposition::Continue);
        };
        if text == "m" {
            return Ok(IntentDisposition::StoredMarks(Some(
                MarkSet::new([Mark::Bold]).unwrap(),
            )));
        }
        if text != "h" {
            return Ok(IntentDisposition::Continue);
        }
        let (_, focus) = context.selection().as_same_node_inline().unwrap();
        let range = TextRange::new(focus.text_offset(), focus.text_offset()).unwrap();
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::ReplaceText {
                node: focus.node_id(),
                range,
                replacement: "H".into(),
            },
        );
        Ok(IntentDisposition::Apply(EditPlan::new(
            transaction,
            SelectionUpdate::CaretAfterReplacement,
            Some(PrimaryEdit::new(focus.node_id(), range, 1)),
        )))
    }
}

#[gpui::test]
fn native_host_apply_and_stored_marks_remain_isolated_without_poisoning_time(
    cx: &mut TestAppContext,
) {
    for input in ["h", "m"] {
        let clock = Rc::new(ManualClock::default());
        let (document, node, selection) = fixture();
        let editor = EditorInstance::new_with_policy_and_history_clock(
            document,
            selection,
            EditorHooks::default(),
            Box::new(Host),
            clock.clone(),
        )
        .unwrap();
        let window = mount(&editor, cx);
        clock.set(1000);
        cx.simulate_input(window.into(), "a");
        clock.set(u64::MAX);
        cx.simulate_input(window.into(), input);
        assert_eq!(clock.samples(), 2);
        clock.set(1100);
        cx.simulate_input(window.into(), "b");
        clock.set(1600);
        cx.simulate_input(window.into(), "c");
        if input == "h" {
            round_trip(window, editor.session(), node, &["", "a", "aH", "aHbc"], cx);
        } else {
            assert_eq!(
                editor.session().borrow().stored_marks(),
                Some(&MarkSet::new([Mark::Bold]).unwrap())
            );
            round_trip(window, editor.session(), node, &["", "a", "abc"], cx);
        }
        assert_eq!(clock.samples(), 4);
    }
}
