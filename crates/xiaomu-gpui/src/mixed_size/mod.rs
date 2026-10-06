//! Conservative GPUI-local real-size layout shared by opt-in editor views.
//!
//! Uniform input delegates to stock GPUI, including its Unicode/bidi behavior.
//! Mixed input admits only conservative LTR scripts and observable cluster/seam
//! invariants. GPUI 0.2.2 omits cluster ends, advances and bidi levels, so this is
//! not a general Unicode shaping engine. Rejection must gate editing, never
//! silently discard the requested sizes. All offsets below are display UTF-8
//! bytes; the host continues to own canonical/display/UTF-16 projection.

mod budget;
#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod cluster_tests;
mod clusters;
mod geometry;
mod native_line;
use native_line::NativeLine;
#[cfg(test)]
mod index_tests;
mod rows;
mod shape;
#[cfg(test)]
mod tests;

use gpui::{Font, Hsla, Pixels, Size, TextRun, WindowTextSystem, WrappedLine};
use std::ops::Range;
use xiaomu_core::selection::CursorAffinity;

/// A resolved positive finite size over a UTF-8 byte range. Input partitions
/// must cover the complete string; adjacent equal sizes are coalesced first.
#[derive(Clone, Debug)]
pub(crate) struct SizeSpan {
    pub(crate) range: Range<usize>,
    pub(crate) size: Pixels,
}

/// Resolved display input. Empty `sizes` / `runs` inherit the base style.
/// `line_height` is a positive finite unitless multiplier, not fixed pixels.
#[derive(Clone)]
pub(crate) struct Input<'a> {
    pub(crate) text: &'a str,
    pub(crate) sizes: &'a [SizeSpan],
    pub(crate) runs: &'a [TextRun],
    pub(crate) base_font: Font,
    pub(crate) base_size: Pixels,
    /// Effective typing size for an empty block, without replacing its base strut.
    pub(crate) empty_size: Option<Pixels>,
    pub(crate) base_color: Hsla,
    pub(crate) line_height: f32,
    pub(crate) wrap_width: Pixels,
    pub(crate) limits: WorkLimits,
}

/// Explicit mixed-renderer work limits. These constrain capability,
/// never the canonical document or the native single-size path. The byte limit
/// counts repeated shaping of the same bytes; calls include empty-row tokens.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkLimits {
    pub(crate) max_shaped_bytes: usize,
    pub(crate) max_shape_calls: usize,
}

impl Default for WorkLimits {
    fn default() -> Self {
        Self {
            max_shaped_bytes: 1024 * 1024,
            max_shape_calls: 4096,
        }
    }
}

/// Uniform text stays on the existing complete-paragraph GPUI path. Hosts must
/// dispatch explicitly; a mixed layout is never repackaged as a `WrappedLine`.
#[derive(Debug)]
pub(crate) enum Layout {
    Uniform(UniformLayout),
    Mixed(MixedLayout),
}

#[derive(Debug)]
pub(crate) struct UniformLayout {
    pub(crate) lines: Vec<WrappedLine>,
    pub(crate) font_size: Pixels,
    pub(crate) line_height: Pixels,
}

/// The only geometry truth for an admitted mixed-size block.
#[derive(Debug)]
pub(crate) struct MixedLayout {
    pub(crate) rows: Vec<Row>,
    pub(crate) size: Size<Pixels>,
    pub(crate) text_len: usize,
}

#[derive(Debug)]
pub(crate) struct Row {
    /// Excludes a terminating LF; a soft wrap shares its end with the next start.
    pub(crate) range: Range<usize>,
    pub(crate) y: Pixels,
    pub(crate) height: Pixels,
    /// Baseline relative to the row top; all fragments share it.
    pub(crate) baseline: Pixels,
    pub(crate) width: Pixels,
    pub(crate) hard_break_after: bool,
    pub(crate) fragments: Vec<Fragment>,
    /// Indivisible shaped-cluster edges for wrapping and pointer hit testing.
    pub(crate) stops: Vec<CaretStop>,
    /// Every UTF-8 scalar boundary, including cluster interiors, mapped with
    /// stock GPUI x_for_index semantics internally; fragment starts are pinned
    /// to their advance origins. Several positions may share an x; GPUI exposes
    /// no GDEF ligature caret metrics. These remain valid keyboard/IME offsets.
    pub(crate) carets: Vec<CaretStop>,
}

#[derive(Debug)]
pub(crate) struct Fragment {
    pub(crate) range: Range<usize>,
    pub(crate) x: Pixels,
    /// Shaped at its actual size, contains exactly this row fragment's bytes.
    /// No whole-span painting or clipping of partial ligatures is necessary.
    pub(crate) line: NativeLine,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaretStop {
    pub(crate) index: usize,
    pub(crate) x: Pixels,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) index: usize,
    pub(crate) affinity: CursorAffinity,
}

/// A fail-closed result with the affected display-byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Unsupported {
    pub(crate) range: Range<usize>,
    pub(crate) reason: Reason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reason {
    InvalidInput,
    ComplexScriptOrControl,
    SizeBoundaryInsideGrapheme,
    SizeBoundaryInsideShapedCluster,
    ContextAtSizeBoundary,
    UnreliableClusterGeometry,
    NativeShapeFailed,
    WorkBudgetExceeded,
}

impl Unsupported {
    fn at(range: Range<usize>, reason: Reason) -> Self {
        Self { range, reason }
    }
}

/// Width-independent safety classification for a candidate edit. The host can
/// use this before committing canonical state; it must preserve the same base
/// font/style inputs used by subsequent layout. No geometry is cached here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    Uniform,
    MixedLtr,
}

/// `wrap_width` is deliberately ignored: resizing cannot change this result.
/// Empty lines and oversized indivisible clusters are valid geometry cases.
pub(crate) fn admission(
    system: &WindowTextSystem,
    mut input: Input<'_>,
) -> Result<Admission, Unsupported> {
    input.wrap_width = gpui::px(1.0);
    let prepared = shape::Prepared::new(input)?;
    admit_prepared(system, &prepared)
}

fn admit_prepared(
    system: &WindowTextSystem,
    prepared: &shape::Prepared<'_>,
) -> Result<Admission, Unsupported> {
    if prepared.sizes.len() <= 1 {
        return Ok(Admission::Uniform);
    }
    budget::check(prepared)?;
    clusters::validate_text(prepared.text)?;
    let mut start = 0;
    for paragraph in prepared.text.split('\n') {
        let end = start + paragraph.len();
        clusters::validate_seams(system, prepared, start..end)?;
        for span in prepared.spans_for(&(start..end)) {
            let range = start.max(span.range.start)..end.min(span.range.end);
            if range.start < range.end {
                let line = prepared.shape(system, range.clone(), span.size)?;
                clusters::stops(&line, range.start)?;
            }
        }
        start = end + 1;
    }
    Ok(Admission::MixedLtr)
}

/// Shape an entire block. Single-size input preserves GPUI's native pathway;
/// mixed input supports LF, UAX14 wrapping, shared baselines and safe clusters.
pub(crate) fn layout(system: &WindowTextSystem, input: Input<'_>) -> Result<Layout, Unsupported> {
    let prepared = shape::Prepared::new(input)?;
    if admit_prepared(system, &prepared)? == Admission::Uniform {
        let font_size = prepared
            .sizes
            .first()
            .map_or(prepared.base_size, |span| span.size);
        let lines = system
            .shape_text(
                prepared.text.to_owned().into(),
                font_size,
                &prepared.runs,
                Some(prepared.wrap_width),
                None,
            )
            .map_err(|_| Unsupported::at(0..prepared.text.len(), Reason::NativeShapeFailed))?;
        return Ok(Layout::Uniform(UniformLayout {
            lines: lines.into_iter().collect(),
            font_size,
            line_height: font_size.max(prepared.base_size) * prepared.line_height,
        }));
    }
    rows::build(system, &prepared).map(Layout::Mixed)
}
