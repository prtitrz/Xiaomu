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
    type PrepaintState = ();
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
    ) {
        self.content.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content.paint(window, cx);
        self.view
            .update(cx, |view, cx| view.finish_reading_frame(window, cx));
    }
}
