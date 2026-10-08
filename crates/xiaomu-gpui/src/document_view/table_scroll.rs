//! Per-table presentation-only horizontal viewports; no visible scrollbars.
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, ParentElement, Pixels, Point, ScrollHandle, Styled, WeakEntity, Window, div, point,
    prelude::{InteractiveElement, StatefulInteractiveElement},
    px,
};
use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};

use super::DocumentView;

pub(super) type SharedTableScroll = Rc<RefCell<Option<ScrollHandle>>>;
pub(super) type SharedTableLayout = Rc<Cell<Option<LayoutId>>>;
pub(super) type TableClipRegistry = Rc<RefCell<HashMap<NodeId, Bounds<Pixels>>>>;

#[derive(Clone)]
pub(super) struct TableScrollMeasurement {
    pub handle: ScrollHandle,
    pub origin: Point<Pixels>,
    pub width: Pixels,
    pub constraint: Pixels,
    pub maximum: Pixels,
    pub scale: f32,
    pub offset: Point<Pixels>,
}

impl TableScrollMeasurement {
    pub fn capture(handle: &ScrollHandle, constraint: Pixels, scale: f32) -> Self {
        Self {
            handle: handle.clone(),
            origin: handle.bounds().origin,
            width: handle.bounds().size.width,
            constraint,
            maximum: handle.max_offset().width,
            scale,
            offset: handle.offset(),
        }
    }
    pub fn projected_offset(&self, maximum: Pixels) -> Point<Pixels> {
        point(self.offset.x.clamp(-maximum.max(px(0.0)), px(0.0)), px(0.0))
    }
}

pub(super) struct TableScrollPreview {
    pub original: TableScrollMeasurement,
    pub observed: TableScrollMeasurement,
}

pub(super) struct TableScrollViewport {
    pub table: NodeId,
    pub width: Pixels,
    pub content_layout: SharedTableLayout,
    pub preview: Option<TableScrollPreview>,
    pub scroll: SharedTableScroll,
    pub view: WeakEntity<DocumentView>,
    pub content: Option<AnyElement>,
}

impl IntoElement for TableScrollViewport {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TableScrollViewport {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(gpui::SharedString::from(format!("table-horizontal-viewport-{:?}", self.table)).into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let scroll = window.with_element_state(
            id.expect("table viewport identity"),
            |state: Option<ScrollHandle>, _| {
                let scroll = state.unwrap_or_default();
                (scroll.clone(), scroll)
            },
        );
        *self.scroll.borrow_mut() = Some(scroll.clone());
        let wheel_scroll = scroll.clone();
        let view = self.view.clone();
        let table = self.table;
        let content = self.content.take().expect("one table viewport layout");
        let mut content = div()
            .id("horizontal-content")
            .debug_selector(move || format!("measured-table-viewport-{table:?}"))
            .my_3()
            .w(self.width)
            .min_w(self.width)
            .max_w(self.width)
            .flex_shrink_0()
            // Hidden overflow supplies clipping, but no stock wheel conversion
            // from y to x and no competing ancestor scroll listener.
            .overflow_x_hidden()
            .track_scroll(&scroll)
            .on_scroll_wheel(move |event, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                let Some(next) = horizontal_offset(
                    wheel_scroll.offset(),
                    wheel_scroll.max_offset().width,
                    delta,
                ) else {
                    return;
                };
                let _ = view.update(cx, |view, cx| {
                    view.column_resize.cancel();
                    cx.notify();
                });
                wheel_scroll.set_offset(next);
                // One owner per horizontal event. At either end, leave the
                // unchanged event available to the next containing viewport.
                cx.stop_propagation();
            })
            .child(content)
            .into_any_element();
        let layout = content.request_layout(window, cx);
        self.content = Some(content);
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(preview) = &self.preview {
            let scroll = self.scroll.borrow();
            let scroll = scroll.as_ref().expect("requested table scroll");
            if scroll.offset() == preview.observed.offset
                && scroll.bounds().origin == preview.observed.origin
                && scroll.bounds().size.width == preview.observed.width
                && bounds.origin == preview.original.origin
                && window.scale_factor() == preview.original.scale
            {
                let content = window
                    .layout_bounds(self.content_layout.get().expect("requested table layout"));
                // Use the real device-rounded child/viewport bounds, matching
                // stock GPUI's centipixel maximum calculation exactly.
                let maximum = ((content.size.width - bounds.size.width) * 100.0).round() / 100.0;
                scroll.set_offset(preview.original.projected_offset(maximum));
            } else {
                // Cancel the preview without overwriting the unrelated offset.
                let external_offset = scroll.offset();
                let _ = self.view.update(cx, |view, cx| {
                    view.column_resize.cancel();
                    cx.notify();
                });
                scroll.set_offset(external_offset);
            }
        }
        // This div shares the parent's Taffy tree and LayoutId. Unlike the
        // separately laid-out table cells, its origin must not be added twice.
        self.content.as_mut().unwrap().prepaint(window, cx);
        if let Some(scroll) = self.scroll.borrow().as_ref() {
            let _ = self.view.update(cx, |view, _| {
                view.reading
                    .borrow_mut()
                    .tables
                    .insert(self.table, scroll.clone());
            });
        }
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
        self.content.as_mut().unwrap().paint(window, cx);
    }
}

fn horizontal_offset(
    offset: Point<Pixels>,
    maximum: Pixels,
    delta: Point<Pixels>,
) -> Option<Point<Pixels>> {
    let (x, y, maximum) = (f32::from(delta.x), f32::from(delta.y), f32::from(maximum));
    // Match the editor's dominant-axis policy. Pure vertical wheels must never
    // become horizontal motion; a horizontal gesture does not move both axes.
    if !x.is_finite()
        || !y.is_finite()
        || !maximum.is_finite()
        || maximum <= 0.0
        || x.abs() <= y.abs()
    {
        return None;
    }
    let next = (offset.x + px(x)).clamp(px(-maximum), px(0.0));
    (next != offset.x).then_some(point(next, offset.y))
}

impl DocumentView {
    /// Full geometry stays intact; only pointer candidacy uses the table clip.
    pub(super) fn visible_table_bounds(
        &self,
        document: &XiaomuDocument,
        node: NodeId,
        bounds: Bounds<Pixels>,
    ) -> Option<Bounds<Pixels>> {
        let clips = self.table_clips.borrow();
        if clips.is_empty() {
            return Some(bounds);
        }
        let mut ancestor = Some(node);
        while let Some(node) = ancestor {
            if document
                .node(node)
                .is_some_and(|node| matches!(node.kind(), NodeKind::Table))
                && let Some(clip) = clips.get(&node)
            {
                let visible = bounds.intersect(clip);
                return (visible.size.width > px(0.0) && visible.size.height > px(0.0))
                    .then_some(visible);
            }
            ancestor = document.parent_of(node);
        }
        Some(bounds)
    }
}
