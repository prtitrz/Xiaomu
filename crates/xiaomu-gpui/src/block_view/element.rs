//! Custom element for the single-block view.
//!
//! The element shapes the block's displayed text into one line, paints the
//! selection highlight and caret (projected from the document-level
//! selection), registers the platform input handler, publishes painted
//! bounds to the document view's hit-test registry, and reuses the cached
//! shaped line while the layout cache key is unchanged.

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, FontStyle, FontWeight,
    GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, ShapedLine, SharedString,
    StrikethroughStyle, Style, TextRun, UnderlineStyle, Window, fill, point, px, relative, rgba,
    size,
};

use super::{ParagraphView, SelectionProjection};
use crate::document_view::cache_key::LayoutCacheKey;

/// Renders one block view's inline content.
pub struct ParagraphElement {
    pub(super) view: Entity<ParagraphView>,
}

/// Layout results computed during prepaint and consumed during paint.
///
/// This is an internal detail of the element pipeline; it is public only
/// because it appears as an associated type of the `Element` impl.
pub struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    cache_key: Option<LayoutCacheKey>,
}

impl IntoElement for ParagraphElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for ParagraphElement {
    type RequestLayoutState = ();
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
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let view = self.view.read(cx);
        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let color = style.color;

        // Composition transients bypass the cache: the virtual projection
        // changes without an epoch bump.
        let composing = view.is_composing();
        let cache_key = (!composing)
            .then(|| LayoutCacheKey::new(view.node(), view.epoch.get(), bounds.size.width.into()));
        let cached_hit = !composing && view.cache_key == cache_key && view.last_layout.is_some();

        let line: ShapedLine = if cached_hit {
            view.last_layout.clone().expect("checked above")
        } else {
            let (display_text, segments) = view.display_content();
            let runs = text_runs(&segments, font.clone(), color);
            window.text_system().shape_line(
                SharedString::new(display_text.as_ref()),
                font_size,
                &runs,
                None,
            )
        };

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

        // A non-collapsed highlight wins over the caret; the caret shows
        // only when this block holds the selection focus.
        let focused = self.view.read(cx).focus_handle.is_focused(window);
        let (selection, cursor) = match projection {
            SelectionProjection::Highlight { start, end } => (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(start), bounds.top()),
                        point(bounds.left() + line.x_for_index(end), bounds.bottom()),
                    ),
                    rgba(0x3377cc44),
                )),
                None,
            ),
            _ => match (focused, caret_byte) {
                (true, Some(caret)) => (
                    None,
                    Some(fill(
                        Bounds::new(
                            point(bounds.left() + line.x_for_index(caret), bounds.top()),
                            size(px(2.0), bounds.bottom() - bounds.top()),
                        ),
                        gpui::blue(),
                    )),
                ),
                _ => (None, None),
            },
        };

        PrepaintState {
            line: Some(line),
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

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        let line = prepaint.line.take().unwrap();
        if let Err(error) = line.paint(bounds.origin, window.line_height(), window, cx) {
            eprintln!("xiaomu: line paint failed: {error}");
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
            view.last_layout = Some(line);
            view.last_bounds = Some(bounds);
            view.cache_key = prepaint.cache_key;
        });
    }
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
