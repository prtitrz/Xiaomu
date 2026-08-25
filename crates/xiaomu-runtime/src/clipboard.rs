//! Frontend-neutral text clipboard seam.
//!
//! The runtime never touches a platform clipboard directly: frontends
//! implement [`TextClipboard`] with their platform bindings (GPUI first)
//! and hand plain text to or take it from the editing layer. Structured
//! clipboard flavors belong to a later phase; P1 covers plain text only.

/// Plain-text read/write seam between the editing layer and the platform.
///
/// Implementations must not interpret the text; they transport it as-is.
pub trait TextClipboard {
    /// Replaces the platform clipboard content with `text`.
    fn write_text(&mut self, text: String);

    /// Returns the current clipboard text when one is available.
    ///
    /// Non-text clipboard content (images, structured flavors) reads as
    /// `None`; implementations must not error on foreign content.
    fn read_text(&self) -> Option<String>;
}

/// Normalizes platform clipboard text for pasting into a single paragraph.
///
/// Line breaks (`\r\n`, `\r`, `\n`) cannot be represented in a paragraph's
/// inline text, so each break collapses to one space. This is an editing
/// policy for the P1 single-block scope; multi-block paste semantics
/// arrive with document-level editing in later phases.
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
