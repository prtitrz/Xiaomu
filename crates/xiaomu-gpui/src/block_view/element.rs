//! Custom element for one inline-bearing block.
//!
//! P3.1 upgrades the P2 single-`ShapedLine` path to GPUI's measured
//! `WrappedLine` layout. Soft-wrap stays entirely in the frontend: canonical
//! byte positions are projected into visual rows for caret, selection and
//! pointer geometry.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, Element, ElementId, ElementInputHandler, Entity, FontStyle,
    FontWeight, GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, SharedString, Size,
    StrikethroughStyle, Style, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px,
    relative, rgba, size,
};

use super::layout::BlockTextLayout;
use super::{ParagraphView, SelectionProjection};
use crate::document_view::cache_key::LayoutCacheKey;

/// Renders one block view's inline content.
pub struct ParagraphElement {
    pub(super) view: Entity<ParagraphView>,
}

/// Measured block layout shared between GPUI's layout and prepaint phases.
#[derive(Clone, Default)]
pub struct RequestLayoutState(Rc<RefCell<Option<BlockTextLayout>>>);

/// Layout results computed during prepaint and consumed during paint.
///
/// This is an internal detail of the element pipeline; it is public only
/// because it appears as an associated type of the `Element` impl.
pub struct PrepaintState {
    layout: Option<BlockTextLayout>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    cache_key: Option<LayoutCacheKey>,
}

impl IntoElement for ParagraphElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ParagraphElement {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let view = self.view.read(cx);
        let (display_text, segments) = view.display_content();
        let composing = view.is_composing();
        let cached_layout = (!composing).then(|| view.last_layout.clone()).flatten();
        let cached_key = (!composing).then_some(view.cache_key).flatten();
        let node = view.node();
        let epoch = view.epoch.get();

        let text_style = window.text_style();
        let font = text_style.font();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let color = text_style.color;
        let line_height = window.line_height();
        let runs = text_runs(&segments, font, color);
        let text = SharedString::new(display_text.as_ref());

        let mut style = Style::default();
        style.size.width = relative(1.0).into();

        let state = RequestLayoutState::default();
        let measured_state = state.clone();
        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, window, _cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });

                let cache_key =
                    wrap_width.map(|width| LayoutCacheKey::new(node, epoch, f32::from(width)));
                if !composing
                    && cache_key == cached_key
                    && let Some(layout) = cached_layout.as_ref()
                {
                    measured_state.0.borrow_mut().replace(layout.clone());
                    return measured_size(layout, wrap_width);
                }

                let layout = match window.text_system().shape_text(
                    text.clone(),
                    font_size,
                    &runs,
                    wrap_width,
                    None,
                ) {
                    Ok(lines) => BlockTextLayout::new(lines.into_iter().collect(), line_height),
                    Err(error) => {
                        eprintln!("xiaomu: wrapped text layout failed: {error}");
                        BlockTextLayout::new(Vec::new(), line_height)
                    }
                };
                let size = measured_size(&layout, wrap_width);
                measured_state.0.borrow_mut().replace(layout);
                size
            },
        );
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let view = self.view.read(cx);
        let composing = view.is_composing();
        let cache_key = (!composing).then(|| {
            LayoutCacheKey::new(view.node(), view.epoch.get(), f32::from(bounds.size.width))
        });
        let layout = request_layout
            .0
            .borrow()
            .clone()
            .unwrap_or_else(|| BlockTextLayout::new(Vec::new(), window.line_height()));

        let caret_byte = view.composing_caret_byte().or_else(|| view.focus_byte());
        let projection = if composing {
            SelectionProjection::None
        } else {
            use crate::document_view::navigation;
            let order: Vec<_> = {
                let session = view.session().borrow();
                navigation::text_blocks(session.document())
                    .into_iter()
                    .map(|block| block.node)
                    .collect()
            };
            view.projected_selection(&order)
        };

        let focused = view.focus_handle.is_focused(window);
        let selection = match projection {
            SelectionProjection::Highlight { start, end } => layout
                .selection_rects(start..end)
                .into_iter()
                .map(|rect| {
                    fill(
                        Bounds::new(
                            point(bounds.left() + rect.origin.x, bounds.top() + rect.origin.y),
                            rect.size,
                        ),
                        rgba(0x3377cc44),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };

        let cursor = if focused && selection.is_empty() {
            caret_byte.and_then(|caret| {
                layout.position_for_index(caret).map(|position| {
                    fill(
                        Bounds::new(
                            point(bounds.left() + position.x, bounds.top() + position.y),
                            size(px(2.0), layout.line_height()),
                        ),
                        gpui::blue(),
                    )
                })
            })
        } else {
            None
        };

        PrepaintState {
            layout: Some(layout),
            cursor,
            selection,
            cache_key,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.view.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let layout = prepaint
            .layout
            .take()
            .unwrap_or_else(|| BlockTextLayout::new(Vec::new(), window.line_height()));
        let mut origin = bounds.origin;
        for line in layout.lines() {
            if let Err(error) = line.paint(
                origin,
                layout.line_height(),
                TextAlign::default(),
                Some(bounds),
                window,
                cx,
            ) {
                eprintln!("xiaomu: wrapped line paint failed: {error}");
            }
            origin.y += line.size(layout.line_height()).height;
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let node_id = self.view.read(cx).node();
        let registry = self.view.read(cx).bounds_registry.clone();
        registry.borrow_mut().push((node_id, bounds));

        self.view.update(cx, |view, _| {
            view.last_layout = Some(layout);
            view.last_bounds = Some(bounds);
            view.cache_key = prepaint.cache_key;
        });
    }
}

fn measured_size(layout: &BlockTextLayout, wrap_width: Option<Pixels>) -> Size<Pixels> {
    let mut measured = layout.size();
    if let Some(width) = wrap_width {
        measured.width = width;
    }
    measured
}

fn text_runs(
    segments: &[super::DisplaySegment],
    font: gpui::Font,
    color: gpui::Hsla,
) -> Vec<TextRun> {
    segments
        .iter()
        .map(|segment| {
            let mut run_font = font.clone();
            if segment.bold {
                run_font.weight = FontWeight::BOLD;
            }
            if segment.italic {
                run_font.style = FontStyle::Italic;
            }
            // Only the preedit segment carries an explicit underline;
            // canonical segments keep their own mark styling.
            let underline = segment.underline.then_some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.0),
                wavy: false,
            });
            TextRun {
                len: segment.text.len(),
                font: run_font,
                color,
                background_color: segment.code.then_some(rgba(0x00000012).into()),
                underline,
                strikethrough: segment.strike.then_some(StrikethroughStyle {
                    color: Some(color),
                    thickness: px(1.0),
                }),
            }
        })
        .collect()
}
