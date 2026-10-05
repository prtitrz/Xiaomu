//! Public EntityInputHandler calls exercise callback boundaries directly;
//! keyboard actions belong in the mounted DocumentView tests.

use super::*;
use std::cell::RefCell;

use gpui::{AppContext as _, EntityInputHandler, Focusable as _};
use xiaomu_gpui::block_view::ParagraphView;
use xiaomu_runtime::session::DocumentSession;

fn paragraph(
    clock: Option<Rc<ManualClock>>,
    history: HistoryOptions,
    marks: DefaultTextInputMarks,
    cx: &mut TestAppContext,
) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
    let (document, node, selection) = fixture();
    let session = Rc::new(RefCell::new(
        DocumentSession::new_with_policy(document, selection, Box::new(Options { history, marks }))
            .unwrap(),
    ));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            cx.new(|cx| {
                let epoch = Rc::new(Cell::new(0));
                let bounds = Rc::new(RefCell::new(Vec::new()));
                let view = if let Some(clock) = clock {
                    ParagraphView::new_with_history_clock(
                        session.clone(),
                        epoch,
                        bounds,
                        node,
                        clock,
                        cx,
                    )
                } else {
                    ParagraphView::new(session.clone(), epoch, bounds, node, cx)
                };
                window.activate_window();
                window.focus(&view.focus_handle(cx));
                view
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    (window, session, node)
}

#[gpui::test]
fn one_unicode_batch_callback_samples_once_and_keeps_utf8_selection(cx: &mut TestAppContext) {
    for mode in [
        DefaultTextInputMarks::PreservePending,
        DefaultTextInputMarks::ConsumePending,
    ] {
        let clock = Rc::new(ManualClock::default());
        let (window, session, node) = paragraph(Some(clock.clone()), timed(), mode, cx);
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        session
            .borrow_mut()
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
            .unwrap();
        assert_eq!(clock.samples(), 0);
        clock.set(1000);
        window
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "甲🙂e\u{301}", window, cx);
            })
            .unwrap();
        assert_eq!(clock.samples(), 1);
        assert_eq!(
            session.borrow().stored_marks(),
            (mode == DefaultTextInputMarks::PreservePending).then_some(&bold)
        );
        clock.set(1500);
        window
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "乙🙂", window, cx);
            })
            .unwrap();
        assert_eq!(clock.samples(), 2);
        let complete = "甲🙂e\u{301}乙🙂";
        assert_eq!(text(&session, node), complete);
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
        assert_eq!(session.borrow().history_depths(), (1, 0));
        let document = session.borrow().document().clone();
        session.borrow_mut().undo().unwrap();
        assert_eq!(text(&session, node), "");
        session.borrow_mut().redo().unwrap();
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(clock.samples(), 2);
    }
}

#[gpui::test]
fn composition_commit_stays_isolated_without_advancing_typing_high_water(cx: &mut TestAppContext) {
    let clock = Rc::new(ManualClock::default());
    let (window, session, node) = paragraph(
        Some(clock.clone()),
        timed(),
        DefaultTextInputMarks::ConsumePending,
        cx,
    );
    clock.set(1000);
    cx.simulate_input(window.into(), "a");
    clock.set(u64::MAX);
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "中🙂", Some(3..3), window, cx);
            assert_eq!(view.marked_text_range(window, cx), Some(1..4));
        })
        .unwrap();
    assert_eq!(text(&session, node), "a");
    assert_eq!(clock.samples(), 1, "preedit is frontend-only");
    window
        .update(cx, |view, window, cx| {
            view.replace_text_in_range(None, "中🙂", window, cx);
            assert_eq!(view.marked_text_range(window, cx), None);
        })
        .unwrap();
    assert_eq!(clock.samples(), 2, "one canonical composition callback");
    assert_eq!(text(&session, node), "a中🙂");
    assert_eq!(session.borrow().history_depths(), (2, 0));
    // Composition's MAX stamp must not poison the successful typing high-water.
    clock.set(1100);
    cx.simulate_input(window.into(), "b");
    clock.set(1600);
    cx.simulate_input(window.into(), "c");
    assert_eq!(session.borrow().history_depths(), (3, 0));
    let document = session.borrow().document().clone();
    for expected in ["a中🙂", "a", ""] {
        session.borrow_mut().undo().unwrap();
        assert_eq!(text(&session, node), expected);
    }
    for expected in ["a", "a中🙂", "a中🙂bc"] {
        session.borrow_mut().redo().unwrap();
        assert_eq!(text(&session, node), expected);
    }
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(clock.samples(), 4);
}

#[gpui::test]
fn standalone_legacy_paragraph_remains_unstamped(cx: &mut TestAppContext) {
    for history in [HistoryOptions::new(), timed()] {
        let (window, session, node) =
            paragraph(None, history, DefaultTextInputMarks::PreservePending, cx);
        cx.simulate_input(window.into(), "ab");
        let timed = history.typing_group_delay_ms().is_some();
        assert_eq!(
            session.borrow().history_depths(),
            (if timed { 2 } else { 1 }, 0)
        );
        session.borrow_mut().undo().unwrap();
        assert_eq!(text(&session, node), if timed { "a" } else { "" });
        session.borrow_mut().redo().unwrap();
        assert_eq!(text(&session, node), "ab");
    }
}
