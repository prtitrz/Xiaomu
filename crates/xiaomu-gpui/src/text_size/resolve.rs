use gpui::{Pixels, TextRun, px};
use xiaomu_core::document::{NodeId, StringAttribute};

use super::{
    TextSizeCapability, TextSizeCaretContext, TextSizeError, TextSizeErrorKind, TextSizeStyle,
};
use crate::block_view::{DisplaySegment, FontCatalog, text_runs};
use crate::font_size::resolve_font_size;
use crate::mixed_size::{Input, SizeSpan, WorkLimits};

/// Resolved geometry and marks, shared unchanged by admission and layout.
pub(crate) struct ResolvedText {
    pub(crate) runs: Vec<TextRun>,
    pub(crate) sizes: Vec<SizeSpan>,
}

impl ResolvedText {
    pub(crate) fn input<'a>(
        &'a self,
        text: &'a str,
        style: &TextSizeStyle,
        wrap_width: Pixels,
    ) -> Input<'a> {
        Input {
            text,
            sizes: &self.sizes,
            runs: &self.runs,
            base_font: style.base_font().clone(),
            empty_size: None,
            base_size: px(style.context().parent_px()),
            base_color: style.color(),
            line_height: style.line_height(),
            wrap_width,
            limits: WorkLimits::default(),
        }
    }
}

impl TextSizeCapability {
    pub(crate) fn resolve_segments(
        &self,
        node: NodeId,
        style: &TextSizeStyle,
        segments: &[DisplaySegment],
    ) -> Result<ResolvedText, TextSizeError> {
        let text_len = segments
            .last()
            .map_or(0, |segment| segment.start + segment.text.len());
        let invalid_style =
            || TextSizeError::new(node, 0..text_len, TextSizeErrorKind::InvalidStyle);
        let inherited =
            resolve_font_size(&StringAttribute::Missing, style.context()).map_err(|error| {
                TextSizeError::new(node, 0..text_len, TextSizeErrorKind::FontSize(error))
            })?;
        if !style.line_height().is_finite()
            || style.line_height() <= 0.0
            || !(inherited * style.line_height()).is_finite()
            || inherited * style.line_height() <= 0.0
        {
            return Err(invalid_style());
        }
        let mut sizes: Vec<SizeSpan> = Vec::new();
        for segment in segments {
            if segment.text.is_empty() {
                continue;
            }
            let range = segment.start..segment.start + segment.text.len();
            let attribute = segment
                .text_style
                .as_ref()
                .map_or(&StringAttribute::Missing, |style| style.font_size());
            let size = resolve_font_size(attribute, style.context()).map_err(|error| {
                TextSizeError::new(node, range.clone(), TextSizeErrorKind::FontSize(error))
            })?;
            if !(size * style.line_height()).is_finite() || size * style.line_height() <= 0.0 {
                return Err(invalid_style());
            }
            let explicit =
                matches!(attribute, StringAttribute::Value(value) if !value.trim().is_empty())
                    .then_some(size);
            self.caret_height(
                node,
                TextSizeCaretContext::new(size, explicit, explicit, explicit),
            )?;
            if let Some(last) = sizes.last_mut()
                && last.size == px(size)
                && last.range.end == range.start
            {
                last.range.end = range.end;
            } else {
                sizes.push(SizeSpan {
                    range,
                    size: px(size),
                });
            }
        }
        let fonts = FontCatalog::from_system(&self.system);
        let runs = text_runs(segments, style.base_font().clone(), style.color(), &fonts)
            .into_iter()
            .filter(|run| run.len > 0)
            .collect();
        Ok(ResolvedText { runs, sizes })
    }
}
