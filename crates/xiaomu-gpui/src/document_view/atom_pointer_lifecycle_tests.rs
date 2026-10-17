//! Focus, activation and renderer lifetimes cannot revive an old pointer press.

use super::*;
use crate::inline_atom::{InlineAtomRenderer, InlineAtomRendererRegistry, InlineAtomView};

#[gpui::test]
fn atom_pointer_focus_away_and_back_cancels_but_a_new_press_works(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    let original = handle
        .update(cx, |_, window, cx| window.focused(cx).unwrap())
        .unwrap();
    down(handle, point, Modifiers::default(), cx);
    handle
        .update(cx, |view, window, _| {
            window.focus(view.focus_handle.as_ref().unwrap());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |_, window, _| window.focus(&original))
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_focus(handle, f.selected, cx);
    up(handle, point, Modifiers::default(), cx);
    assert!(host.clicks.borrow().is_empty());
    unchanged(&f, &session);
    down(handle, point, Modifiers::default(), cx);
    up(handle, point, Modifiers::default(), cx);
    assert_eq!(host.clicks.borrow().len(), 1);
}

#[gpui::test]
fn atom_pointer_window_deactivation_and_reactivation_cancels_old_press(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    down(handle, point, Modifiers::default(), cx);
    VisualTestContext::from_window(handle.into(), cx).deactivate_window();
    handle
        .update(cx, |_, window, _| {
            assert!(!window.is_window_active());
            window.activate_window();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |_, window, _| assert!(window.is_window_active()))
        .unwrap();
    up(handle, point, Modifiers::default(), cx);
    assert!(host.clicks.borrow().is_empty());
    unchanged(&f, &session);
    down(handle, point, Modifiers::default(), cx);
    up(handle, point, Modifiers::default(), cx);
    assert_eq!(host.clicks.borrow().len(), 1);
}

#[gpui::test]
fn atom_pointer_capture_preserves_no_focus_or_an_unrelated_focus_owner(cx: &mut TestAppContext) {
    for foreign_owner in [false, true] {
        let f = fixture();
        let host = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
        let owner = handle
            .update(cx, |_, window, cx| {
                let owner = foreign_owner.then(|| cx.focus_handle());
                if let Some(owner) = &owner {
                    window.focus(owner);
                } else {
                    window.blur();
                }
                owner
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        down(handle, point, Modifiers::default(), cx);
        handle
            .update(cx, |_, window, cx| assert_eq!(window.focused(cx), owner))
            .unwrap();
        assert!(host.clicks.borrow().is_empty());
        up(handle, point, Modifiers::default(), cx);
        handle
            .update(cx, |_, window, cx| assert_eq!(window.focused(cx), owner))
            .unwrap();
        assert_eq!(host.clicks.borrow().len(), 1);
        unchanged(&f, &session);
    }
}

#[gpui::test]
fn atom_pointer_render_epoch_change_cancels_without_changing_canonical_state(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    down(handle, point, Modifiers::default(), cx);
    handle
        .update(cx, |view, _, _| view.epoch.set(view.epoch.get() + 1))
        .unwrap();
    up(handle, point, Modifiers::default(), cx);
    assert!(host.clicks.borrow().is_empty());
    unchanged(&f, &session);
}

struct CaptureWithDefaultCallback(LegacyHost);

impl InlineAtomHostCapability for CaptureWithDefaultCallback {
    fn atom_action(&self, action: AtomAction) {
        self.0.atom_action(action);
    }

    fn capture_pointer_click(&self, _: &AtomAction, _: Modifiers) -> bool {
        true
    }
}

#[gpui::test]
fn atom_pointer_default_completed_callback_delegates_to_legacy_once_on_release(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let host = Rc::new(CaptureWithDefaultCallback(LegacyHost::default()));
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    let padding = handle
        .update(cx, |view, _, cx| {
            let bounds = view.block_bounds(f.paragraph).unwrap();
            let padding = gpui::point(point.x, bounds.top() - px(4.));
            let hit = view.hit_test(padding, cx).unwrap();
            assert_eq!(hit.chip.unwrap().node, f.atoms[0]);
            assert!(!hit.inside_visible_bounds);
            padding
        })
        .unwrap();
    down(handle, padding, Modifiers::default(), cx);
    assert!(host.0.0.borrow().is_empty());
    up(handle, padding, Modifiers::default(), cx);
    assert!(host.0.0.borrow().is_empty());
    unchanged(&f, &session);
    down(handle, point, Modifiers::default(), cx);
    assert!(host.0.0.borrow().is_empty());
    unchanged(&f, &session);
    up(handle, point, Modifiers::default(), cx);
    assert_eq!(host.0.0.borrow().len(), 1);
    assert_eq!(host.0.0.borrow()[0].node, f.atoms[0]);
    unchanged(&f, &session);
}

#[gpui::test]
fn atom_pointer_non_left_button_never_captures_or_activates(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_down(point, MouseButton::Right, Modifiers::default());
    visual.simulate_mouse_up(point, MouseButton::Right, Modifiers::default());
    assert!(host.probes.borrow().is_empty());
    assert!(host.clicks.borrow().is_empty());
    assert!(host.legacy.borrow().is_empty());
    unchanged(&f, &session);
}

struct ReplacementRenderer;

impl InlineAtomRenderer for ReplacementRenderer {
    fn display_text(&self, _: &InlineAtomView) -> String {
        "replacement label".into()
    }
}

#[gpui::test]
fn atom_pointer_press_before_repaint_cannot_activate_a_replaced_snapshot(cx: &mut TestAppContext) {
    for stale in 0..4 {
        let f = fixture();
        let host = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let painted = chip_point(handle, f.paragraph, f.atoms[0], cx);
        match stale {
            0 => {
                session
                    .borrow_mut()
                    .apply(&Transaction::new(TransactionOrigin::System).with_step(
                        TransactionStep::SetNodeAttrs {
                            node: f.atoms[0],
                            attrs: NodeAttrs::empty(),
                        },
                    ))
                    .unwrap();
            }
            1 => {
                *session.borrow_mut() =
                    DocumentSession::new(f.document.clone(), f.selection).unwrap();
            }
            2 => {
                handle
                    .update(cx, |view, _, _| view.epoch.set(view.epoch.get() + 1))
                    .unwrap();
            }
            _ => {
                let mut renderers = InlineAtomRendererRegistry::new();
                renderers.register(
                    &AtomKind::new("reference").unwrap(),
                    Rc::new(ReplacementRenderer),
                );
                handle
                    .update(cx, |view, _, _| view.set_atom_renderers(Rc::new(renderers)))
                    .unwrap();
            }
        }
        let before = session.borrow().document().clone();
        let history = session.borrow().history_depths();
        // No notify or executor flush occurs after the mutation. GPUI dispatches
        // this real press against the existing painted frame before repainting.
        down(handle, painted, Modifiers::default(), cx);
        up(handle, painted, Modifiers::default(), cx);
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        assert_eq!(session.borrow().selection(), f.selection);
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), history);
        handle.update(cx, |_, _, cx| cx.notify()).unwrap();
        cx.background_executor.run_until_parked();
        let current = chip_point(handle, f.paragraph, f.atoms[0], cx);
        down(handle, current, Modifiers::default(), cx);
        up(handle, current, Modifiers::default(), cx);
        assert_eq!(
            host.clicks.borrow().len(),
            1,
            "a newly painted press can activate"
        );
    }
}
