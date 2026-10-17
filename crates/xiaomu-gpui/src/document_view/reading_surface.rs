//! Complete passive requests after all children have published painted geometry.
use super::DocumentView;
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};

pub(super) struct ReadingSurface {
    view: Entity<DocumentView>,
    content: AnyElement,
}
impl ReadingSurface {
    pub(super) fn new(view: Entity<DocumentView>, content: AnyElement) -> Self {
        Self { view, content }
    }
}
impl IntoElement for ReadingSurface {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for ReadingSurface {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.content.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.content.prepaint(window, cx);
        super::atom_keyboard::prepaint_targets(&self.view, window, cx)
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        targets: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        super::atom_pointer::observe_pointer_motion(
            &self.view,
            bounds.intersect(&window.content_mask().bounds),
            window,
            cx,
        );
        self.content.paint(window, cx);
        for target in targets {
            target.paint(window, cx);
        }
        self.view.update(cx, |view, cx| {
            view.atom_keyboard.borrow_mut().finish_frame(window);
            view.record_atom_pointer_frame();
            view.finish_reading_frame(window, cx);
        });
    }
}
