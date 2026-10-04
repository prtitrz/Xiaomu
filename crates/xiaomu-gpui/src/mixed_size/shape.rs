use super::{Input, Reason, SizeSpan, Unsupported, WorkLimits};
use gpui::{Font, Pixels, ShapedLine, TextRun, WindowTextSystem, px};
use std::ops::Range;

pub(super) struct Prepared<'a> {
    pub(super) text: &'a str,
    pub(super) sizes: Vec<SizeSpan>,
    pub(super) runs: Vec<TextRun>,
    run_ends: Vec<usize>,
    pub(super) base_font: Font,
    pub(super) base_size: Pixels,
    pub(super) line_height: f32,
    pub(super) wrap_width: Pixels,
    pub(super) limits: WorkLimits,
}

impl<'a> Prepared<'a> {
    pub(super) fn new(input: Input<'a>) -> Result<Self, Unsupported> {
        let invalid = || Unsupported::at(0..input.text.len(), Reason::InvalidInput);
        if !positive(input.base_size)
            || !positive(input.wrap_width)
            || !input.line_height.is_finite()
            || input.line_height <= 0.0
            || !positive(input.base_size * input.line_height)
        {
            return Err(invalid());
        }
        let mut sizes: Vec<SizeSpan> = Vec::new();
        let mut end = 0;
        for span in input.sizes {
            if span.range.start != end
                || span.range.end <= end
                || span.range.end > input.text.len()
                || !input.text.is_char_boundary(span.range.end)
                || !positive(span.size)
                || !positive(span.size * input.line_height)
            {
                return Err(invalid());
            }
            end = span.range.end;
            if let Some(last) = sizes.last_mut()
                && last.size == span.size
            {
                last.range.end = end;
            } else {
                sizes.push(span.clone());
            }
        }
        if !input.sizes.is_empty() && end != input.text.len() {
            return Err(invalid());
        }
        if sizes.is_empty() {
            sizes.push(SizeSpan {
                range: 0..input.text.len(),
                size: input.base_size,
            });
        }
        let runs = if input.runs.is_empty() {
            vec![TextRun {
                len: input.text.len(),
                font: input.base_font.clone(),
                color: input.base_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }]
        } else {
            let mut end = 0usize;
            for run in input.runs {
                end = end.checked_add(run.len).ok_or_else(invalid)?;
                if run.len == 0 || end > input.text.len() || !input.text.is_char_boundary(end) {
                    return Err(invalid());
                }
            }
            if end != input.text.len() {
                return Err(invalid());
            }
            input.runs.to_vec()
        };
        let mut run_end = 0;
        let run_ends = runs
            .iter()
            .map(|run| {
                run_end += run.len; // Total length was checked above.
                run_end
            })
            .collect();
        Ok(Self {
            text: input.text,
            sizes,
            runs,
            run_ends,
            base_font: input.base_font,
            base_size: input.base_size,
            line_height: input.line_height,
            wrap_width: input.wrap_width,
            limits: input.limits,
        })
    }

    pub(super) fn spans_for(&self, range: &Range<usize>) -> &[SizeSpan] {
        if range.is_empty() {
            return &[];
        }
        let start = self
            .sizes
            .partition_point(|span| span.range.end <= range.start);
        let end = self
            .sizes
            .partition_point(|span| span.range.start < range.end);
        &self.sizes[start..end]
    }

    pub(super) fn shape(
        &self,
        system: &WindowTextSystem,
        range: Range<usize>,
        size: Pixels,
    ) -> ShapedLine {
        system.shape_line(
            self.text[range.clone()].to_owned().into(),
            size,
            &self.clip_runs(range),
            None,
        )
    }

    /// Binary-search cumulative byte ends; only intersecting runs are cloned.
    /// This must not rescan a paragraph's entire mark list on every row shape.
    pub(super) fn run_indices(&self, range: &Range<usize>) -> Range<usize> {
        if range.is_empty() {
            return 0..0;
        }
        let first = self.run_ends.partition_point(|end| *end <= range.start);
        let last = self.run_ends.partition_point(|end| *end < range.end);
        first..last.saturating_add(1).min(self.runs.len())
    }

    pub(super) fn clip_runs(&self, range: Range<usize>) -> Vec<TextRun> {
        let indices = self.run_indices(&range);
        let mut offset = if indices.start == 0 {
            0
        } else {
            self.run_ends[indices.start - 1]
        };
        self.runs[indices]
            .iter()
            .filter_map(|run| {
                let start = offset.max(range.start);
                offset += run.len;
                let end = offset.min(range.end);
                (start < end).then(|| {
                    let mut clipped = run.clone();
                    clipped.len = end - start;
                    clipped
                })
            })
            .collect()
    }

    pub(super) fn size_at(&self, index: usize) -> Pixels {
        self.sizes
            .iter()
            .find(|span| span.range.contains(&index))
            .or_else(|| self.sizes.last())
            .map_or(self.base_size, |span| span.size)
    }
}

pub(super) fn positive(value: Pixels) -> bool {
    f32::from(value).is_finite() && value > px(0.0)
}
