//! P4.7 Image canonical contract regressions.
//!
//! Typed image attrs stay frontend-neutral: an opaque asset reference or an
//! external URL, alternative text, and optional metadata. The InsertImage
//! command inserts a `NodeKind::Image` atomic block as a sibling of the
//! focused block with the caret preserved. The AssetService capability seam
//! resolves references through host-owned callbacks without async runtimes
//! or frontend types.

use std::cell::RefCell;
use std::rc::Rc;

use xiaomu_core::document::{
    ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::assets::{AssetError, AssetRef, AssetService, AssetSink, ResolvedAsset};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn offset_of(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    let text: String = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    TextBuffer::from_string(text).offset_at(byte).unwrap()
}

fn paragraph_document(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

fn session_at(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentSession {
    let point = DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_of(document, node, byte),
        0,
        CursorAffinity::Before,
    ));
    DocumentSession::new(document.clone(), DocumentSelection::collapsed(point)).unwrap()
}

fn sample_image() -> ImageAttrs {
    ImageAttrs::new(
        ImageSource::AssetRef("host-media/2026/report-cover".to_owned()),
        "季度报告封面".to_owned(),
        Some("封面".to_owned()),
        Some(1280),
        Some(720),
    )
    .unwrap()
}

#[test]
fn image_attrs_round_trip_through_canonical_attrs() {
    let image = sample_image();
    let attrs = image.to_attrs().unwrap();

    let parsed = ImageAttrs::from_attrs(&attrs).unwrap();
    assert_eq!(parsed, image);
    assert_eq!(
        parsed.source(),
        &ImageSource::AssetRef("host-media/2026/report-cover".to_owned())
    );
    assert_eq!(parsed.alt(), "季度报告封面");
    assert_eq!(parsed.title().map(String::as_str), Some("封面"));
    assert_eq!(parsed.width(), Some(1280));
    assert_eq!(parsed.height(), Some(720));

    let url_image = ImageAttrs::new(
        ImageSource::ExternalUrl("https://example.invalid/x.png".to_owned()),
        "徽标".to_owned(),
        None,
        None,
        None,
    )
    .unwrap();
    let parsed = ImageAttrs::from_attrs(&url_image.to_attrs().unwrap()).unwrap();
    assert_eq!(parsed, url_image);
}

#[test]
fn image_attrs_reject_ambiguous_or_empty_contracts() {
    let empty_source = ImageAttrs::new(
        ImageSource::AssetRef("  ".to_owned()),
        "alt".to_owned(),
        None,
        None,
        None,
    );
    assert!(empty_source.is_err());

    let empty_alt = ImageAttrs::new(
        ImageSource::ExternalUrl("https://example.invalid".to_owned()),
        "  ".to_owned(),
        None,
        None,
        None,
    );
    assert!(empty_alt.is_err());

    // Both source keys at once is ambiguous.
    let mut values = std::collections::BTreeMap::new();
    values.insert(
        "src".to_owned(),
        xiaomu_core::document::AttrValue::String("https://example.invalid".to_owned()),
    );
    values.insert(
        "asset".to_owned(),
        xiaomu_core::document::AttrValue::String("ref".to_owned()),
    );
    values.insert(
        "alt".to_owned(),
        xiaomu_core::document::AttrValue::String("x".to_owned()),
    );
    let attrs = NodeAttrs::new(values).unwrap();
    assert!(ImageAttrs::from_attrs(&attrs).is_err());

    // Dimensions must be positive integers.
    let zero = ImageAttrs::new(
        ImageSource::AssetRef("r".to_owned()),
        "alt".to_owned(),
        None,
        Some(0),
        None,
    );
    assert!(zero.is_err());
}

#[test]
fn insert_image_command_adds_a_typed_sibling_block() {
    let (document, paragraph) = paragraph_document("段落");
    let mut session = session_at(&document, paragraph, 0);

    let outcome = session
        .apply_intent(&EditIntent::InsertImage {
            image: sample_image(),
        })
        .unwrap();
    assert_eq!(outcome, SessionOutcome::DocumentChanged);

    // The image block inserts right after the focused paragraph.
    let root = session.document().root();
    let children = session
        .document()
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    assert_eq!(children.len(), 2);
    let image = children[1];
    assert!(matches!(
        session.document().node(image).unwrap().kind(),
        NodeKind::Image
    ));
    assert!(
        session
            .document()
            .node(image)
            .unwrap()
            .content()
            .is_atomic()
    );

    // The typed semantics survive the canonical attrs.
    let parsed = ImageAttrs::from_attrs(session.document().node(image).unwrap().attrs()).unwrap();
    assert_eq!(parsed, sample_image());

    // The caret stays where it was, and undo removes the block again.
    match session.selection().focus() {
        DocumentPosition::Inline(point) => {
            assert_eq!(point.node_id(), paragraph);
            assert_eq!(point.text_offset().as_usize(), 0);
        }
        other => panic!("caret must stay inline after InsertImage: {other:?}"),
    }
    session.undo().unwrap();
    assert_eq!(
        session
            .document()
            .node(root)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
            .len(),
        1
    );
}

/// Fake host service: parks the sink until the test resolves it.
struct FakeAssetService {
    pending: RefCell<Option<Rc<dyn AssetSink>>>,
}

impl AssetService for FakeAssetService {
    fn resolve(&self, asset_ref: AssetRef, sink: Rc<dyn AssetSink>) {
        self.pending.borrow_mut().replace(sink);
        let _ = asset_ref;
    }
}

struct CollectingSink {
    result: RefCell<Option<Result<ResolvedAsset, AssetError>>>,
}

impl AssetSink for CollectingSink {
    fn resolved(self: Rc<Self>, result: Result<ResolvedAsset, AssetError>) {
        self.result.borrow_mut().replace(result);
    }
}

#[test]
fn asset_service_seam_resolves_opaque_references() {
    let service = FakeAssetService {
        pending: RefCell::new(None),
    };
    let sink = Rc::new(CollectingSink {
        result: RefCell::new(None),
    });

    let reference = AssetRef::new("host-media/2026/report-cover".to_owned()).unwrap();
    service.resolve(reference.clone(), sink.clone());
    assert!(service.pending.borrow().is_some());

    // The host delivers bytes for the current source revision.
    service.pending.borrow_mut().take();
    sink.clone().resolved(Ok(ResolvedAsset::new(
        reference.clone(),
        7,
        xiaomu_runtime::assets::AssetFormat::Png,
        vec![1, 2, 3],
    )));
    let delivered = sink.result.borrow().as_ref().unwrap().clone().unwrap();
    assert_eq!(delivered.asset_ref(), &reference);
    assert_eq!(delivered.revision(), 7);
    assert_eq!(delivered.bytes(), &[1, 2, 3]);

    // Empty references are rejected before they reach the host.
    assert_eq!(AssetRef::new("   ".to_owned()), Err(AssetError::InvalidRef));
}
