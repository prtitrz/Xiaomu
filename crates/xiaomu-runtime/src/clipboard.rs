//! Frontend-neutral clipboard model and platform transport seam.
//!
//! Runtime owns detached clipboard values and their versioned metadata codec.
//! The platform-visible clipboard body remains ordinary text; Xiaomu-native
//! structure is carried as optional metadata and never leaks canonical
//! `NodeId`s or frontend-specific types into Core.

mod fragment;
mod projection;
mod wire;

pub use fragment::{ClipboardBlock, ClipboardNode, ClipboardNodeContent, ClipboardSlice};
pub use wire::{ClipboardMetadataError, decode_metadata, encode_metadata};

pub(crate) use projection::slice_selection;

/// Plain-text read/write seam between the editing layer and the platform.
///
/// Structured GPUI transport is implemented by the frontend adapter; Runtime
/// keeps this minimal text seam for generic hosts and plain-text fallback.
pub trait TextClipboard {
    /// Replaces the platform clipboard content with `text`.
    fn write_text(&mut self, text: String);

    /// Returns the current clipboard text when one is available.
    ///
    /// Non-text clipboard content reads as `None`; implementations must not
    /// error on foreign content.
    fn read_text(&self) -> Option<String>;
}

/// Normalizes platform clipboard text for pasting into a single inline block.
///
/// Line breaks (`\r\n`, `\r`, `\n`) cannot be represented in ordinary
/// paragraph inline text, so each break collapses to one space. Xiaomu-native
/// structured paste bypasses this fallback and reconstructs block structure.
#[must_use]
pub fn normalize_paste_text(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(character) = chars.next() {
        match character {
            '\r' => {
                chars.next_if_eq(&'\n');
                normalized.push(' ');
            }
            '\n' => normalized.push(' '),
            other => normalized.push(other),
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_collapse_to_spaces() {
        assert_eq!(normalize_paste_text("a\r\nb"), "a b");
        assert_eq!(normalize_paste_text("a\rb"), "a b");
        assert_eq!(normalize_paste_text("a\nb"), "a b");
        assert_eq!(normalize_paste_text("a\r\r\nb"), "a  b");
        assert_eq!(normalize_paste_text("\n"), " ");
        assert_eq!(normalize_paste_text(""), "");
    }

    #[test]
    fn non_breaking_content_is_preserved() {
        assert_eq!(normalize_paste_text("你好 world 👍"), "你好 world 👍");
        assert_eq!(
            normalize_paste_text("combining é\u{301}"),
            "combining é\u{301}"
        );
    }
}
