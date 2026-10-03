//! GPUI platform binding for Xiaomu clipboard transport.
//!
//! This is the only place where the clipboard touches GPUI. Ordinary text is
//! always the platform-visible fallback. Xiaomu structured metadata rides on
//! GPUI's string metadata slot and is decoded only when it still matches that
//! text exactly.

use gpui::App;
use xiaomu_runtime::assets::AssetFormat;

use xiaomu_runtime::clipboard::{ClipboardSlice, TextClipboard, decode_metadata, encode_metadata};

/// Content read from the platform clipboard.
pub(crate) enum PlatformClipboardContent {
    /// Xiaomu metadata decoded and validated against the text fallback.
    Structured(ClipboardSlice),
    /// Foreign, stale, malformed, or ordinary plain text.
    Text(String),
    /// Encoded pixels, imported by the host before any document edit.
    Image { format: AssetFormat, bytes: Vec<u8> },
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
        decode_item(item)
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

/// Valid structured content wins; supported pixels precede ordinary text.
fn decode_item(item: gpui::ClipboardItem) -> Option<PlatformClipboardContent> {
    let text = item.text();
    if let Some(text) = &text
        && let Some(metadata) = item.metadata()
        && let Some(slice) = decode_metadata(text, metadata)
    {
        return Some(PlatformClipboardContent::Structured(slice));
    }
    for entry in item.into_entries() {
        if let gpui::ClipboardEntry::Image(image) = entry {
            let format = match image.format {
                gpui::ImageFormat::Png => AssetFormat::Png,
                gpui::ImageFormat::Jpeg => AssetFormat::Jpeg,
                _ => continue,
            };
            return Some(PlatformClipboardContent::Image {
                format,
                bytes: image.bytes,
            });
        }
    }
    text.map(PlatformClipboardContent::Text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_only_clipboard_does_not_require_text() {
        for (input, expected) in [
            (gpui::ImageFormat::Png, AssetFormat::Png),
            (gpui::ImageFormat::Jpeg, AssetFormat::Jpeg),
        ] {
            let item =
                gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(input, vec![1, 2, 3]));
            let Some(PlatformClipboardContent::Image { format, bytes }) = decode_item(item) else {
                panic!("image lost")
            };
            assert_eq!(format, expected);
            assert_eq!(bytes, [1, 2, 3]);
        }
    }

    #[test]
    fn unsupported_image_is_not_reinterpreted_as_text() {
        assert!(
            decode_item(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                gpui::ImageFormat::Svg,
                vec![1]
            )))
            .is_none()
        );
        assert!(
            matches!(decode_item(gpui::ClipboardItem::new_string("hello".into())), Some(PlatformClipboardContent::Text(text)) if text == "hello")
        );
    }
}
