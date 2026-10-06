//! Custom element for one inline-bearing block.
//!
//! P3.1 upgrades the P2 single-`ShapedLine` path to GPUI's measured
//! `WrappedLine` layout. Soft-wrap stays entirely in the frontend: canonical
//! byte positions are projected into visual rows for caret, selection and
//! pointer geometry.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, AvailableSpace, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    IntoElement, LayoutId, PaintQuad, Pixels, SharedString, Size, Style, Window, fill, point, px,
    relative, rgba, size,
};
use xiaomu_core::selection::CursorAffinity;

use super::layout::BlockTextLayout;
use super::text_style::{BlockTextStyle, FontCatalog, block_text_style, text_runs};
use super::{ParagraphView, SelectionProjection};
use crate::document_view::cache_key::{LayoutCacheKey, style_fingerprint};

#[cfg(test)]
#[path = "link_style_tests.rs"]
mod link_style_tests;
#[cfg(test)]
#[path = "element_tests.rs"]
mod tests;

/// Renders one block view's inline content.
pub struct ParagraphElement {
    pub(super) view: Entity<ParagraphView>,
}

/// Measured block layout shared between GPUI's layout and prepaint phases.
#[derive(Clone, Default)]
pub struct RequestLayoutState(Rc<RefCell<Option<BlockTextLayout>>>, u64, Pixels);

/// Layout results computed during prepaint and consumed during paint.
///
/// This is an internal detail of the element pipeline; it is public only
/// because it appears as an associated type of the `Element` impl.
pub struct PrepaintState {
    layout: Option<BlockTextLayout>,
    cursor: Option<PaintQuad>,
    caret_geometry: Option<(usize, CursorAffinity, Bounds<Pixels>)>,
    chips: Vec<PaintQuad>,
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
        let (display_text, segments) = view.layout_content();
        let composing = view.is_composing();
        let cached_layout = (!composing).then(|| view.last_layout.clone()).flatten();
        let cached_key = (!composing).then_some(view.cache_key).flatten();
        let node = view.node();
        let epoch = view.epoch.get();
        let alignment = view.effective_block_alignment();

        let fonts = FontCatalog::from_system(window.text_system());
        let BlockTextStyle {
            mut font,
            mut font_size,
            mut color,
            mut line_height,
        } = block_text_style(window, view.active_code_presentation(), &fonts);
        let sized = view.sized_content(&segments);
        if let Ok(Some(content)) = &sized {
            font = content.style.base_font().clone();
            color = content.style.color();
            font_size = px(content.style.context().parent_px());
            line_height = font_size * content.style.line_height();
        }
        let runs = match &sized {
            Ok(Some(content)) => content.resolved.runs.clone(),
            _ => text_runs(&segments, font.clone(), color, &fonts),
        };
        let sized_enabled = view.text_size_capability.is_some() && !view.is_range_input();
        let aligned_decorations = alignment
            .is_some_and(|alignment| alignment != crate::block_alignment::BlockAlignment::Left)
            && runs
                .iter()
                .any(|run| run.underline.is_some() || run.strikethrough.is_some());
        let fingerprint =
            style_fingerprint(&display_text, &font, color, font_size, line_height, &runs);
        let fingerprint = match &sized {
            Ok(Some(content)) => content.fingerprint(fingerprint),
            _ => fingerprint,
        };
        let text = SharedString::new(display_text.as_ref());

        let mut style = Style::default();
        style.size.width = relative(1.0).into();

        let state = RequestLayoutState(Rc::new(RefCell::new(None)), fingerprint, line_height);
        let measured_state = state.clone();
        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, window, _cx| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });

                let cache_key = wrap_width.map(|width| {
                    LayoutCacheKey::new(node, epoch, f32::from(width))
                        .with_style(fingerprint)
                        .with_alignment(alignment, width)
                        .with_text_sizes(sized_enabled, width)
                });
                // None is not a cache identity: intrinsic width probes have
                // no key, and a painted preedit deliberately has no key too.
                // Treating None == None as a hit resurrects cancelled preedit.
                if !composing
                    && sized.is_ok()
                    && cache_key.is_some()
                    && cache_key == cached_key
                    && let Some(layout) = cached_layout
                        .as_ref()
                        .filter(|layout| layout.is_available())
                {
                    measured_state.0.borrow_mut().replace(layout.clone());
                    return measured_size(layout, wrap_width);
                }

                let layout = match &sized {
                    Ok(Some(content)) => match crate::mixed_size::layout(
                        content.capability.text_system(),
                        content.input(&text, wrap_width.unwrap_or(px(f32::MAX)).max(px(0.01))),
                    ) {
                        Ok(layout) => BlockTextLayout::from_sized(layout),
                        Err(error) => {
                            eprintln!("xiaomu: unsupported text-size layout: {error:?}");
                            BlockTextLayout::unavailable(line_height)
                        }
                    },
                    Err(error) => {
                        eprintln!("xiaomu: unsupported text-size input: {error}");
                        BlockTextLayout::unavailable(line_height)
                    }
                    Ok(None) => {
                        match window.text_system().shape_text(
                            text.clone(),
                            font_size,
                            &runs,
                            wrap_width,
                            None,
                        ) {
                            Ok(lines) => {
                                BlockTextLayout::new(lines.into_iter().collect(), line_height)
                            }
                            Err(error) => {
                                eprintln!("xiaomu: wrapped text layout failed: {error}");
                                BlockTextLayout::new(Vec::new(), line_height)
                            }
                        }
                    }
                };
                let width = wrap_width.unwrap_or(layout.size().width);
                let mut layout = layout.with_alignment(alignment, width);
                if aligned_decorations && !layout.paint_lines().is_empty() {
                    let mut plain = runs.clone();
                    for run in &mut plain {
                        run.underline = None;
                        run.strikethrough = None;
                    }
                    match window.text_system().shape_text(
                        text.clone(),
                        layout.sized_font_size().unwrap_or(font_size),
                        &plain,
                        wrap_width,
                        None,
                    ) {
                        Ok(carrier) => {
                            layout = layout.with_decoration_carrier(
                                carrier.into_iter().collect(),
                                runs.clone(),
                            )
                        }
                        Err(error) => {
                            eprintln!("xiaomu: aligned decoration layout failed: {error}");
                            layout = if sized_enabled {
                                BlockTextLayout::unavailable(line_height)
                            } else {
                                BlockTextLayout::new(Vec::new(), line_height)
                            }
                            .with_alignment(alignment, width);
                        }
                    }
                }
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
                .with_style(request_layout.1)
                .with_alignment(view.effective_block_alignment(), bounds.size.width)
                .with_text_sizes(
                    view.text_size_capability.is_some() && !view.is_range_input(),
                    bounds.size.width,
                )
        });
        let mut layout = request_layout
            .0
            .borrow()
            .clone()
            .unwrap_or_else(|| BlockTextLayout::new(Vec::new(), request_layout.2))
            .with_alignment(view.effective_block_alignment(), bounds.size.width);

        let caret = view
            .composing_caret_byte()
            .map(|byte| (byte, CursorAffinity::Before))
            .or_else(|| view.display_focus_caret());
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
            view.projected_display_selection(&order)
        };

        let focused = view.focus_handle.is_focused(window);
        let caret_height = if focused {
            view.presented_caret_height()
        } else {
            Ok(None)
        };
        if let Err(error) = &caret_height {
            eprintln!("xiaomu: unsupported text-size caret: {error}");
            layout = BlockTextLayout::unavailable(request_layout.2);
        }
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

        // Decorations follow the preedit splice, just like text and caret.
        let chips = view
            .layout_atom_ranges()
            .into_iter()
            .flat_map(|range| layout.selection_rects(range))
            .map(|rect| {
                fill(
                    Bounds::new(
                        point(bounds.left() + rect.origin.x, bounds.top() + rect.origin.y),
                        rect.size,
                    ),
                    rgba(0x7755aa30),
                )
            })
            .collect();

        let caret_geometry = if focused {
            caret.and_then(|(byte, affinity)| {
                let mut rect = layout.caret_rect(byte, affinity, px(2.0))?;
                if let Ok(Some(height)) = caret_height {
                    rect.origin.y += (rect.size.height - height) / 2.0;
                    rect.size.height = height;
                }
                Some((byte, affinity, rect))
            })
        } else {
            None
        };
        let caret_bounds = caret_geometry.map(|(_, _, mut rect)| {
            rect.origin += bounds.origin;
            rect
        });

        if let Some(caret_bounds) = caret_bounds.as_ref() {
            view.keep_caret_visible(caret_bounds, window);
        }

        let cursor = if selection.is_empty() {
            caret_bounds.map(|bounds| fill(bounds, gpui::blue()))
        } else {
            None
        };

        PrepaintState {
            layout: Some(layout),
            cursor,
            caret_geometry,
            chips,
            selection,
            cache_key,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
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

        for chip in prepaint.chips.drain(..) {
            window.paint_quad(chip);
        }

        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }

        let layout = prepaint
            .layout
            .take()
            .unwrap_or_else(|| BlockTextLayout::new(Vec::new(), request_layout.2));
        let mut origin = bounds.origin;
        for line in layout.paint_lines() {
            if let Err(error) = line.paint(
                origin,
                layout.line_height(),
                layout.alignment().text_align(),
                Some(Bounds::new(
                    bounds.origin,
                    size(layout.alignment_width(), bounds.size.height),
                )),
                window,
                cx,
            ) {
                eprintln!("xiaomu: wrapped line paint failed: {error}");
            }
            origin.y += line.size(layout.line_height()).height;
        }
        layout.paint_aligned_decorations(bounds.origin, window);
        if let Err(error) = layout.paint_mixed(bounds.origin, window, cx) {
            eprintln!("xiaomu: mixed-size paint failed: {error}");
        }

        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let node_id = self.view.read(cx).node();
        let registry = self.view.read(cx).bounds_registry.clone();
        if !self.view.read(cx).is_range_input() {
            registry.borrow_mut().push((node_id, bounds));
        }

        let changed_ime_coordinates = self.view.update(cx, |view, cx| {
            if !layout.is_available()
                && view
                    .last_layout
                    .as_ref()
                    .is_none_or(BlockTextLayout::is_available)
            {
                use crate::document_view::{
                    EditorRejection, EditorRejectionReason, EditorRejectionStage,
                };
                let revision = view.session.borrow().document().revision();
                cx.emit(EditorRejection::new(
                    EditorRejectionStage::TextSizeLayout,
                    EditorRejectionReason::UnsupportedTextSize,
                    revision,
                ));
            }
            view.last_layout = Some(layout);
            view.last_caret = prepaint.caret_geometry;
            view.last_bounds = Some(bounds);
            view.cache_key = view
                .last_layout
                .as_ref()
                .is_some_and(BlockTextLayout::is_available)
                .then_some(prepaint.cache_key)
                .flatten();
            view.ime_coordinates_changed(bounds, window, cx)
        });
        if changed_ime_coordinates {
            // The current input handler is registered for this frame and its
            // painted layout is published.
            // Stock GPUI defers its query; no nested view borrow or forced draw.
            window.invalidate_character_coordinates();
        }
    }
}

fn measured_size(layout: &BlockTextLayout, wrap_width: Option<Pixels>) -> Size<Pixels> {
    let mut measured = layout.size();
    if let Some(width) = wrap_width {
        measured.width = width;
    }
    measured
}

#[cfg(test)]
#[path = "code_presentation_tests.rs"]
mod code_presentation_tests;
