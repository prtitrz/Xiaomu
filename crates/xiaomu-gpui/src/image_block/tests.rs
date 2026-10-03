use super::*;
use xiaomu_core::document::{NodeAttrs, NodeKind, NodeStoreBuilder};
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
