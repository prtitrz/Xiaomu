#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! xiaomu-gpui: GPUI adapter for the Xiaomu native structured rich-text
//! editor.
//!
//! This crate owns everything GPUI-specific: input translation, block views,
//! layout, paint, hit testing, clipboard bindings, and IME composition state.
//! GPUI types never leak into `xiaomu-core` or `xiaomu-runtime`, and platform
//! coordinates (UTF-16 ranges, physical pixels) are converted to Core types at
//! this boundary.
//!
//! The adapter is bootstrapping: P1.1 only pins the GPUI dependency
//! (`=0.2.2`, see `docs/planning.md` §17) and proves it compiles and links
//! without opening a window. Subsequent P1 slices build the single-block
//! editing pipeline on top.

/// Bootstrap marker kept intentionally small while the public API is designed.
pub const CRATE_NAME: &str = "xiaomu-gpui";

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

        let color = gpui::Hsla::default();
        assert_eq!((color.h, color.s, color.l, color.a), (0.0, 0.0, 0.0, 0.0));

        assert_eq!(gpui_version(), "0.2.2");
        assert_eq!(CRATE_NAME, "xiaomu-gpui");
    }
}
