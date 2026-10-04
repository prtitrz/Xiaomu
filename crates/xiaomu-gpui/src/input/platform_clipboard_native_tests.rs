//! Transport-only coverage for combinations GPUI cannot construct publicly.
//!
//! Actual native-metadata paste actions are covered by document_view tests.

use super::*;

#[test]
fn rejected_native_never_falls_back_to_pixels_or_absent_empty_plain_text() {
    for text in [None, Some(""), Some("unsafe fallback")] {
        for format in [gpui::ImageFormat::Png, gpui::ImageFormat::Jpeg] {
            let decoded =
                decode_metadata_checked(text.unwrap_or_default(), "xiaomu.clipboard.v14\n{");
            assert!(matches!(decoded, ClipboardMetadataDecode::RejectedNative));
            assert!(
                decode_transport(
                    text.map(str::to_owned),
                    decoded,
                    [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                        format,
                        vec![1, 2, 3]
                    ))],
                )
                .is_none()
            );
        }
    }
}

#[test]
fn recognized_native_prefix_without_a_text_flavor_is_rejected() {
    let metadata = "xiaomu.clipboard.v14\n{\"version\":999}";
    let decoded = decode_metadata_checked("", metadata);
    assert!(matches!(decoded, ClipboardMetadataDecode::RejectedNative));
    assert!(decode_transport(None, decoded, []).is_none());
}

#[test]
fn validated_empty_slice_still_requires_a_platform_text_flavor() {
    use xiaomu_core::document::{
        InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument,
    };
    use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let all = DocumentSelection::all(&document);
    let slice = DocumentSession::new(document, all)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    assert!(slice.plain_text().is_empty());
    assert!(
        decode_transport(
            None,
            ClipboardMetadataDecode::Valid(slice),
            [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                vec![1]
            ))],
        )
        .is_none()
    );
}

#[test]
fn native_rejection_does_not_even_visit_fallback_entries() {
    let entries = std::iter::from_fn(|| -> Option<gpui::ClipboardEntry> {
        panic!("native metadata rejection must not examine another flavor")
    });
    assert!(
        decode_transport(
            Some("must not paste".into()),
            decode_metadata_checked("must not paste", "xiaomu.clipboard.v14\nnull"),
            entries,
        )
        .is_none()
    );
}

#[test]
fn foreign_metadata_keeps_the_image_first_fallback() {
    let content = decode_transport(
        Some("ordinary".into()),
        decode_metadata_checked("ordinary", "foreign"),
        [gpui::ClipboardEntry::Image(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![1, 2, 3],
        ))],
    );
    assert!(
        matches!(content, Some(PlatformClipboardContent::Image { plain_text, .. }) if plain_text.as_deref() == Some("ordinary"))
    );
}
