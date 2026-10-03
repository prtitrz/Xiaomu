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

/// Keep the normal ParagraphElement pipeline while supplying an inherited
/// host style. Returning its actual states lets the test observe the manual
/// frame before VisualTestContext::draw refreshes the original window root.
struct HostStyledParagraph {
    paragraph: ParagraphElement,
    style: gpui::TextStyleRefinement,
}

impl IntoElement for HostStyledParagraph {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for HostStyledParagraph {
    type RequestLayoutState = RequestLayoutState;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        window.with_text_style(Some(self.style.clone()), |window| {
            let actual = window.text_style();
            assert_eq!(Some(actual.font_family), self.style.font_family);
            assert_eq!(Some(actual.color), self.style.color);
            assert_eq!(self.paragraph.view.read(cx).epoch.get(), 0);
            self.paragraph.request_layout(id, inspector_id, window, cx)
        })
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        window.with_text_style(Some(self.style.clone()), |window| {
            self.paragraph
                .prepaint(id, inspector_id, bounds, request_layout, window, cx)
        })
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_text_style(Some(self.style.clone()), |window| {
            self.paragraph.paint(
                id,
                inspector_id,
                bounds,
                request_layout,
                prepaint,
                window,
                cx,
            );
            assert_eq!(self.paragraph.view.read(cx).cache_key, prepaint.cache_key);
        });
    }
}

#[gpui::test]
fn host_color_and_font_changes_invalidate_cached_runs_without_an_epoch(cx: &mut TestAppContext) {
    let (handle, session, node) = super::super::ime_atom_tests::open(cx, 2);
    let before = session.borrow().document().clone();
    let view = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    let mut previous = None;
    for (family, color) in [
        (".SystemUIFont", 0x112233ff),
        (".SystemUIFont", 0x445566ff),
        ("monospace", 0x445566ff),
    ] {
        let (request, prepaint) = visual.draw(
            point(px(0.0), px(0.0)),
            size(
                AvailableSpace::Definite(px(320.0)),
                AvailableSpace::MaxContent,
            ),
            |_, _| HostStyledParagraph {
                paragraph: ParagraphElement { view: view.clone() },
                style: gpui::TextStyleRefinement {
                    font_family: Some(family.into()),
                    color: Some(rgba(color).into()),
                    ..Default::default()
                },
            },
        );
        // Do not read view.cache_key through handle.update here: draw() calls
        // window.refresh(), and the original root may already have repainted
        // at its default style and 1536px width. Observe this frame directly.
        let key = prepaint
            .cache_key
            .expect("manual definite-width frame is cached");
        assert_eq!(
            key,
            LayoutCacheKey::new(node, 0, 320.0).with_style(request.1)
        );
        if let Some(previous) = previous {
            assert_ne!(key, previous);
        }
        previous = Some(key);
    }
    assert_eq!(session.borrow().document().store(), before.store());
}
