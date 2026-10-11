//! Width-driven image layout. Measurement never lays out children recursively.

use std::sync::Arc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, InteractiveElement, IntoElement, LayoutId, ParentElement, Pixels,
    RenderImage, Size, Style, Styled, StyledImage, Window, div, img, px, size,
};

/// Per-view image sizing; canonical image attributes are never rewritten.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageSizingPolicy {
    /// Existing full-width, contained presentation with a 320px height cap.
    #[default]
    FitContainer,
    /// Decoded intrinsic ratio with automatic height and local width clamping.
    ///
    /// A positive width hint requests a border-box width (and may upscale).
    /// Without it, intrinsic content width is used. Height hints are preserved
    /// but do not constrain display. A stable 1px border is included in layout.
    /// Until decoding succeeds, a 96px placeholder remains; this mode does not
    /// promise browser-equivalent pre-load ratio reservation or zero reflow.
    ResponsiveIntrinsic,
}

/// Returns paired border-box dimensions, including the stable one-pixel border.
fn responsive_size(natural: Size<Pixels>, width: Option<u32>, available: Pixels) -> Size<Pixels> {
    let natural_width = f64::from(f32::from(natural.width));
    let natural_height = f64::from(f32::from(natural.height));
    let available = f64::from(f32::from(available)).max(0.0);
    if !natural_width.is_finite()
        || !natural_height.is_finite()
        || natural_width <= 0.0
        || natural_height <= 0.0
    {
        return size(px(0.0), px(0.0));
    }
    let requested = width.map(f64::from).unwrap_or(natural_width + 2.0);
    let outer_width = requested.min(available);
    // At less than two pixels the border itself is clipped to the assigned box.
    let content_width = (outer_width - 2.0).max(0.0);
    let content_height = if natural_width > 0.0 {
        content_width * natural_height / natural_width
    } else {
        0.0
    };
    size(
        px(outer_width as f32),
        px((content_height + 2.0).min(f64::from(f32::MAX)) as f32),
    )
}

pub(super) struct ResponsiveImage {
    pub cache: super::SharedImageLoadCache,
    pub node: xiaomu_core::document::NodeId,
    pub source: Option<String>,
    pub width: Option<u32>,
    pub selected: bool,
    pub label: String,
    pub state_color: gpui::Rgba,
    pub selector: String,
}

impl IntoElement for ResponsiveImage {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for ResponsiveImage {
    type RequestLayoutState = Option<Arc<RenderImage>>;
    type PrepaintState = AnyElement;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        // PNG/JPEG Image decoding uses GPUI's scale-factor-one RenderImage.
        // Each fresh node request schedules its own view after async decode.
        let decoded = self.source.as_deref().and_then(|source| {
            super::decode::decoded_image(&self.cache, self.node, source, window, cx)
        });
        if decoded.as_ref().is_some_and(Result::is_err) {
            self.label = format!("Image decode failed: {}", self.label);
            self.state_color = gpui::rgba(0xf6d5d5ff);
        }
        let decoded = decoded.and_then(Result::ok);
        let natural = decoded
            .as_ref()
            .map(|image| image.size(0).map(|v| px(v.0 as f32)));
        let width = self.width;
        let style = Style {
            min_size: size(px(0.0).into(), px(0.0).into()),
            flex_shrink: 0.0,
            ..Style::default()
        };
        let layout = window.request_measured_layout(style, move |known, available, _, _| {
            let available = known
                .width
                .or(match available.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                })
                .unwrap_or_else(|| natural.map_or(px(96.0), |natural| natural.width + px(2.0)));
            natural.map_or(size(available.max(px(0.0)), px(96.0)), |natural| {
                responsive_size(natural, width, available)
            })
        });
        (layout, decoded)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        decoded: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let selector = self.selector.clone();
        let mut child = div()
            .debug_selector(move || selector.clone())
            .w(bounds.size.width)
            .h(bounds.size.height)
            .overflow_hidden()
            .border_1()
            .border_x(bounds.size.width.min(px(2.0)) / 2.0)
            .border_color(if self.selected {
                gpui::rgba(0x2b6cb8ff)
            } else {
                gpui::rgba(0xccccccff)
            });
        if let Some(image) = decoded {
            child = child.child(
                img(image.clone())
                    .w((bounds.size.width - px(2.0)).max(px(0.0)))
                    .h((bounds.size.height - px(2.0)).max(px(0.0)))
                    .object_fit(gpui::ObjectFit::Contain),
            );
        } else {
            child = child.bg(self.state_color).child(
                div()
                    .px_3()
                    .py_2()
                    .child(self.label.clone())
                    .text_size(px(14.0))
                    .text_color(gpui::rgba(0x555555ff)),
            );
        }
        let mut child = child.into_any_element();
        child.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        child.prepaint_at(bounds.origin, window, cx);
        child
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        child: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests;
