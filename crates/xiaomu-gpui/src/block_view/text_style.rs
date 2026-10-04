//! Rendering interpretation of exact TextStyle strings. Core data is untouched.
//!
//! Font size is deliberately not projected: mixed-size layout geometry is a
//! separate capability. Hosts must reject writable documents with any explicit
//! fontSize string instead of presenting this renderer as mixed-size support.

mod font_family;
pub(super) use font_family::FontCatalog;

use cssparser::{Color, Parser, ParserInput};
use gpui::{
    Font, FontStyle, FontWeight, Hsla, StrikethroughStyle, TextRun, UnderlineStyle, px, rgba,
};
use xiaomu_core::document::StringAttribute;

use super::DisplaySegment;

fn css_color(value: &str) -> Option<Hsla> {
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

pub(super) fn text_runs(
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
