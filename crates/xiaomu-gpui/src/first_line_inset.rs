//! Host-resolved first-visual-line indentation in logical pixels.
//!
//! The engine owns wrapping and geometry, never attribute names, `em` units,
//! paragraph eligibility, or ancestor policies. Install this provider on the
//! shared [`crate::text_size::TextSizeCapability`] used for both candidate
//! validation and the view. This is a conservative LTR layout capability,
//! including for uniformly sized text; it is not general bidi layout.

use xiaomu_core::document::{Node, XiaomuDocument};

/// Maximum supported inset in logical pixels, a frontend geometry budget.
/// Values beyond this limit fail admission without changing canonical data.
pub const MAX_FIRST_LINE_INSET_PX: f32 = 65_536.0;

/// Pure first-line geometry projection for an inline-bearing canonical node.
///
/// The immutable document exposes ancestor context. `base_font_px` is the
/// resolved inherited block size, never the largest inline run or typing size.
/// Return a finite nonnegative logical-pixel inset no greater than
/// [`MAX_FIRST_LINE_INSET_PX`]. Zero retains the existing
/// layout path exactly. Invalid values reject admission; they are never
/// clamped or silently ignored. Positive values can exceed the container width:
/// the first indivisible shaped cluster then overflows on the first row.
///
/// The inset is consumed once across the entire block, including an empty first
/// row or leading hard break. Continuations use the full container width.
/// Center/right alignment applies inside each row's remaining allocated box.
/// Like stock GPUI, wrapping uses the exact logical measurement width while
/// alignment uses the final allocated width (which Taffy may round afterward).
/// The retained row-partition cache key is never relabeled by prepaint.
///
/// Like the style provider, this must be deterministic for a given snapshot and
/// base size. Do not mutate a session or query mutable view state. Replace and
/// revalidate the entire editor to replace or remove this provider or change
/// host configuration; do not keep an old policy with a newly configured view.
/// Native range-input
/// proxies never invoke this provider.
pub trait FirstLineInsetProvider {
    /// Resolves this block's first visual row inset without changing its data.
    fn first_line_inset(&self, document: &XiaomuDocument, node: &Node, base_font_px: f32) -> f32;
}
