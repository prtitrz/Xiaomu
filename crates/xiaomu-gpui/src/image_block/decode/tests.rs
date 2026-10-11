//! Actual async GPUI decode completion cannot revive replaced requests.
use super::*;
use crate::image_block::{ImageLoadCache, ImageLoadState};
use gpui::{AppContext, Context, IntoElement, Render, TestAppContext};
use std::rc::Rc;
use xiaomu_core::document::{NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder};

type DuringPrepaint = Box<dyn FnOnce(&mut Window, &mut App)>;
struct Host {
    callback: Option<DuringPrepaint>,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let callback = self.callback.take();
        gpui::canvas(
            move |_, window, cx| {
                if let Some(callback) = callback {
                    callback(window, cx);
                }
            },
            |_, (), _, _| {},
        )
    }
}

fn node() -> NodeId {
    NodeStoreBuilder::new()
        .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
        .unwrap()
}

fn start(
    cache: &SharedImageLoadCache,
    node: NodeId,
    key: &str,
    bytes: &[u8],
    window: &mut Window,
    cx: &mut App,
) {
    cache.begin_load(node, key.into());
    cache.store_render_source(
        node,
        key.into(),
        Arc::new(Image::from_bytes(gpui::ImageFormat::Png, bytes.to_vec())),
    );
    assert!(decoded_image(cache, node, key, window, cx).is_none());
}

#[gpui::test]
fn pending_decode_cannot_overwrite_a_to_b_to_a_or_revive_deleted_request(cx: &mut TestAppContext) {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| Host { callback: None })
        })
        .unwrap()
    });
    let task_cache = cache.clone();
    handle
        .update(cx, move |host, _, cx| {
            host.callback = Some(Box::new(move |window, cx| {
                let cache = task_cache;
                start(
                    &cache,
                    node,
                    "a",
                    include_bytes!("../fixtures/landscape.png"),
                    window,
                    cx,
                );
                start(
                    &cache,
                    node,
                    "b",
                    include_bytes!("../fixtures/wide.png"),
                    window,
                    cx,
                );
                start(
                    &cache,
                    node,
                    "a",
                    include_bytes!("../fixtures/tall.png"),
                    window,
                    cx,
                );
                assert!(cache.entries.borrow()[&node].decoded.is_none());
            }));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let decoded = cache.entries.borrow()[&node]
        .decoded
        .clone()
        .unwrap()
        .unwrap();
    assert_eq!(decoded.size(0).width.0, 80);
    assert_eq!(decoded.size(0).height.0, 800);
    assert_eq!(cache.fresh_state(node, "a"), Some(ImageLoadState::Loading));
    let task_cache = cache.clone();
    handle
        .update(cx, move |host, _, cx| {
            host.callback = Some(Box::new(move |window, cx| {
                let cache = task_cache;
                start(
                    &cache,
                    node,
                    "deleted",
                    include_bytes!("../fixtures/landscape.png"),
                    window,
                    cx,
                );
                cache.discard(node);
            }));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(!cache.entries.borrow().contains_key(&node));
    assert!(cache.render_source(node, "deleted").is_none());
}

#[gpui::test]
fn pending_decode_is_guarded_by_exact_render_source_even_with_same_request(
    cx: &mut TestAppContext,
) {
    let cache = Rc::new(ImageLoadCache::default());
    let node = node();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| Host { callback: None })
        })
        .unwrap()
    });
    let task_cache = cache.clone();
    handle
        .update(cx, move |host, _, cx| {
            host.callback = Some(Box::new(move |window, cx| {
                let cache = task_cache;
                start(
                    &cache,
                    node,
                    "a",
                    include_bytes!("../fixtures/landscape.png"),
                    window,
                    cx,
                );
                cache.store_render_source(
                    node,
                    "a".into(),
                    Arc::new(Image::from_bytes(
                        gpui::ImageFormat::Png,
                        include_bytes!("../fixtures/tall.png").to_vec(),
                    )),
                );
                assert!(decoded_image(&cache, node, "a", window, cx).is_none());
            }));
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let decoded = cache.entries.borrow()[&node]
        .decoded
        .clone()
        .unwrap()
        .unwrap();
    assert_eq!(decoded.size(0).height.0, 800);
}
