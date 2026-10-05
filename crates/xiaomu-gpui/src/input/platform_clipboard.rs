//! GPUI platform binding for Xiaomu clipboard transport.
//!
//! This is the only place where the clipboard touches GPUI. Ordinary text is
//! always the platform-visible fallback. Xiaomu structured metadata rides on
//! GPUI's string metadata slot and is decoded only when it still matches that
//! text exactly.

use gpui::App;
use xiaomu_runtime::assets::AssetFormat;

use xiaomu_runtime::clipboard::{
    ClipboardMetadataDecode, ClipboardSlice, TextClipboard, decode_metadata_checked,
    encode_metadata,
};

/// Content read from the platform clipboard.
pub(crate) enum PlatformClipboardContent {
    /// Xiaomu metadata decoded and validated against the text fallback.
    Structured(ClipboardSlice),
    /// Foreign, legacy-fallback, or ordinary plain text.
    Text(String),
    /// Encoded pixels, imported by the host before any document edit.
    Image {
        format: AssetFormat,
        bytes: Vec<u8>,
        /// Preserve coexisting text for an opt-in code-target route. Merely
        /// retaining it does not change the default image-first precedence.
        plain_text: Option<String>,
    },
}

/// Recognized native metadata must never degrade to another clipboard flavor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeMetadataRejection;

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
    /// Returns false only when required lossless transport refused the write.
    pub(crate) fn write_slice(&mut self, slice: &ClipboardSlice) -> bool {
        // Boundaries and explicit text projections cannot survive a fallback.
        // Preserve the existing clipboard unless the complete descriptor and
        // tree survive the same decoder used by platform reads.
        if slice.requires_lossless_transport() {
            let written = write_lossless_slice(slice, |item| self.app.write_to_clipboard(item));
            if !written {
                eprintln!("xiaomu: clipboard copy requires lossless structured metadata");
            }
            return written;
        }
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
        true
    }

    /// Writes only a fully round-trippable structured slice for Cut.
    ///
    /// A failed encode or decode must not replace the platform clipboard or
    /// authorize removal of canonical source content. Ordinary Copy keeps
    /// its interoperable plain-text fallback through `write_slice`.
    pub(crate) fn write_slice_for_cut(&mut self, slice: &ClipboardSlice) -> bool {
        write_lossless_slice(slice, |item| self.app.write_to_clipboard(item))
    }

    /// Reads valid native content or foreign/legacy fallbacks.
    ///
    /// Rejected recognized native metadata never falls through to text or
    /// pixels: losing its descriptor could change the meaning of a paste.
    pub(crate) fn read_content_checked(
        &self,
    ) -> Result<Option<PlatformClipboardContent>, NativeMetadataRejection> {
        #[cfg(test)]
        if let Some(content) = mixed_tests::take_content() {
            return Ok(Some(content));
        }
        let Some(item) = self.app.read_from_clipboard() else {
            return Ok(None);
        };
        decode_item_checked(item)
    }

    /// Existing transport fixtures only need the accepted content.
    #[cfg(test)]
    pub(crate) fn read_content(&self) -> Option<PlatformClipboardContent> {
        self.read_content_checked().ok().flatten()
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

/// The write callback is reached only after exact structured round-trip
/// validation, including the decoder's untrusted-metadata limits.
fn write_lossless_slice(slice: &ClipboardSlice, write: impl FnOnce(gpui::ClipboardItem)) -> bool {
    let Ok(metadata) = encode_metadata(slice) else {
        return false;
    };
    if !matches!(
        decode_metadata_checked(slice.plain_text(), &metadata),
        ClipboardMetadataDecode::Valid(decoded) if &decoded == slice
    ) {
        return false;
    }
    write(gpui::ClipboardItem::new_string_with_metadata(
        slice.plain_text().to_owned(),
        metadata,
    ));
    true
}

/// Classify native metadata before considering any platform fallback flavor.
fn decode_item_checked(
    item: gpui::ClipboardItem,
) -> Result<Option<PlatformClipboardContent>, NativeMetadataRejection> {
    let text = item.text();
    let decoded = item.metadata().map_or(
        ClipboardMetadataDecode::ForeignOrLegacyFallback,
        |metadata| decode_metadata_checked(text.as_deref().unwrap_or_default(), metadata),
    );
    decode_transport_checked(text, decoded, item.into_entries())
}

/// Even a native empty-body slice requires an actual platform text flavor.
fn decode_transport_checked(
    text: Option<String>,
    decoded: ClipboardMetadataDecode,
    entries: impl IntoIterator<Item = gpui::ClipboardEntry>,
) -> Result<Option<PlatformClipboardContent>, NativeMetadataRejection> {
    match decoded {
        ClipboardMetadataDecode::Valid(slice) if text.is_some() => {
            Ok(Some(PlatformClipboardContent::Structured(slice)))
        }
        ClipboardMetadataDecode::Valid(_) | ClipboardMetadataDecode::RejectedNative => {
            Err(NativeMetadataRejection)
        }
        ClipboardMetadataDecode::ForeignOrLegacyFallback => Ok(decode_fallback(text, entries)),
    }
}

#[cfg(test)]
fn decode_item(item: gpui::ClipboardItem) -> Option<PlatformClipboardContent> {
    decode_item_checked(item).ok().flatten()
}

#[cfg(test)]
fn decode_transport(
    text: Option<String>,
    decoded: ClipboardMetadataDecode,
    entries: impl IntoIterator<Item = gpui::ClipboardEntry>,
) -> Option<PlatformClipboardContent> {
    decode_transport_checked(text, decoded, entries)
        .ok()
        .flatten()
}

/// Retains image-first transport while keeping any accompanying raw text.
fn decode_fallback(
    text: Option<String>,
    entries: impl IntoIterator<Item = gpui::ClipboardEntry>,
) -> Option<PlatformClipboardContent> {
    for entry in entries {
        if let gpui::ClipboardEntry::Image(image) = entry {
            let format = match image.format {
                gpui::ImageFormat::Png => AssetFormat::Png,
                gpui::ImageFormat::Jpeg => AssetFormat::Jpeg,
                _ => continue,
            };
            return Some(PlatformClipboardContent::Image {
                format,
                bytes: image.bytes,
                plain_text: text,
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
            let Some(PlatformClipboardContent::Image {
                format,
                bytes,
                plain_text,
            }) = decode_item(item)
            else {
                panic!("image lost")
            };
            assert_eq!(format, expected);
            assert_eq!(bytes, [1, 2, 3]);
            assert_eq!(plain_text, None);
        }
    }

    #[test]
    fn mixed_image_fallback_retains_exact_nonempty_empty_and_absent_text() {
        for format in [gpui::ImageFormat::Png, gpui::ImageFormat::Jpeg] {
            for text in [None, Some(""), Some(" "), Some("甲\r\n乙\r丙\n丁\t")] {
                let entries = [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                    format,
                    vec![1, 2, 3],
                ))];
                let Some(PlatformClipboardContent::Image {
                    plain_text, bytes, ..
                }) = decode_fallback(text.map(str::to_owned), entries)
                else {
                    panic!("mixed transport must still default to image");
                };
                assert_eq!(plain_text.as_deref(), text);
                assert_eq!(bytes, [1, 2, 3]);
            }
        }
    }

    #[test]
    fn unsupported_mixed_image_keeps_original_text_fallback() {
        for text in [None, Some(""), Some("raw\r\ntext")] {
            let entries = [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                gpui::ImageFormat::Svg,
                vec![1],
            ))];
            let content = decode_fallback(text.map(str::to_owned), entries);
            match (text, content) {
                (None, None) => {}
                (Some(expected), Some(PlatformClipboardContent::Text(actual))) => {
                    assert_eq!(actual, expected)
                }
                _ => panic!("unsupported pixels must not invent or discard text"),
            }
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

#[cfg(test)]
mod cut_tests {
    use super::*;
    use std::cell::Cell;
    use xiaomu_core::document::{
        AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, InlinePoint};
    use xiaomu_runtime::clipboard::decode_metadata;
    use xiaomu_runtime::session::{
        DocumentPosition, DocumentSelection, DocumentSession, EditIntent,
    };

    fn atomic_session(attr_depth: usize) -> (DocumentSession, NodeId) {
        let mut payload = AttrValue::String("must-survive".to_owned());
        for _ in 0..attr_depth {
            payload = AttrValue::List(vec![payload]);
        }
        let attrs = NodeAttrs::new([("host-data".to_owned(), payload)].into()).unwrap();
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("keep", Default::default()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap();
        let image = builder
            .insert(NodeKind::Image, attrs, NodeContent::Atomic)
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph, image]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(0)
            .unwrap();
        let mut session = DocumentSession::new(
            document,
            DocumentSelection::collapsed(DocumentPosition::Inline(InlinePoint::new(
                paragraph,
                offset,
                0,
                CursorAffinity::Before,
            ))),
        )
        .unwrap();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".to_owned(),
            })
            .unwrap();
        session.set_atomic_selection(image).unwrap();
        (session, image)
    }

    #[test]
    fn cut_stops_before_clipboard_write_and_delete_when_metadata_cannot_round_trip() {
        // This valid canonical payload exceeds the symmetric wire budget.
        // A plain-text fallback cannot retain its extension payload.
        let (mut session, image) = atomic_session(160);
        let slice = session.clipboard_slice().unwrap().unwrap();
        assert!(encode_metadata(&slice).is_err());
        let before_document = format!("{:?}", session.document());
        let before_selection = session.selection();
        let before_history = session.history_depths();
        let writes = Cell::new(0);
        let written = write_lossless_slice(&slice, |_| writes.set(writes.get() + 1));
        if written {
            session.apply_intent(&EditIntent::Delete).unwrap();
        }
        assert!(!written);
        assert_eq!(writes.get(), 0);
        assert_eq!(format!("{:?}", session.document()), before_document);
        assert_eq!(session.selection(), before_selection);
        assert_eq!(session.history_depths(), before_history);
        assert!(session.document().node(image).is_some());
    }

    #[gpui::test]
    fn closed_copy_keeps_previous_clipboard_when_metadata_exceeds_budget(
        cx: &mut gpui::TestAppContext,
    ) {
        let (mut session, _) = atomic_session(160);
        let legacy = session.clipboard_slice().unwrap().unwrap();
        let all = DocumentSelection::all(session.document());
        session.set_document_selection(all).unwrap();
        let closed = session.clipboard_slice().unwrap().unwrap();
        assert!(closed.is_closed());
        assert!(encode_metadata(&closed).is_err());
        let before = session.document().clone();
        let history = session.history_depths();
        cx.update(|cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string("previous".into()));
            PlatformClipboard::new(cx).write_slice(&closed);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("previous")
            );
            // The legacy open-slice interoperability contract stays unchanged.
            PlatformClipboard::new(cx).write_slice(&legacy);
            let item = cx.read_from_clipboard().unwrap();
            assert_eq!(item.text().as_deref(), Some(legacy.plain_text()));
            assert!(item.metadata().is_none());
        });
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.selection(), all);
        assert_eq!(session.history_depths(), history);
    }

    #[test]
    fn lossless_atomic_cut_writes_complete_payload_before_deleting_and_can_undo() {
        let (mut session, image) = atomic_session(2);
        let slice = session.clipboard_slice().unwrap().unwrap();
        let before_node = session.document().node(image).unwrap().clone();
        let before_history = session.history_depths();
        let written = write_lossless_slice(&slice, |item| {
            let text = item.text().unwrap();
            let metadata = item.metadata().unwrap();
            assert_eq!(decode_metadata(&text, metadata), Some(slice.clone()));
            assert!(session.document().node(image).is_some());
        });
        assert!(written);
        session.apply_intent(&EditIntent::Delete).unwrap();
        assert!(session.document().node(image).is_none());
        assert_eq!(session.history_depths(), (before_history.0 + 1, 0));
        session.undo().unwrap();
        assert_eq!(session.document().node(image), Some(&before_node));
    }
}

#[cfg(test)]
#[path = "platform_clipboard_mixed_tests.rs"]
mod mixed_tests;

#[cfg(test)]
#[path = "platform_clipboard_native_tests.rs"]
mod native_tests;
