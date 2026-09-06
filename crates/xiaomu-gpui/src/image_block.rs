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

use xiaomu_core::document::{ImageAttrs, ImageSource, NodeContent, NodeId, XiaomuDocument};
use xiaomu_runtime::assets::{AssetError, AssetRef, AssetService, AssetSink, ResolvedAsset};

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
            Ok(resolved) => ImageLoadState::Resolved {
                revision: resolved.revision(),
                byte_len: resolved.bytes().len(),
            },
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
