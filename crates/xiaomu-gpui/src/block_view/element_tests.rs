//! Exercise intrinsic measurement, not only definite-width test windows.
use super::*;
use gpui::{EntityInputHandler, TestAppContext, VisualTestContext};

#[gpui::test]
fn cancelled_preedit_is_not_reused_by_intrinsic_measurement(cx: &mut TestAppContext) {
    for available in [AvailableSpace::MinContent, AvailableSpace::MaxContent] {
        let (handle, session, _) = super::super::ime_atom_tests::open(cx, 2);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        let view = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        handle
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "zhongwen", None, window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        // Measure immediately after cancellation, before a definite-width
        // repaint can hide the missing-key/unknown-width cache collision.
        let (measured, _) = visual.draw(
            point(px(0.0), px(0.0)),
            size(available, AvailableSpace::MaxContent),
            |window, cx| {
                view.update(cx, |view, cx| {
                    assert!(view.cache_key.is_none(), "preedit layout is not reusable");
                    view.replace_and_mark_text_in_range(None, "", None, window, cx);
                });
                ParagraphElement { view: view.clone() }
            },
        );
        let shaped: String = measured
            .0
            .borrow()
            .as_ref()
            .unwrap()
            .lines()
            .iter()
            .map(|line| line.text.as_ref())
            .collect();
        assert_eq!(shaped, "A@Ann🙂中Z");
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}
