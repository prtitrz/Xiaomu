//! A single native `shape_text` result with shared geometry and decorations.
//!
//! Stock GPUI 0.2.2's `shape_line` can merge a new font into a preceding run
//! when decorations match. `shape_text` resolves each run's font before merging.
//! Every mixed admission/row shape uses this same public, unwrapped primitive.
//! The geometry handle below shares the returned Arc; it performs no extra shape.

use super::{Reason, Unsupported};
use gpui::{
    App, Pixels, Point, ShapedLine, SharedString, TextAlign, TextRun, Window, WindowTextSystem,
    WrappedLine,
};
use std::ops::{Deref, Range};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct NativeLine {
    geometry: ShapedLine,
    paint: WrappedLine,
}

impl NativeLine {
    pub(super) fn shape(
        system: &WindowTextSystem,
        text: SharedString,
        size: Pixels,
        runs: &[TextRun],
        range: Range<usize>,
    ) -> Result<Self, Unsupported> {
        let mut lines = system
            .shape_text(text.clone(), size, runs, None, None)
            .map_err(|_| Unsupported::at(range.clone(), Reason::NativeShapeFailed))?;
        if lines.len() != 1 || lines[0].text != text || !lines[0].wrap_boundaries.is_empty() {
            return Err(Unsupported::at(range, Reason::NativeShapeFailed));
        }
        let paint = lines.pop().expect("one validated native line");
        let mut geometry = ShapedLine::default();
        geometry.text = paint.text.clone();
        *geometry = Arc::clone(&paint.unwrapped_layout);
        Ok(Self { geometry, paint })
    }

    pub(super) fn paint(
        &self,
        origin: Point<Pixels>,
        height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Result<()> {
        self.paint
            .paint(origin, height, TextAlign::Left, None, window, cx)
    }

    pub(super) fn paint_background(
        &self,
        origin: Point<Pixels>,
        height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Result<()> {
        self.paint
            .paint_background(origin, height, TextAlign::Left, None, window, cx)
    }

    #[cfg(test)]
    pub(super) fn synthetic(geometry: ShapedLine) -> Self {
        let mut paint = WrappedLine::default();
        paint.text = geometry.text.clone();
        *paint = Arc::new(gpui::WrappedLineLayout {
            unwrapped_layout: Arc::clone(&geometry),
            wrap_boundaries: Default::default(),
            wrap_width: None,
        });
        Self { geometry, paint }
    }
}

impl Deref for NativeLine {
    type Target = ShapedLine;
    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{FontStyle, FontWeight, TestAppContext, font, px};

    #[gpui::test]
    fn mixed_font_only_transitions_keep_authoritative_shape_text_geometry_and_paint(
        cx: &mut TestAppContext,
    ) {
        cx.add_empty_window().update(|window, _| {
            let mut bold = font(".SystemUIFont");
            bold.weight = FontWeight::BOLD;
            let mut italic = font("monospace");
            italic.style = FontStyle::Italic;
            let runs: Vec<_> = [font(".SystemUIFont"), bold, italic]
                .into_iter()
                .map(|font| TextRun {
                    len: 2,
                    font,
                    color: gpui::rgba(0x123456ff).into(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            let line = NativeLine::shape(
                window.text_system(),
                "abcdef".into(),
                px(24.0),
                &runs,
                10..16,
            )
            .unwrap();
            // Virtual fonts collapse IDs, so do not pretend this proves actual
            // font faces. It proves geometry and paint share shape_text's Arc,
            // and the production primitive preserves identical decorations.
            assert!(Arc::ptr_eq(&line.geometry, &line.paint.unwrapped_layout));
            assert_eq!(line.text.as_ref(), "abcdef");
            assert_eq!(line.font_size, px(24.0));
            assert!(line.paint.wrap_boundaries.is_empty());
            assert_eq!(line.paint.wrap_width, None);
        });
    }
}
