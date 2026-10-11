//! Per-view completion delivery over GPUI's shared public image decoder.

use super::SharedImageLoadCache;
use gpui::{App, Asset, Image, RenderImage, Window};
use std::sync::Arc;
use xiaomu_core::document::NodeId;

/// GPUI's stock image asset notifies only its first requesting view. Keep the
/// same decoder and shared asset work, but await completion once in every fresh
/// node request so another pane/window cannot remain at its loading geometry.
enum DecodedImage {}

impl Asset for DecodedImage {
    type Source = Arc<Image>;
    type Output = Result<Arc<RenderImage>, ()>;

    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move { source.to_image_data(renderer).map_err(|_| ()) }
    }
}

pub(super) fn decoded_image(
    cache: &SharedImageLoadCache,
    node: NodeId,
    source: &str,
    window: &mut Window,
    cx: &mut App,
) -> Option<Result<Arc<RenderImage>, ()>> {
    let image = cache.render_source(node, source)?;
    let request = {
        let mut entries = cache.entries.borrow_mut();
        let entry = entries
            .get_mut(&node)
            .filter(|entry| entry.source_key == source)?;
        if entry.decode_started {
            return entry.decoded.clone();
        }
        entry.decode_started = true;
        entry.request.clone()
    };
    let (task, _) = cx.fetch_asset::<DecodedImage>(&image);
    let cache = cache.clone();
    let source = source.to_owned();
    let view = window.current_view();
    window
        .spawn(cx, async move |cx| {
            let result = task.await;
            let _ = cx.update(move |_, cx| {
                if !cache.request_is_current(node, &source, &request)
                    || !cache
                        .render_source(node, &source)
                        .is_some_and(|current| Arc::ptr_eq(&current, &image))
                {
                    return;
                }
                if let Some(entry) = cache.entries.borrow_mut().get_mut(&node) {
                    entry.decoded = Some(result);
                }
                cx.notify(view);
            });
        })
        .detach();
    None
}

#[cfg(test)]
mod tests;
