//! P4.8 GPUI image block regressions.
//!
//! Image atomic blocks render a stateful placeholder driven by the host
//! asset resolver: neutral without a service, loading while a request is in
//! flight, resolved after the sink delivers. The block stays a whole-node
//! selectable atomic (P4.6 contract).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{AppContext as _, Modifiers, Point, TestAppContext, VisualTestContext, px};
use xiaomu_core::document::{
    ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_gpui::image_block::ImageLoadState;
use xiaomu_gpui::image_block::SharedImageAssetService;
use xiaomu_runtime::assets::{AssetFormat, AssetService, AssetSink, ResolvedAsset};

/// A valid 1x1 transparent PNG so the resolve exercises real bytes.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

struct ParkedService {
    parked: RefCell<Option<Rc<dyn AssetSink>>>,
}

impl AssetService for ParkedService {
    fn resolve(&self, _asset_ref: xiaomu_runtime::assets::AssetRef, sink: Rc<dyn AssetSink>) {
        self.parked.borrow_mut().replace(sink);
    }
}

impl ParkedService {
    fn deliver(&self, result: Result<ResolvedAsset, xiaomu_runtime::assets::AssetError>) {
        if let Some(sink) = self.parked.borrow_mut().take() {
            sink.resolved(result);
        }
    }
}

fn cover_attrs() -> NodeAttrs {
    ImageAttrs::new(
        ImageSource::AssetRef("host-media/2026/report-cover".to_owned()),
        "季度报告封面".to_owned(),
        None,
        Some(1280),
        Some(720),
    )
    .unwrap()
    .to_attrs()
    .unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("段", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let image = builder
        .insert(NodeKind::Image, cover_attrs(), NodeContent::Atomic)
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, image]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        image,
    )
}

fn offset_at(
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

#[gpui::test]
fn placeholder_renders_click_selects_and_resolve_lands(cx: &mut TestAppContext) {
    let (document, first, image) = fixture();
    let service = Rc::new(ParkedService {
        parked: RefCell::new(None),
    });
    let service_for_hooks: SharedImageAssetService = service.clone();

    let editor = EditorInstance::new(
        document.clone(),
        DocumentSelection::collapsed(TextPoint::new(
            first,
            offset_at(&document, first, 3),
            CursorAffinity::Before,
        )),
        EditorHooks {
            persistence: None,
            listener: None,
            atom_renderers: None,
            atom_capability: None,
            asset_service: Some(service_for_hooks),
        },
    )
    .unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();

    // The paint pass kicked one resolve; the sink is parked with the host.
    let mut view = VisualTestContext::from_window(window.into(), cx);
    window
        .update(cx, |_: &mut DocumentView, _, cx| {
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let state = window
        .update(cx, |view: &mut DocumentView, _, _| {
            view.image_load_state(image)
        })
        .unwrap();
    assert_eq!(state, Some(ImageLoadState::Loading));

    // The image block is a whole-node selectable atomic: clicking it selects
    // it instead of placing a text caret.
    view.simulate_click(Point::new(px(48.0), px(100.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    match session.borrow().selection().focus() {
        DocumentPosition::Atomic(node) => assert_eq!(node, image),
        other => panic!("image click must select the node: {other:?}"),
    }

    // While the placeholder still shows (96px box), the click above already
    // selected the node. Now the host delivers bytes; the state carries
    // identity + revision.
    service.deliver(Ok(ResolvedAsset::new(
        xiaomu_runtime::assets::AssetRef::new("host-media/2026/report-cover".to_owned()).unwrap(),
        3,
        AssetFormat::Png,
        TINY_PNG.to_vec(),
    )));
    window
        .update(cx, |_: &mut DocumentView, _, cx| {
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let state = window
        .update(cx, |view: &mut DocumentView, _, _| {
            view.image_load_state(image)
        })
        .unwrap();
    assert_eq!(
        state,
        Some(ImageLoadState::Resolved {
            revision: 3,
            byte_len: TINY_PNG.len(),
        })
    );

    // The fresh render source is available for the img() paint path.
    let source = window
        .update(cx, |view: &mut DocumentView, _, _| {
            view.image_render_source(image)
        })
        .unwrap();
    assert!(source.is_some(), "resolved render source must be present");
}
