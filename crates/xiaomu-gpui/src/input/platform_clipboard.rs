//! GPUI platform binding for Xiaomu clipboard transport.
//!
//! This is the only place where the clipboard touches GPUI. Ordinary text is
//! always the platform-visible fallback. Xiaomu structured metadata rides on
//! GPUI's string metadata slot and is decoded only when it still matches that
//! text exactly.

use gpui::App;

use xiaomu_runtime::clipboard::{
    ClipboardSlice, TextClipboard, decode_metadata, encode_metadata,
};

/// Content read from the platform clipboard.
pub(crate) enum PlatformClipboardContent {
    /// Xiaomu metadata decoded and validated against the text fallback.
    Structured(ClipboardSlice),
    /// Foreign, stale, malformed, or ordinary plain text.
    Text(String),
}

/// Clipboard adapter backed by the GPUI app clipboard.
pub(crate) struct PlatformClipboard<'a> {
    app: &'a App,
}

impl<'a> PlatformClipboard<'a> {
    /// Creates a clipboard adapter over the running GPUI app.
    pub(crate) fn new(app: &'a App) -> Self {
        Self { app }
    }

    /// Writes a structured Xiaomu slice with interoperable plain text.
    pub(crate) fn write_slice(&mut self, slice: &ClipboardSlice) {
        let text = slice.plain_text().to_owned();
        let item = match encode_metadata(slice) {
            Ok(metadata) => gpui::ClipboardItem::new_string_with_metadata(text, metadata),
            Err(error) => {
                #[cfg(debug_assertions)]
                eprintln!("xiaomu: structured clipboard encoding failed: {error}");
                gpui::ClipboardItem::new_string(text)
            }
        };
        self.app.write_to_clipboard(item);
    }

    /// Reads structured Xiaomu content when valid, otherwise plain text.
    pub(crate) fn read_content(&self) -> Option<PlatformClipboardContent> {
        let item = self.app.read_from_clipboard()?;
        let text = item.text()?;
        if let Some(metadata) = item.metadata()
            && let Some(slice) = decode_metadata(&text, metadata)
        {
            return Some(PlatformClipboardContent::Structured(slice));
        }
        Some(PlatformClipboardContent::Text(text))
    }
}

impl TextClipboard for PlatformClipboard<'_> {
    fn write_text(&mut self, text: String) {
        self.app
            .write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    fn read_text(&self) -> Option<String> {
        self.app.read_from_clipboard()?.text()
    }
}
