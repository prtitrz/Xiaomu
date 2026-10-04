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
        // Explicit whole-root boundaries cannot survive a plain-text fallback.
        // Preserve the existing clipboard when their metadata is not lossless.
        if slice.is_closed() {
            if !write_lossless_slice(slice, |item| self.app.write_to_clipboard(item)) {
                eprintln!("xiaomu: closed clipboard copy requires lossless structured metadata");
            }
            return;
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
    }

    /// Writes only a fully round-trippable structured slice for Cut.
    ///
    /// A failed encode or decode must not replace the platform clipboard or
    /// authorize removal of canonical source content. Ordinary Copy keeps
    /// its interoperable plain-text fallback through `write_slice`.
    pub(crate) fn write_slice_for_cut(&mut self, slice: &ClipboardSlice) -> bool {
        write_lossless_slice(slice, |item| self.app.write_to_clipboard(item))
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

/// The write callback is reached only after exact structured round-trip
/// validation, including the decoder's untrusted-metadata limits.
fn write_lossless_slice(slice: &ClipboardSlice, write: impl FnOnce(gpui::ClipboardItem)) -> bool {
    let Ok(metadata) = encode_metadata(slice) else {
        return false;
    };
    if decode_metadata(slice.plain_text(), &metadata).as_ref() != Some(slice) {
        return false;
    }
    write(gpui::ClipboardItem::new_string_with_metadata(
        slice.plain_text().to_owned(),
        metadata,
    ));
    true
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

#[cfg(test)]
mod cut_tests {
    use super::*;
    use std::cell::Cell;
    use xiaomu_core::document::{
        AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    };
    use xiaomu_core::selection::{CursorAffinity, InlinePoint};
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
