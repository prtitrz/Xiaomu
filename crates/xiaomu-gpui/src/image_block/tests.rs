use super::*;
use xiaomu_core::document::{NodeAttrs, NodeKind, NodeStoreBuilder};

#[test]
fn image_load_cache_keeps_its_send_contract() {
    fn assert_send<T: Send>() {}
    assert_send::<ImageLoadCache>();
}

fn node() -> NodeId {
    NodeStoreBuilder::new()
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap()
}
fn sink(cache: &Rc<ImageLoadCache>, node: NodeId, key: &str) -> Rc<NodeImageSink> {
    Rc::new(NodeImageSink {
        cache: cache.clone(),
        node,
        source_key: key.into(),
        request: cache.entries.borrow()[&node].request.clone(),
    })
}
fn payload(key: &str, bytes: Vec<u8>) -> Result<ResolvedAsset, AssetError> {
    Ok(ResolvedAsset::new(
        AssetRef::new(key.into()).unwrap(),
        1,
        AssetFormat::Png,
        bytes,
    ))
}
#[test]
fn late_old_source_cannot_overwrite_current_render_source() {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    cache.begin_load(node, "old".into());
    let old = sink(&cache, node, "old");
    cache.begin_load(node, "new".into());
    sink(&cache, node, "new").resolved(payload("new", vec![2]));
    old.resolved(payload("old", vec![1]));
    assert_eq!(cache.render_source(node, "new").unwrap().bytes, [2]);
    assert!(cache.render_source(node, "old").is_none());
}
#[test]
fn mismatched_asset_identity_is_rejected_and_source_change_clears_texture() {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    cache.begin_load(node, "old".into());
    sink(&cache, node, "old").resolved(payload("old", vec![1]));
    cache.begin_load(node, "new".into());
    assert!(cache.render_source(node, "old").is_none());
    sink(&cache, node, "new").resolved(payload("wrong", vec![2]));
    assert_eq!(
        cache.fresh_state(node, "new"),
        Some(ImageLoadState::Failed(AssetError::InvalidRef))
    );
    assert!(cache.render_source(node, "new").is_none());
}

#[test]
fn returning_to_same_source_does_not_revive_an_old_request() {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    cache.begin_load(node, "same".into());
    let old = sink(&cache, node, "same");
    cache.begin_load(node, "other".into());
    let intermediate = sink(&cache, node, "other");
    cache.begin_load(node, "same".into());
    sink(&cache, node, "same").resolved(payload("same", vec![3]));
    old.resolved(payload("same", vec![1]));
    intermediate.resolved(Err(AssetError::NotFound));
    assert_eq!(cache.render_source(node, "same").unwrap().bytes, [3]);
    assert_eq!(
        cache.fresh_state(node, "same"),
        Some(ImageLoadState::Resolved {
            revision: 1,
            byte_len: 1
        })
    );
}

#[test]
fn deleted_image_drops_request_and_rejects_late_resolve() {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    cache.begin_load(node, "deleted".into());
    let pending = sink(&cache, node, "deleted");
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
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
    sync_image_loads(&document, &cache, None);
    pending.resolved(payload(
        "deleted",
        include_bytes!("fixtures/landscape.png").to_vec(),
    ));
    assert!(cache.fresh_state(node, "deleted").is_none());
    assert!(cache.render_source(node, "deleted").is_none());
}

#[test]
fn clearing_for_new_session_invalidates_same_node_and_source_request() {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    cache.begin_load(node, "same".into());
    let stale = sink(&cache, node, "same");
    cache.clear();
    cache.begin_load(node, "same".into());
    sink(&cache, node, "same").resolved(payload(
        "same",
        include_bytes!("fixtures/tall.png").to_vec(),
    ));
    stale.resolved(payload(
        "same",
        include_bytes!("fixtures/landscape.png").to_vec(),
    ));
    assert_eq!(
        cache.render_source(node, "same").unwrap().bytes,
        include_bytes!("fixtures/tall.png")
    );
}
