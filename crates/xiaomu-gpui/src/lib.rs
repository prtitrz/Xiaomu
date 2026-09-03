#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! xiaomu-gpui: GPUI adapter for the Xiaomu native structured rich-text
//! editor.
//!
//! This crate owns everything GPUI-specific: input translation, block views,
//! layout, paint, hit testing, clipboard bindings, accessibility projection,
//! and IME composition state. GPUI types never leak into `xiaomu-core` or
//! `xiaomu-runtime`, and platform coordinates (UTF-16 ranges, physical pixels)
//! are converted to Core types at this boundary.
//!
//! Modules:
//! - [`accessibility`]: frontend-readable semantic/text/selection/focus
//!   projection, kept independent from unavailable platform role builders in
//!   the pinned crates.io GPUI 0.2.2 artifact.
//! - [`input`]: UTF-16 ↔ Core-offset conversion for the platform input path.
//! - [`block_view`]: one inline-bearing block view and its custom element.
//! - [`document_view`]: multi-block editor projection and interaction owner.
//! - [`editor`]: reusable editor-instance and application assembly helpers.
//! - [`inline_atom`]: host-neutral renderer registry.
//! - [`inline_atom_display`]: canonical mixed-inline ↔ display-byte projection.

pub mod accessibility;
pub mod block_view;
pub mod document_view;
pub mod editor;
pub mod inline_atom;
pub mod inline_atom_display;
mod inline_position;
pub mod input;

/// Returns the pinned GPUI crates.io version this crate is built against.
///
/// The dependency is exact-pinned in the workspace manifest; upgrades happen
/// in dedicated pull requests only.
#[must_use]
pub const fn gpui_version() -> &'static str {
    "0.2.2"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-and-link level smoke: GPUI's platform layer must resolve
    /// without opening a window or requiring a display server.
    #[test]
    fn gpui_platform_layer_links() {
        assert_eq!(gpui::px(12.0), gpui::px(6.0) + gpui::px(6.0));
        assert_eq!(gpui_version(), "0.2.2");
    }
}
