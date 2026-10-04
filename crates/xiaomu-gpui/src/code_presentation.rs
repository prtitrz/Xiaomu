//! Opt-in, per-editor code-block presentation. This is frontend state only:
//! it never adds marks, changes source bytes, or participates in persistence.
//!
//! The metrics follow Chuanyun's `editor/styles/editor-typography.css`:
//! `pre code` is .88em, note `pre` uses 1.55 line height, and the wrapper has
//! 14px/16px padding, an 8px radius, and a 1px border. Colors are supplied by
//! the host because the original CSS resolves them from the current theme.
//! Syntax decorations can later be projected into shaping runs before the
//! existing style fingerprint is computed; no tokenizer or mark API is implied.

use gpui::{Div, Hsla, SharedString, prelude::*, px, rgba};

/// Visual defaults applied only to `CodeBlock` nodes in an opted-in editor.
///
/// Generic editor instances keep their existing presentation until this is
/// supplied through the editor builder or document-view setter. Fonts use a
/// CSS family list with the same native resolver as text-style marks. A host
/// can supply its resolved theme colors without changing canonical content.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeBlockPresentation {
    /// CSS font-family list. Defaults to `"DM Mono", monospace`.
    pub font_family: SharedString,
    /// Base code text color; `None` inherits the surrounding text color.
    pub text_color: Option<Hsla>,
    /// Code wrapper fill. The default is a subtle translucent neutral tint.
    pub background_color: Hsla,
    /// Code wrapper border color.
    pub border_color: Hsla,
}

impl Default for CodeBlockPresentation {
    fn default() -> Self {
        Self {
            font_family: "\"DM Mono\", monospace".into(),
            text_color: None,
            background_color: rgba(0x0000000f).into(),
            border_color: rgba(0x00000026).into(),
        }
    }
}

impl CodeBlockPresentation {
    /// Code font size relative to the inherited body font size.
    pub const FONT_SCALE: f32 = 0.88;
    /// Line-box height relative to the inherited body font size. The original
    /// block `pre` retains this minimum even though inline `code` is smaller.
    pub const LINE_HEIGHT: f32 = 1.55;
    /// Vertical wrapper padding in logical pixels.
    pub const PADDING_Y: f32 = 14.0;
    /// Horizontal wrapper padding in logical pixels.
    pub const PADDING_X: f32 = 16.0;
    /// Wrapper corner radius in logical pixels.
    pub const RADIUS: f32 = 8.0;
    /// Wrapper border width in logical pixels.
    pub const BORDER_WIDTH: f32 = 1.0;

    pub(crate) fn style_wrapper(&self, wrapper: Div) -> Div {
        // Only the paragraph child is wrapped. List markers remain siblings
        // in markers::style_block and cannot inherit this box or code style.
        wrapper
            .min_w_0()
            .bg(self.background_color)
            .border_color(self.border_color)
            .border_1()
            .rounded(px(Self::RADIUS))
            .px(px(Self::PADDING_X))
            .py(px(Self::PADDING_Y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_has_the_original_padding_border_radius_and_configurable_colors() {
        let presentation = CodeBlockPresentation {
            background_color: rgba(0x112233ff).into(),
            border_color: rgba(0x445566ff).into(),
            ..Default::default()
        };
        let mut wrapper = presentation.style_wrapper(gpui::div());
        let style = wrapper.style();
        assert_eq!(style.padding.top, Some(px(14.0).into()));
        assert_eq!(style.padding.bottom, Some(px(14.0).into()));
        assert_eq!(style.padding.left, Some(px(16.0).into()));
        assert_eq!(style.padding.right, Some(px(16.0).into()));
        assert_eq!(style.border_widths.top, Some(px(1.0).into()));
        assert_eq!(style.corner_radii.top_left, Some(px(8.0).into()));
        assert_eq!(style.border_color, Some(presentation.border_color));
        assert_eq!(style.background, Some(presentation.background_color.into()));
    }
}
