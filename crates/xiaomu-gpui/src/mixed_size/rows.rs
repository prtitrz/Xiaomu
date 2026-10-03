use super::clusters;
use super::shape::Prepared;
use super::{CaretStop, Fragment, MixedLayout, Row, Unsupported};
use gpui::{Pixels, WindowTextSystem, px, size};
use std::collections::BTreeSet;
use std::ops::Range;
use unicode_linebreak::linebreaks;

struct Cluster {
    range: Range<usize>,
    width: Pixels,
    break_after: bool,
}

pub(super) fn build(
    system: &WindowTextSystem,
    input: &Prepared<'_>,
) -> Result<MixedLayout, Unsupported> {
    let mut rows = Vec::new();
    let mut start = 0;
    // `split` intentionally preserves empty paragraphs and a trailing LF.
    for paragraph in input.text.split('\n') {
        let end = start + paragraph.len();
        let hard_break_after = end < input.text.len();
        let measured = measure(system, input, start..end)?;
        if measured.is_empty() {
            rows.push(shape_row(system, input, start..end)?);
        } else {
            wrap(system, input, &measured, &mut rows)?;
        }
        rows.last_mut()
            .expect("one row per paragraph")
            .hard_break_after = hard_break_after;
        start = end + 1;
    }
    let mut measured_size = size(px(0.0), px(0.0));
    for row in &mut rows {
        row.y = measured_size.height;
        measured_size.width = measured_size.width.max(row.width);
        measured_size.height += row.height;
    }
    Ok(MixedLayout {
        rows,
        size: measured_size,
        text_len: input.text.len(),
    })
}

fn measure(
    system: &WindowTextSystem,
    input: &Prepared<'_>,
    paragraph: Range<usize>,
) -> Result<Vec<Cluster>, Unsupported> {
    // The UAX14 pass sees the logical paragraph, never individual mark spans.
    let breaks: BTreeSet<_> = linebreaks(&input.text[paragraph.clone()])
        .map(|(index, _)| paragraph.start + index)
        .collect();
    let mut result = Vec::new();
    for span in input.spans_for(&paragraph) {
        let range = span.range.start.max(paragraph.start)..span.range.end.min(paragraph.end);
        if range.start >= range.end {
            continue;
        }
        let line = input.shape(system, range.clone(), span.size);
        let stops = clusters::stops(&line, range.start)?;
        for pair in stops.windows(2) {
            result.push(Cluster {
                range: pair[0].index..pair[1].index,
                width: pair[1].x - pair[0].x,
                break_after: breaks.contains(&pair[1].index),
            });
        }
    }
    Ok(result)
}

fn wrap(
    system: &WindowTextSystem,
    input: &Prepared<'_>,
    clusters: &[Cluster],
    rows: &mut Vec<Row>,
) -> Result<(), Unsupported> {
    let mut start = 0;
    while start < clusters.len() {
        let mut end = start;
        let mut width = px(0.0);
        while end < clusters.len() {
            let next_width = width + clusters[end].width;
            if next_width > input.wrap_width && end > start {
                break;
            }
            width = next_width;
            end += 1;
            if width > input.wrap_width {
                break; // One indivisible oversized cluster is permitted to overflow.
            }
        }
        if end < clusters.len() {
            end = preferred_break(clusters, start, end);
        }
        // Re-shape exactly the final row slices: no partial glyph painting and
        // no assumption that removing line-edge kerning preserves advance.
        let mut row = shape_row(
            system,
            input,
            clusters[start].range.start..clusters[end - 1].range.end,
        )?;
        if row.width > input.wrap_width && end > start + 1 {
            // Bound reflow independently of width: one candidate and at most
            // one atomic fallback. Do not repeatedly re-shape shorter prefixes.
            // A contextual edge may leave a sparse row, but never a split
            // cluster or a multi-cluster overflow. Admission reserves this cost.
            end = start + 1;
            row = shape_row(
                system,
                input,
                clusters[start].range.start..clusters[end - 1].range.end,
            )?;
        }
        restrict_hit_stops(&mut row, |index| {
            clusters[start..end]
                .binary_search_by_key(&index, |cluster| cluster.range.end)
                .is_ok()
        });
        rows.push(row);
        start = end;
    }
    Ok(())
}

fn preferred_break(clusters: &[Cluster], start: usize, end: usize) -> usize {
    (start..end)
        .rev()
        .find(|index| clusters[*index].break_after)
        .map_or(end, |index| index + 1)
}

fn shape_row(
    system: &WindowTextSystem,
    input: &Prepared<'_>,
    range: Range<usize>,
) -> Result<Row, Unsupported> {
    let font = system.resolve_font(&input.base_font);
    let strut_size = if range.is_empty() {
        input.size_at(range.start)
    } else {
        input.base_size
    };
    let mut above = system.ascent(font, strut_size);
    let mut below = px(f32::from(system.descent(font, strut_size)).abs());
    let leading = (strut_size * input.line_height - above - below).max(px(0.0)) / 2.0;
    above += leading;
    below += leading;
    let mut fragments = Vec::new();
    let mut stops = vec![CaretStop {
        index: range.start,
        x: px(0.0),
    }];
    let mut x = px(0.0);
    for span in input.spans_for(&range) {
        let fragment_range = span.range.start.max(range.start)..span.range.end.min(range.end);
        if fragment_range.start >= fragment_range.end {
            continue;
        }
        let line = input.shape(system, fragment_range.clone(), span.size);
        let local_stops = clusters::stops(&line, fragment_range.start)?;
        stops.extend(local_stops.into_iter().skip(1).map(|stop| CaretStop {
            index: stop.index,
            x: stop.x + x,
        }));
        let descent = px(f32::from(line.descent).abs());
        let leading = (span.size * input.line_height - line.ascent - descent).max(px(0.0)) / 2.0;
        above = above.max(line.ascent + leading);
        below = below.max(descent + leading);
        let width = line.width;
        fragments.push(Fragment {
            range: fragment_range,
            x,
            line,
        });
        x += width;
    }
    let carets = caret_positions(&fragments, range.start);
    Ok(Row {
        range,
        y: px(0.0),
        height: above + below,
        baseline: above,
        width: x,
        hard_break_after: false,
        fragments,
        stops,
        carets,
    })
}

/// Re-shaping a line edge may decompose a contextual ligature. That does not
/// license a new emergency-wrap/pointer edge inside the original atomic unit.
/// Logical scalar caret positions are deliberately independent of this filter.
pub(super) fn restrict_hit_stops(row: &mut Row, is_measured_end: impl Fn(usize) -> bool) {
    let start = row.range.start;
    row.stops
        .retain(|stop| stop.index == start || is_measured_end(stop.index));
}

pub(super) fn caret_positions(fragments: &[Fragment], start: usize) -> Vec<CaretStop> {
    let mut result = Vec::<CaretStop>::new();
    for fragment in fragments {
        // Interior scalar offsets follow stock x_for_index's first glyph.index
        // >= local rule, merged in one pass rather than O(N^2) rescanning.
        // Fragment starts use their advance origin, matching wrap/hit stops
        // and adjacent fragment endpoints even with a nonzero first glyph x.
        let mut glyphs = fragment
            .line
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .peekable();
        let boundaries = fragment
            .line
            .text
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(fragment.line.text.len()));
        for local in boundaries {
            let index = fragment.range.start + local;
            if result.last().is_some_and(|stop| stop.index == index) {
                continue;
            }
            while glyphs.peek().is_some_and(|glyph| glyph.index < local) {
                glyphs.next();
            }
            result.push(CaretStop {
                index,
                x: fragment.x
                    + if local == 0 {
                        px(0.0)
                    } else {
                        glyphs
                            .peek()
                            .map_or(fragment.line.width, |glyph| glyph.position.x)
                    },
            });
        }
    }
    if result.is_empty() {
        result.push(CaretStop {
            index: start,
            x: px(0.0),
        });
    }
    result
}
