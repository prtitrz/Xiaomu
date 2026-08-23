//! Custom element for the single-paragraph view.
//!
//! The element shapes the paragraph's inline runs into one line, paints the
//! selection highlight and caret, registers the platform input handler, and
//! stores the layout so the view can hit-test mouse positions.

use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, FontStyle, FontWeight,
    GlobalElementId, IntoElement, LayoutId, PaintQuad, Pixels, ShapedLine, SharedString,
    StrikethroughStyle, Style, TextRun, UnderlineStyle, Window, fill, point, px, relative, rgba,
    size,
};

use xiaomu_core::document::MarkKind;

use super::ParagraphView;

/// Renders one paragraph view's inline content.
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
}

impl IntoElement for ParagraphElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
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
        let session = view.session();
        let selection = session.selection();
        let focus = selection.focus().offset().as_usize();
        let anchor = selection.anchor().offset().as_usize();

        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let color = style.color;

        let (display_text, runs) = paragraph_runs(view, &font, color);
        let line = window.text_system().shape_line(
            SharedString::new(display_text.as_ref()),
            font_size,
            &runs,
            None,
        );

        let (selection, cursor) = if anchor == focus {
            let caret_x = line.x_for_index(focus);
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + caret_x, bounds.top()),
                        size(px(2.0), bounds.bottom() - bounds.top()),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            let (start, end) = (anchor.min(focus), anchor.max(focus));
            (
                Some(fill(
                    Bounds::from_corners(
                        point(bounds.left() + line.x_for_index(start), bounds.top()),
                        point(bounds.left() + line.x_for_index(end), bounds.bottom()),
                    ),
                    rgba(0x3377cc44),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
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

        self.view.update(cx, |view, _| {
            view.last_layout = Some(line);
            view.last_bounds = Some(bounds);
        });
    }
}

/// Returns the concatenated text and mark-aware style runs of the view's
/// inline node.
fn paragraph_runs(
    view: &ParagraphView,
    base_font: &gpui::Font,
    color: gpui::Hsla,
) -> (String, Vec<TextRun>) {
    let Some(inline) = view
        .session()
        .document()
        .node(view.node())
        .and_then(|node| node.content().as_inline())
    else {
        return (String::new(), Vec::new());
    };

    let text: String = inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    if text.is_empty() {
        return (String::new(), Vec::new());
    }

    let runs = inline
        .runs()
        .iter()
        .map(|run| {
            let marks = run.marks();
            let mut font = base_font.clone();
            if marks.contains(MarkKind::Bold) {
                font.weight = FontWeight::BOLD;
            }
            if marks.contains(MarkKind::Italic) {
                font.style = FontStyle::Italic;
            }
            let underline = marks
                .contains(MarkKind::Underline)
                .then_some(UnderlineStyle {
                    color: Some(color),
                    thickness: px(1.0),
                    wavy: false,
                });
            let strikethrough = marks
                .contains(MarkKind::Strike)
                .then_some(StrikethroughStyle {
                    color: Some(color),
                    thickness: px(1.0),
                });
            TextRun {
                len: run.len_bytes(),
                font,
                color,
                background_color: None,
                underline,
                strikethrough,
            }
        })
        .collect();

    (text, runs)
}
