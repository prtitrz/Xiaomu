//! Same-frame width adapter for explicitly enabled measured tables.
//!
//! `DocumentView` uses this only after per-instance opt-in, with independent
//! per-table measurement admission. It does not imply browser-CSS parity or
//! completed native GUI verification. The containing layout must supply a
//! bounded viewport,
//! such as the existing editor's `size_full()` pane; an intrinsically sized
//! document cannot determine its own containing width/height with this API.
//!
//! The viewport participates normally in its parent's layout. Only after that
//! layout finishes does prepaint pass its **current actual size** to the subtree
//! builder. The builder creates the complete scroll container and document,
//! propagating content widths after real padding/borders to the table and any
//! nested tables. The entire subtree is laid out as a root at that definite
//! size, then prepainted at the actual viewport origin. Its content height,
//! sibling positions and scroll range therefore come from the same frame.
//!
//! This deliberately does not call `layout_as_root` from a measured-layout
//! callback (stock GPUI temporarily removes the layout engine there), retain
//! last-frame widths/heights, or call `notify`/`refresh` to converge. Build a new
//! viewport element each render; the `FnOnce` builder is consumed once during
//! its single prepaint, as required by stock GPUI's normal element lifecycle.
//! Render this adapter beneath an ordinary `Entity<impl Render>`: GPUI's scroll
//! listener painting needs the owning rendered-view scope. A bare element drawn
//! with `VisualTestContext::draw` does not acquire that scope just because the
//! test window happens to contain another root entity.
//!
//! The `DocumentView` connection keeps state synchronization and
//! focus ownership outside this builder, clear geometry registries once when
//! the builder starts, and build the current listener-bearing scroll container
//! inside it using the same entity. Table failures retain a visible placeholder
//! plus edit rejection. This adapter alone never grants edit permission.

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Size, Style, Window, px, relative, size,
};

type SubtreeBuilder = Box<dyn FnOnce(Size<Pixels>, &mut Window, &mut App) -> AnyElement>;

/// Fixed-viewport adapter whose builder receives this frame's actual size.
///
/// The subtree should be a viewport-filling scroll root. The builder owns its
/// real insets, content-width propagation, styling and events. This adapter
/// adds no padding, scroll state, or hidden geometry translation of its own.
pub struct SameFrameTableViewport {
    id: ElementId,
    builder: Option<SubtreeBuilder>,
}

impl SameFrameTableViewport {
    /// Defers construction of a complete subtree until the viewport is laid out.
    #[must_use]
    pub fn new<E: IntoElement + 'static>(
        id: impl Into<ElementId>,
        builder: impl FnOnce(Size<Pixels>, &mut Window, &mut App) -> E + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            builder: Some(Box::new(move |size, window, cx| {
                builder(size, window, cx).into_any_element()
            })),
        }
    }
}

/// Current-frame viewport/subtree geometry, available to prototype hosts/tests.
pub struct TableViewportPrepaintState {
    /// Actual bounds assigned by the containing layout, including its origin.
    pub viewport_bounds: Bounds<Pixels>,
    /// Result from laying out the returned subtree with definite current size.
    /// A viewport-filling scroll root should return the viewport's own size;
    /// its `ScrollHandle` exposes the separately measured content range.
    pub subtree_size: Size<Pixels>,
    subtree: AnyElement,
}

impl IntoElement for SameFrameTableViewport {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for SameFrameTableViewport {
    type RequestLayoutState = ();
    type PrepaintState = TableViewportPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            size: size(relative(1.0).into(), relative(1.0).into()),
            min_size: size(px(0.0).into(), px(0.0).into()),
            ..Style::default()
        };
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let builder = self
            .builder
            .take()
            .expect("rebuild SameFrameTableViewport for each element lifecycle");
        let mut subtree = builder(bounds.size, window, cx);
        let subtree_size = subtree.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        subtree.prepaint_at(bounds.origin, window, cx);
        TableViewportPrepaintState {
            viewport_bounds: bounds,
            subtree_size,
            subtree,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        prepaint.subtree.paint(window, cx);
    }
}

#[cfg(test)]
mod tests;
