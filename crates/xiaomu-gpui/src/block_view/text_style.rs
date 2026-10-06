//! Rendering interpretation of exact TextStyle strings. Core data is untouched.
//!
//! Font size is resolved by the opt-in `text_size` capability. Without that
//! capability, hosts must reject writable documents with explicit font sizes.

mod font_family;
pub(crate) use font_family::FontCatalog;

use cssparser::{Color, Parser, ParserInput};
use gpui::{
    Font, FontStyle, FontWeight, Hsla, Pixels, StrikethroughStyle, TextRun, UnderlineStyle, Window,
    px, rgba,
};
use xiaomu_core::document::StringAttribute;

use super::DisplaySegment;
use crate::code_presentation::CodeBlockPresentation;

/// Resolve block defaults once. The resulting shaped layout owns the same
/// font size and line height for paint, selection, pointer and native IME.
pub(super) struct BlockTextStyle {
    pub font: Font,
    pub font_size: Pixels,
    pub color: Hsla,
    pub line_height: Pixels,
}

pub(super) fn block_text_style(
    window: &Window,
    code: Option<&CodeBlockPresentation>,
    fonts: &FontCatalog<'_>,
) -> BlockTextStyle {
    let inherited = window.text_style();
    let mut style = BlockTextStyle {
        font: inherited.font(),
        font_size: inherited.font_size.to_pixels(window.rem_size()),
        color: inherited.color,
        line_height: window.line_height(),
    };
    if let Some(code) = code {
        // Keep native fallback support for CJK/emoji. A platform with no
        // catalog match still receives an explicit monospace family request.
        style.font.family = "monospace".into();
        style.font = fonts.apply(&code.font_family, &style.font);
        // In the original CSS the 1.55 belongs to the parent `pre`. Its
        // body-sized strut sets a minimum line box, even though `code` is
        // .88em. Keep that minimum rather than shrinking it a second time.
        // Native baseline/font-fallback raster parity is a separate concern.
        style.line_height = style.font_size * CodeBlockPresentation::LINE_HEIGHT;
        style.font_size *= CodeBlockPresentation::FONT_SCALE;
        style.color = code.text_color.unwrap_or(style.color);
    }
    style
}

pub(crate) fn css_color(value: &str) -> Option<Hsla> {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let parsed = Color::parse(&mut parser).ok()?;
    parser.expect_exhausted().ok()?;
    match parsed {
        Color::CurrentColor => None,
        Color::RGBA(color) => Some(
            rgba(u32::from_be_bytes([
                color.red,
                color.green,
                color.blue,
                color.alpha,
            ]))
            .into(),
        ),
    }
}

pub(crate) fn text_runs(
    segments: &[DisplaySegment],
    font: Font,
    color: Hsla,
    fonts: &FontCatalog<'_>,
) -> Vec<TextRun> {
    segments
        .iter()
        .map(|segment| {
            let mut run_font = font.clone();
            let mut run_color = if segment.link {
                rgba(0x2563ebff).into()
            } else {
                color
            };
            if let Some(style) = &segment.text_style {
                if let StringAttribute::Value(value) = style.color() {
                    // currentColor, inherit and invalid/unsupported values retain
                    // the surrounding mark/host color, just like CSS fallback.
                    if let Some(color) = css_color(value) {
                        run_color = color;
                    }
                }
                if let StringAttribute::Value(value) = style.font_family() {
                    run_font = fonts.apply(value, &run_font);
                }
            }
            if segment.bold {
                run_font.weight = FontWeight::BOLD;
            }
            if segment.italic {
                run_font.style = FontStyle::Italic;
            }
            TextRun {
                len: segment.text.len(),
                font: run_font,
                color: run_color,
                background_color: segment.code.then_some(rgba(0x00000012).into()),
                underline: (segment.underline || segment.link).then_some(UnderlineStyle {
                    color: Some(run_color),
                    thickness: px(1.0),
                    wavy: false,
                }),
                strikethrough: segment.strike.then_some(StrikethroughStyle {
                    color: Some(run_color),
                    thickness: px(1.0),
                }),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "text_style_tests.rs"]
mod tests;
