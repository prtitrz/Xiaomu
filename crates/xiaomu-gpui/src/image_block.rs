//! Image block resolve pipeline and per-node load state (P4.8).
//!
//! The document never holds image bytes; the view resolves opaque
//! [`AssetRef`] sources through the host [`AssetService`] and keeps a
//! per-node load state for the placeholder renderer. Entries are keyed by
//! node identity and validated against the node's current source, so a
//! resolve that lands after an edit (or a changed source) is discarded as
//! stale instead of poisoning the view cache. Decoded texture painting
//! arrives with the follow-up slice; resolved bytes are only metered here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::Image as GpuiImage;
use gpui::{
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Styled as _,
    StyledImage as _, px,
};
use xiaomu_core::document::{ImageAttrs, ImageSource, NodeContent, NodeId, XiaomuDocument};
use xiaomu_runtime::assets::{
    AssetError, AssetFormat, AssetRef, AssetService, AssetSink, ResolvedAsset,
};

use crate::document_view::DocumentView;

/// Per-node load state of one image block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageLoadState {
    /// A resolve request is in flight.
    Loading,
    /// The host delivered bytes for one source revision.
    Resolved {
        /// Host-assigned source revision the payload carries.
        revision: u64,
        /// Byte length of the opaque payload; decoding is frontend work.
        byte_len: usize,
    },
    /// The resolve failed with the host-neutral failure.
    Failed(AssetError),
}

struct ImageLoadEntry {
    state: ImageLoadState,
    /// The source key this state belongs to; a changed source is stale.
    source_key: String,
}

/// Per-node load states for every image block in one document view.
#[derive(Default)]
pub struct ImageLoadCache {
    entries: RefCell<HashMap<NodeId, ImageLoadEntry>>,
    /// Decoded-ready render sources per node, validated against the source
    /// key like the states above.
    render_sources: RefCell<HashMap<NodeId, (String, Arc<GpuiImage>)>>,
}

impl ImageLoadCache {
    /// Returns the fresh state for `node`, or `None` when no request was
    /// made or the state belongs to a different source.
    #[must_use]
    pub fn fresh_state(&self, node: NodeId, source_key: &str) -> Option<ImageLoadState> {
        self.entries
            .borrow()
            .get(&node)
            .filter(|entry| entry.source_key == source_key)
            .map(|entry| entry.state.clone())
    }

    /// Marks a request as in flight.
    pub fn begin_load(&self, node: NodeId, source_key: String) {
        self.entries.borrow_mut().insert(
            node,
            ImageLoadEntry {
                state: ImageLoadState::Loading,
                source_key,
            },
        );
    }

    /// Returns the render source for `node`, or `None` when absent or stale.
    #[must_use]
    pub fn render_source(&self, node: NodeId, source_key: &str) -> Option<Arc<GpuiImage>> {
        self.entries.borrow();
        self.render_sources
            .borrow()
            .get(&node)
            .filter(|(key, _)| key == source_key)
            .map(|(_, image)| Arc::clone(image))
    }

    /// Stores the render source for one fresh resolve.
    pub fn store_render_source(&self, node: NodeId, source_key: String, image: Arc<GpuiImage>) {
        self.render_sources
            .borrow_mut()
            .insert(node, (source_key, image));
    }

    /// Applies a resolve outcome, dropping stale results whose node moved on
    /// to a different source while the host worked.
    pub fn finish(&self, node: NodeId, source_key: &str, state: ImageLoadState) {
        let mut entries = self.entries.borrow_mut();
        if let Some(entry) = entries.get_mut(&node)
            && entry.source_key == source_key
        {
            entry.state = state;
        }
    }
}

/// The shared cache handle sinks and the view both hold.
pub(crate) type SharedImageLoadCache = Rc<ImageLoadCache>;

/// Shared handle for the host asset resolver wiring.
pub type SharedImageAssetService = Rc<dyn AssetService>;

/// Sink bound to one node identity and source; stale outcomes are dropped.
///
/// The callback carries no frontend context, so it only lands the outcome in
/// the cache; re-render scheduling after a resolve belongs to the host
/// integration, which runs the callback inside its own application context.
struct NodeImageSink {
    cache: SharedImageLoadCache,
    node: NodeId,
    source_key: String,
}

impl AssetSink for NodeImageSink {
    fn resolved(self: Rc<Self>, result: Result<ResolvedAsset, AssetError>) {
        let state = match result {
            Ok(resolved) => {
                let format = match resolved.format() {
                    AssetFormat::Png => gpui::ImageFormat::Png,
                    AssetFormat::Jpeg => gpui::ImageFormat::Jpeg,
                };
                let image = Arc::new(GpuiImage::from_bytes(format, resolved.bytes().to_vec()));
                self.cache
                    .store_render_source(self.node, self.source_key.clone(), image);
                ImageLoadState::Resolved {
                    revision: resolved.revision(),
                    byte_len: resolved.bytes().len(),
                }
            }
            Err(error) => ImageLoadState::Failed(error),
        };
        self.cache.finish(self.node, &self.source_key, state);
    }
}

/// Collects every Image atomic block, depth-first from the root.
pub(crate) fn image_nodes(document: &XiaomuDocument) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    walk(document, document.root(), &mut nodes);
    nodes
}

fn walk(document: &XiaomuDocument, id: NodeId, nodes: &mut Vec<NodeId>) {
    let Some(node) = document.node(id) else {
        return;
    };
    match node.content() {
        NodeContent::Atomic if matches!(node.kind(), xiaomu_core::document::NodeKind::Image) => {
            nodes.push(id);
        }
        NodeContent::Children(children) => {
            for child in children {
                walk(document, *child, nodes);
            }
        }
        _ => {}
    }
}

/// Kicks one resolve request per stale image block.
///
/// Called from the paint pass; requests are idempotent per source because an
/// in-flight entry short-circuits the walk.
pub(crate) fn sync_image_loads(
    document: &XiaomuDocument,
    cache: &SharedImageLoadCache,
    service: Option<&Rc<dyn AssetService>>,
) {
    let Some(service) = service else {
        return;
    };
    for node in image_nodes(document) {
        let Some(node_data) = document.node(node) else {
            continue;
        };
        let Ok(attrs) = ImageAttrs::from_attrs(node_data.attrs()) else {
            continue;
        };
        let ImageSource::AssetRef(value) = attrs.source() else {
            // External URLs are host-imported; the neutral placeholder shows
            // until a host contract for URL fetches exists.
            continue;
        };
        let source_key = value.clone();
        if cache.fresh_state(node, &source_key).is_some() {
            continue;
        }
        let Ok(asset_ref) = AssetRef::new(source_key.clone()) else {
            cache.finish(
                node,
                &source_key,
                ImageLoadState::Failed(AssetError::InvalidRef),
            );
            continue;
        };
        cache.begin_load(node, source_key.clone());
        service.resolve(
            asset_ref,
            Rc::new(NodeImageSink {
                cache: Rc::clone(cache),
                node,
                source_key,
            }),
        );
    }
}

/// Everything the image block renderer needs for one paint pass.
pub struct ImageBlockPresentation {
    /// Whether the block carries the active whole-node selection.
    pub selected: bool,
    /// Placeholder label (alt text plus state prefix).
    pub label: String,
    /// Placeholder background tint.
    pub state_color: gpui::Rgba,
    /// The decoded-ready render source, when a fresh resolve landed.
    pub source: Option<Arc<GpuiImage>>,
}

/// Renders one image block: the resolved texture when available, otherwise
/// the stateful placeholder. The block stays whole-node selectable either
/// way; intrinsic aspect ratio comes from the decoded source, capped to a
/// display height.
pub(crate) fn render_image_block(
    node: NodeId,
    index: usize,
    presentation: &ImageBlockPresentation,
    cx: &mut gpui::Context<DocumentView>,
) -> gpui::AnyElement {
    let border = if presentation.selected {
        gpui::rgba(0x2b6cb8ff)
    } else {
        gpui::rgba(0x00000000)
    };
    let interactive = gpui::div()
        .id(("atomic-block", index))
        .w_full()
        .my_3()
        .border_2()
        .border_color(border)
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.select_atomic_block(node, window, cx);
            }),
        );

    match &presentation.source {
        Some(image) => interactive
            .child(
                gpui::img(gpui::ImageSource::Image(Arc::clone(image)))
                    .w_full()
                    .max_h(Pixels::from(320.0))
                    .object_fit(gpui::ObjectFit::Contain),
            )
            .into_any_element(),
        None => interactive
            .h(px(96.0))
            .bg(presentation.state_color)
            .child(
                gpui::div()
                    .px_3()
                    .py_2()
                    .child(presentation.label.clone())
                    .text_size(px(14.0))
                    .text_color(gpui::rgba(0x555555ff)),
            )
            .into_any_element(),
    }
}
