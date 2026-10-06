//! Repair stock GPUI's wrapped decoration origins for opt-in aligned text.
//!
//! Glyph painting still uses stock WrappedLine. This only paints the original
//! run decorations against the exact same original glyph clusters and rows.

use gpui::{Pixels, Point, StrikethroughStyle, UnderlineStyle, Window, point};

use super::BlockTextLayout;

#[cfg(test)]
#[path = "aligned_decoration_tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq)]
enum Decoration {
    Underline(UnderlineStyle),
    Strike(StrikethroughStyle),
}

#[derive(Clone, Debug)]
pub(super) struct Stroke {
    row: usize,
    origin: Point<Pixels>,
    width: Pixels,
    decoration: Decoration,
}

impl BlockTextLayout {
    pub(in crate::block_view) fn paint_aligned_decorations(
        &self,
        origin: Point<Pixels>,
        window: &mut Window,
    ) {
        for stroke in &self.decorations {
            let origin = origin + stroke.origin + point(self.rows[stroke.row].x, Pixels::ZERO);
            match &stroke.decoration {
                Decoration::Underline(style) => {
                    window.paint_underline(origin, stroke.width, style);
                }
                Decoration::Strike(style) => {
                    window.paint_strikethrough(origin, stroke.width, style);
                }
            }
        }
    }

    pub(super) fn decoration_strokes(&self) -> Vec<Stroke> {
        if self.decoration_runs.is_empty() {
            return Vec::new();
        }
        let mut end = 0;
        let runs: Vec<_> = self
            .decoration_runs
            .iter()
            .map(|run| {
                let start = end;
                end += run.len;
                (start..end, run)
            })
            .collect();
        let mut underlines = Vec::new();
        let mut strikes = Vec::new();
        let mut row_ix = 0;
        for line in &self.lines {
            let glyphs = line.runs().iter().enumerate().flat_map(|(run_ix, run)| {
                run.glyphs
                    .iter()
                    .enumerate()
                    .map(move |(glyph_ix, glyph)| ((run_ix, glyph_ix), glyph))
            });
            let mut wraps = line.wrap_boundaries().iter().peekable();
            let padding = (self.line_height - line.ascent() - line.descent()) / 2.0;
            let baseline = padding + line.ascent();
            let mut underline = None;
            let mut strike = None;
            let mut style_ix =
                runs.partition_point(|(range, _)| range.end <= self.rows[row_ix].logical_start);
            for ((run_ix, glyph_ix), glyph) in glyphs {
                if wraps
                    .peek()
                    .is_some_and(|wrap| wrap.run_ix == run_ix && wrap.glyph_ix == glyph_ix)
                {
                    let end_x = glyph.position.x - self.rows[row_ix].start_x;
                    finish(&mut underlines, &mut underline, end_x);
                    finish(&mut strikes, &mut strike, end_x);
                    row_ix += 1;
                    wraps.next();
                }
                let row = &self.rows[row_ix];
                let byte = row.logical_start + glyph.index;
                // Walk decorations forward like stock GPUI. Positioned glyphs
                // inside a cluster may repeat/reorder indices and have negative
                // x offsets; those are not decoration or advance boundaries.
                while runs
                    .get(style_ix)
                    .is_some_and(|(range, _)| range.end <= byte)
                {
                    style_ix += 1;
                }
                let run = runs.get(style_ix).map(|(_, run)| *run);
                let x = glyph.position.x - row.start_x;
                transition(
                    &mut underlines,
                    &mut underline,
                    row_ix,
                    point(x, row.y + baseline + line.descent() * 0.618),
                    run.and_then(|run| {
                        run.underline.map(|style| {
                            Decoration::Underline(UnderlineStyle {
                                color: Some(style.color.unwrap_or(run.color)),
                                ..style
                            })
                        })
                    }),
                );
                transition(
                    &mut strikes,
                    &mut strike,
                    row_ix,
                    point(x, row.y + (line.ascent() * 0.5 + baseline) * 0.5),
                    run.and_then(|run| {
                        run.strikethrough.map(|style| {
                            Decoration::Strike(StrikethroughStyle {
                                color: Some(style.color.unwrap_or(run.color)),
                                ..style
                            })
                        })
                    }),
                );
            }
            let end_x = line.unwrapped_layout.width - self.rows[row_ix].start_x;
            finish(&mut underlines, &mut underline, end_x);
            finish(&mut strikes, &mut strike, end_x);
            row_ix += 1;
        }
        underlines.extend(strikes);
        underlines
    }
}

fn finish(strokes: &mut Vec<Stroke>, active: &mut Option<Stroke>, end_x: Pixels) {
    if let Some(mut stroke) = active.take() {
        stroke.width = end_x - stroke.origin.x;
        if stroke.width > Pixels::ZERO {
            strokes.push(stroke);
        }
    }
}

fn transition(
    strokes: &mut Vec<Stroke>,
    active: &mut Option<Stroke>,
    row: usize,
    origin: Point<Pixels>,
    decoration: Option<Decoration>,
) {
    if active.as_ref().map(|stroke| &stroke.decoration) == decoration.as_ref() {
        return;
    }
    finish(strokes, active, origin.x);
    *active = decoration.map(|decoration| Stroke {
        row,
        origin,
        width: Pixels::ZERO,
        decoration,
    });
}
