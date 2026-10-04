//! Conservative evidence gates for the metadata stock GPUI actually exposes.
//! We never claim that a next glyph index is a general Unicode cluster end.
//! For admitted monotone LTR output, EGC boundaries intersected with observed
//! glyph starts provide indivisible units (including multi-EGC ligatures).

use super::shape::Prepared;
use super::{CaretStop, Reason, Unsupported};
use gpui::{ShapedLine, WindowTextSystem, px};
use std::ops::Range;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn validate_text(text: &str) -> Result<(), Unsupported> {
    for (start, grapheme) in text.grapheme_indices(true) {
        for character in grapheme.chars() {
            let script_allowed = matches!(
                character.script(),
                Script::Latin
                    | Script::Han
                    | Script::Hiragana
                    | Script::Katakana
                    | Script::Hangul
                    | Script::Bopomofo
                    | Script::Common
                    | Script::Inherited
            );
            let forbidden_control = (character.is_control() && character != '\n')
                || matches!(
                    character,
                    '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}'
                        | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{00ad}'
                );
            // ZWJ is admitted only inside an emoji-like extended grapheme.
            let emoji_joiner = character == '\u{200d}'
                && grapheme
                    .chars()
                    .any(|ch| matches!(ch as u32, 0x1f000..=0x1faff | 0x2600..=0x27bf));
            if !script_allowed || (forbidden_control && !emoji_joiner) {
                return Err(Unsupported::at(
                    start..start + grapheme.len(),
                    Reason::ComplexScriptOrControl,
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn stops(line: &ShapedLine, offset: usize) -> Result<Vec<CaretStop>, Unsupported> {
    let unreliable = || {
        Unsupported::at(
            offset..offset + line.text.len(),
            Reason::UnreliableClusterGeometry,
        )
    };
    if !f32::from(line.width).is_finite()
        || !f32::from(line.ascent).is_finite()
        || !f32::from(line.descent).is_finite()
        || line.width < px(0.0)
        || line.ascent < px(0.0)
    {
        return Err(unreliable());
    }
    if line.text.is_empty() {
        return Ok(vec![CaretStop {
            index: offset,
            x: px(0.0),
        }]);
    }
    let graphemes: Vec<_> = line.text.grapheme_indices(true).map(|(i, _)| i).collect();
    let mut result = Vec::<CaretStop>::new();
    let mut previous_group = None;
    for glyph in line.runs.iter().flat_map(|run| &run.glyphs) {
        if glyph.index >= line.text.len()
            || !line.text.is_char_boundary(glyph.index)
            || !f32::from(glyph.position.x).is_finite()
            || !f32::from(glyph.position.y).is_finite()
        {
            return Err(unreliable());
        }
        let group = graphemes[graphemes.partition_point(|index| *index <= glyph.index) - 1];
        if previous_group.is_some_and(|previous| group < previous) {
            return Err(unreliable());
        }
        if previous_group != Some(group) {
            // A glyph starting inside an EGC cannot establish a new safe stop.
            if glyph.index != group {
                return Err(unreliable());
            }
            if let Some(previous) = result.last() {
                if glyph.position.x <= previous.x {
                    return Err(unreliable());
                }
            } else if group != 0 {
                return Err(unreliable());
            }
            result.push(CaretStop {
                index: offset + group,
                x: if group == 0 {
                    px(0.0)
                } else {
                    glyph.position.x
                },
            });
        }
        previous_group = Some(group);
    }
    let Some(last) = result.last() else {
        return Err(unreliable());
    };
    if line.width <= last.x {
        return Err(unreliable());
    }
    result.push(CaretStop {
        index: offset + line.text.len(),
        x: line.width,
    });
    Ok(result)
}

/// Check every size seam against complete paragraph shaping at each size.
/// Missing explicit cluster-end metadata makes rejection preferable to
/// guessing. Even a safe grapheme seam can lose `ffi` or cross-seam AV kerning.
pub(super) fn validate_seams(
    system: &WindowTextSystem,
    input: &Prepared<'_>,
    paragraph: Range<usize>,
) -> Result<(), Unsupported> {
    let spans = input.spans_for(&paragraph);
    let boundaries: Vec<_> = spans
        .iter()
        .map(|span| span.range.start)
        .filter(|index| *index > paragraph.start && *index < paragraph.end)
        .collect();
    if boundaries.is_empty() {
        return Ok(());
    }
    let text = &input.text[paragraph.clone()];
    let graphemes: Vec<_> = text
        .grapheme_indices(true)
        .map(|(index, _)| index + paragraph.start)
        .collect();
    for boundary in &boundaries {
        if graphemes.binary_search(boundary).is_err() {
            return Err(Unsupported::at(
                *boundary..*boundary,
                Reason::SizeBoundaryInsideGrapheme,
            ));
        }
    }
    let mut checked_sizes = Vec::new();
    for span in spans {
        if span.range.end <= paragraph.start
            || span.range.start >= paragraph.end
            || checked_sizes.contains(&span.size)
        {
            continue;
        }
        checked_sizes.push(span.size);
        let full = input.shape(system, paragraph.clone(), span.size);
        for boundary in &boundaries {
            let left = input.shape(system, paragraph.start..*boundary, span.size);
            let right = input.shape(system, *boundary..paragraph.end, span.size);
            validate_split(&full, &left, &right, paragraph.start)?;
        }
    }
    Ok(())
}

pub(super) fn validate_split(
    full: &ShapedLine,
    left: &ShapedLine,
    right: &ShapedLine,
    offset: usize,
) -> Result<(), Unsupported> {
    let boundary = offset + left.text.len();
    if !stops(full, offset)?
        .iter()
        .any(|stop| stop.index == boundary)
    {
        return Err(Unsupported::at(
            boundary..boundary,
            Reason::SizeBoundaryInsideShapedCluster,
        ));
    }
    if !same_after_split(full, left, right) {
        return Err(Unsupported::at(
            boundary..boundary,
            Reason::ContextAtSizeBoundary,
        ));
    }
    Ok(())
}

fn same_after_split(full: &ShapedLine, left: &ShapedLine, right: &ShapedLine) -> bool {
    let near = |a: gpui::Pixels, b: gpui::Pixels| f32::from(a - b).abs() <= 0.01;
    if !near(full.width, left.width + right.width) {
        return false;
    }
    let full_glyphs: Vec<_> = full
        .runs
        .iter()
        .flat_map(|run| run.glyphs.iter().map(move |glyph| (run.font_id, glyph)))
        .collect();
    let split_glyphs: Vec<_> = [(left, 0, px(0.0)), (right, left.text.len(), left.width)]
        .into_iter()
        .flat_map(|(line, offset, x)| {
            line.runs.iter().flat_map(move |run| {
                run.glyphs
                    .iter()
                    .map(move |glyph| (run.font_id, glyph, offset, x))
            })
        })
        .collect();
    full_glyphs.len() == split_glyphs.len()
        && full_glyphs.iter().zip(split_glyphs).all(
            |((font, glyph), (other_font, other, offset, x))| {
                *font == other_font
                    && glyph.id == other.id
                    && glyph.index == other.index + offset
                    && near(glyph.position.x, other.position.x + x)
                    && near(glyph.position.y, other.position.y)
            },
        )
}
