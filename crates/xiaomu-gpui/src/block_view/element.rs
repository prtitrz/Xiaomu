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
        let alignment = view.block_alignment;

        let fonts = FontCatalog::from_system(window.text_system());
        let BlockTextStyle {
            font,
            font_size,
            color,
            line_height,
        } = block_text_style(window, view.active_code_presentation(), &fonts);
        let runs = text_runs(&segments, font.clone(), color, &fonts);
        let aligned_decorations = alignment
            .is_some_and(|alignment| alignment != crate::block_alignment::BlockAlignment::Left)
            && runs
                .iter()
                .any(|run| run.underline.is_some() || run.strikethrough.is_some());
        let fingerprint =
            style_fingerprint(&display_text, &font, color, font_size, line_height, &runs);
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
                });
                // None is not a cache identity: intrinsic width probes have
                // no key, and a painted preedit deliberately has no key too.
                // Treating None == None as a hit resurrects cancelled preedit.
                if !composing
                    && cache_key.is_some()
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
                        font_size,
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
                            layout = BlockTextLayout::new(Vec::new(), line_height)
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
                .with_alignment(view.block_alignment, bounds.size.width)
        });
        let layout = request_layout
            .0
            .borrow()
            .clone()
            .unwrap_or_else(|| BlockTextLayout::new(Vec::new(), request_layout.2))
            .with_alignment(view.block_alignment, bounds.size.width);

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

        let caret_bounds = if focused {
            caret.and_then(|(byte, affinity)| {
                layout.position_for_caret(byte, affinity).map(|position| {
                    Bounds::new(
                        point(bounds.left() + position.x, bounds.top() + position.y),
                        size(px(2.0), layout.line_height()),
                    )
                })
            })
        } else {
            None
        };

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
            view.last_layout = Some(layout);
            view.last_bounds = Some(bounds);
            view.cache_key = prepaint.cache_key;
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
mod code_presentation_tests {
    use super::*;
    use crate::block_view::SharedSession;
    use crate::code_presentation::CodeBlockPresentation;
    use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
    use std::cell::Cell;
    use xiaomu_core::{
        document::{
            InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
            TextRun, XiaomuDocument,
        },
        selection::InlinePoint,
    };
    use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

    const SOURCE: &str = "A\t中\r\nZ\n";

    fn open(
        cx: &mut TestAppContext,
        kind: NodeKind,
        presentation: Option<CodeBlockPresentation>,
    ) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let node = builder
            .insert(
                kind,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(SOURCE, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([node]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let session = Rc::new(RefCell::new(
            DocumentSession::new(
                document,
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
            )
            .unwrap(),
        ));
        let handle = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                cx.new(|cx| {
                    let mut view = ParagraphView::new(
                        session.clone(),
                        Rc::new(Cell::new(0)),
                        Rc::new(RefCell::new(Vec::new())),
                        node,
                        cx,
                    );
                    view.set_code_block_presentation(presentation);
                    window.focus(&view.focus_handle);
                    view
                })
            })
            .unwrap()
        });
        handle
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.background_executor.run_until_parked();
        (handle, session, node)
    }

    #[gpui::test]
    fn code_style_and_padding_are_opt_in_and_do_not_leak_to_ordinary_blocks(
        cx: &mut TestAppContext,
    ) {
        for (kind, presentation, enabled) in [
            (NodeKind::CodeBlock, None, false),
            (
                NodeKind::Paragraph,
                Some(CodeBlockPresentation::default()),
                false,
            ),
            (
                NodeKind::CodeBlock,
                Some(CodeBlockPresentation::default()),
                true,
            ),
        ] {
            let (handle, session, _) = open(cx, kind, presentation);
            let before = session.borrow().document().clone();
            handle
                .update(cx, |view, window, _| {
                    let inherited = window.text_style();
                    let body_size = inherited.font_size.to_pixels(window.rem_size());
                    let layout = view.last_layout.as_ref().unwrap();
                    let bounds = view.last_bounds.unwrap();
                    assert_eq!(bounds.left(), px(if enabled { 17.0 } else { 0.0 }));
                    assert_eq!(bounds.top(), px(if enabled { 15.0 } else { 0.0 }));
                    let code_size = body_size * CodeBlockPresentation::FONT_SCALE;
                    assert_eq!(
                        layout.lines()[0].font_size(),
                        if enabled { code_size } else { body_size }
                    );
                    assert_eq!(
                        layout.line_height(),
                        if enabled {
                            body_size * CodeBlockPresentation::LINE_HEIGHT
                        } else {
                            window.line_height()
                        }
                    );
                    // GPUI splits LF into logical lines; the source CR/tab bytes
                    // remain literal and trailing LF still has its empty row.
                    let shaped = layout
                        .lines()
                        .iter()
                        .map(|line| line.text.as_ref())
                        .collect::<Vec<_>>()
                        .join("\n");
                    assert_eq!(shaped, SOURCE);
                    assert_eq!(view.layout_content().0, SOURCE);
                })
                .unwrap();
            assert_eq!(session.borrow().document().store(), before.store());
            assert_eq!(session.borrow().history_depths(), (0, 0));
        }
    }

    #[gpui::test]
    fn code_preedit_paint_candidate_and_hit_test_use_the_same_padded_layout(
        cx: &mut TestAppContext,
    ) {
        let (handle, session, _) = open(
            cx,
            NodeKind::CodeBlock,
            Some(CodeBlockPresentation::default()),
        );
        let original = session.borrow().document().clone();
        let selection = session.borrow().selection();
        handle
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "中文🙂", Some(4..4), window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |view, window, cx| {
                assert_eq!(view.layout_content().0, format!("中文🙂{SOURCE}"));
                assert!(
                    view.cache_key.is_none(),
                    "preedit is not a reusable source cache"
                );
                let bounds = view.last_bounds.unwrap();
                let layout = view.last_layout.as_ref().unwrap();
                let line_height = layout.line_height();
                let caret_byte = view.composing_caret_byte().unwrap();
                let expected = layout.position_for_index(caret_byte).unwrap();
                let candidate = view.bounds_for_range(4..4, bounds, window, cx).unwrap();
                assert_eq!(candidate.origin, bounds.origin + expected);
                assert_eq!(candidate.size.height, line_height);
                assert_eq!(
                    view.character_index_for_point(candidate.origin, window, cx),
                    Some(4)
                );
                assert_eq!(view.bounds_registry.borrow().last().unwrap().1, bounds);
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |view, _, _| {
                assert_eq!(view.layout_content().0, SOURCE);
                assert!(view.cache_key.is_some());
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), original.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }

    #[gpui::test]
    fn code_theme_changes_reshape_cached_runs_without_document_edits(cx: &mut TestAppContext) {
        let (handle, session, _) = open(
            cx,
            NodeKind::CodeBlock,
            Some(CodeBlockPresentation::default()),
        );
        let before = session.borrow().document().clone();
        let first = handle
            .update(cx, |view, _, _| view.cache_key.unwrap())
            .unwrap();
        let presentation = CodeBlockPresentation {
            text_color: Some(rgba(0x112233ff).into()),
            ..Default::default()
        };
        handle
            .update(cx, |view, _, cx| {
                view.set_code_block_presentation(Some(presentation.clone()));
                assert!(view.cache_key.is_none());
                assert!(view.last_layout.is_none());
                cx.notify();
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |view, window, _| {
                assert_ne!(view.cache_key.unwrap(), first);
                assert_eq!(view.epoch.get(), 0);
                let fonts = FontCatalog::from_system(window.text_system());
                let style = block_text_style(window, view.active_code_presentation(), &fonts);
                assert_eq!(style.color, presentation.text_color.unwrap());
                let runs = text_runs(
                    &view.layout_content().1,
                    style.font.clone(),
                    style.color,
                    &fonts,
                );
                assert!(
                    runs.iter()
                        .all(|run| run.font == style.font && run.color == style.color)
                );
                let same = view.cache_key;
                view.set_code_block_presentation(Some(presentation));
                assert_eq!(
                    view.cache_key, same,
                    "same config does not discard source layout"
                );
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}
