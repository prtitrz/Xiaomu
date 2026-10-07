//! Nullable raw metadata uses one display contract throughout the image view.
//! These exercise resolve/cache wiring, not native decoding or pixel geometry.

use super::*;
use crate::accessibility::{AccessibilityRole, project_accessibility};
use crate::editor::{EditorHooks, EditorInstance};
use gpui::{AppContext as _, TestAppContext};
use std::collections::BTreeMap;
use std::sync::Arc;
use xiaomu_core::document::{AttrValue, NodeContent, NodeKind, NodeStoreBuilder};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::assets::{AssetError, AssetFormat, AssetRef, AssetSink, ResolvedAsset};
use xiaomu_runtime::session::DocumentSelection;

#[derive(Default)]
struct PendingResolver {
    requests: RefCell<Vec<(AssetRef, Rc<dyn AssetSink>)>>,
}

impl AssetService for PendingResolver {
    fn resolve(&self, asset_ref: AssetRef, sink: Rc<dyn AssetSink>) {
        self.requests.borrow_mut().push((asset_ref, sink));
    }
}

impl PendingResolver {
    fn finish(&self, index: usize, result: Result<ResolvedAsset, AssetError>) {
        let sink = self.requests.borrow()[index].1.clone();
        sink.resolved(result);
    }
}

fn all_null(source_key: &str, source: &str) -> NodeAttrs {
    NodeAttrs::new(BTreeMap::from([
        (source_key.into(), AttrValue::String(source.into())),
        ("alt".into(), AttrValue::Null),
        ("title".into(), AttrValue::Null),
        ("width".into(), AttrValue::Null),
        ("height".into(), AttrValue::Null),
    ]))
    .unwrap()
}

fn changed(attrs: &NodeAttrs, key: &str, value: Option<AttrValue>) -> NodeAttrs {
    let mut values = attrs
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    if let Some(value) = value {
        values.insert(key.into(), value);
    } else {
        values.remove(key);
    }
    NodeAttrs::new(values).unwrap()
}

fn fixture(attrs: NodeAttrs, resolver: Option<Rc<PendingResolver>>) -> (DocumentView, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let image = builder
        .insert(NodeKind::Image, attrs, NodeContent::Atomic)
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([image]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let hooks = EditorHooks {
        asset_service: resolver.map(|resolver| resolver as Rc<dyn AssetService>),
        ..EditorHooks::default()
    };
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(DocumentPosition::Atomic(image)),
        hooks,
    )
    .unwrap();
    (editor.build_view(), image)
}

fn sync(view: &DocumentView) {
    sync_image_loads(
        view.session.borrow().document(),
        &view.image_loads,
        view.asset_service.as_ref(),
    );
}

fn assert_raw_and_a11y(view: &DocumentView, image: NodeId, raw: &NodeAttrs, alt: Option<&str>) {
    let session = view.session.borrow();
    let projection = project_accessibility(session.document(), session.selection(), None).unwrap();
    let projected = &projection.root().children()[0];
    assert_eq!(projected.node_id(), image);
    assert_eq!(projected.role(), &AccessibilityRole::Image);
    assert_eq!(projected.text(), alt);
    assert!(!projected.editable());
    assert_eq!(session.document().node(image).unwrap().attrs(), raw);
}

fn payload(source: &str, revision: u64, byte: u8) -> Result<ResolvedAsset, AssetError> {
    Ok(ResolvedAsset::new(
        AssetRef::new(source.into()).unwrap(),
        revision,
        AssetFormat::Png,
        vec![byte],
    ))
}

#[gpui::test]
fn nullable_images_resolve_and_agree_across_state_source_placeholder_and_a11y(
    cx: &mut TestAppContext,
) {
    for (alt, expected) in [
        (None, None),
        (Some(AttrValue::Null), None),
        (Some(AttrValue::String("".into())), Some("")),
        (Some(AttrValue::String(" \t".into())), Some(" \t")),
        (Some(AttrValue::String("封面👩‍💻".into())), Some("封面👩‍💻")),
    ] {
        let raw = changed(&all_null("asset", "sample"), "alt", alt);
        let resolver = Rc::new(PendingResolver::default());
        let (view, image) = fixture(raw.clone(), Some(resolver.clone()));
        let view = cx.new(|_| view);
        view.update(cx, |view, _| {
            let label = expected.unwrap_or("图片");
            assert_eq!(view.image_load_state(image), None);
            assert!(view.image_render_source(image).is_none());
            assert_eq!(view.image_placeholder_presentation(image).0, label);
            assert_raw_and_a11y(view, image, &raw, expected);
            sync(view);
            assert_eq!(resolver.requests.borrow().len(), 1);
            assert_eq!(resolver.requests.borrow()[0].0.value(), "sample");
            assert_eq!(view.image_load_state(image), Some(ImageLoadState::Loading));
            assert_eq!(
                view.image_placeholder_presentation(image).0,
                format!("加载中：{label}")
            );
            sync(view);
            assert_eq!(resolver.requests.borrow().len(), 1);
            resolver.finish(0, payload("sample", 7, 42));
            assert_eq!(
                view.image_load_state(image),
                Some(ImageLoadState::Resolved {
                    revision: 7,
                    byte_len: 1
                })
            );
            assert_eq!(view.image_render_source(image).unwrap().bytes, [42]);
            assert_eq!(
                view.image_placeholder_presentation(image).0,
                format!("已解析：{label}")
            );
            assert_raw_and_a11y(view, image, &raw, expected);
            assert_eq!(view.session.borrow().history_depths(), (0, 0));
        });
    }
}

#[gpui::test]
fn nullable_images_keep_failure_identity_and_stale_source_guards(cx: &mut TestAppContext) {
    for result in [Err(AssetError::NotFound), payload("wrong", 1, 42)] {
        let raw = all_null("asset", "sample");
        let resolver = Rc::new(PendingResolver::default());
        let (view, image) = fixture(raw.clone(), Some(resolver.clone()));
        let view = cx.new(|_| view);
        view.update(cx, |view, _| {
            sync(view);
            let expected = if result.is_ok() {
                AssetError::InvalidRef
            } else {
                AssetError::NotFound
            };
            resolver.finish(0, result);
            assert_eq!(
                view.image_load_state(image),
                Some(ImageLoadState::Failed(expected))
            );
            assert!(view.image_render_source(image).is_none());
            assert_eq!(
                view.image_placeholder_presentation(image).0,
                "加载失败：图片"
            );
            assert_raw_and_a11y(view, image, &raw, None);
        });
    }

    let resolver = Rc::new(PendingResolver::default());
    let (view, image) = fixture(all_null("asset", "old"), Some(resolver.clone()));
    let view = cx.new(|_| view);
    view.update(cx, |view, _| {
        sync(view);
        let current = all_null("asset", "new");
        replace_attrs(view, image, current.clone());
        assert_eq!(view.image_load_state(image), None);
        assert!(view.image_render_source(image).is_none());
        sync(view);
        assert_eq!(resolver.requests.borrow().len(), 2);
        resolver.finish(1, payload("new", 2, 22));
        resolver.finish(0, payload("old", 1, 11));
        assert_eq!(view.image_render_source(image).unwrap().bytes, [22]);
        assert_eq!(
            view.image_load_state(image),
            Some(ImageLoadState::Resolved {
                revision: 2,
                byte_len: 1
            })
        );
        assert_raw_and_a11y(view, image, &current, None);

        // Only metadata changes: retain the exact cached source and request count.
        let source = view.image_render_source(image).unwrap();
        let metadata = changed(&current, "alt", Some(AttrValue::String("".into())));
        let metadata = changed(&metadata, "title", None);
        let metadata = changed(&metadata, "width", Some(AttrValue::Integer(240)));
        replace_attrs(view, image, metadata.clone());
        sync(view);
        assert_eq!(resolver.requests.borrow().len(), 2);
        assert!(Arc::ptr_eq(
            &source,
            &view.image_render_source(image).unwrap()
        ));
        assert_eq!(view.image_placeholder_presentation(image).0, "已解析：");
        assert_raw_and_a11y(view, image, &metadata, Some(""));
    });
}

fn replace_attrs(view: &DocumentView, image: NodeId, attrs: NodeAttrs) {
    view.session
        .borrow_mut()
        .apply(
            &Transaction::new(TransactionOrigin::UserInput)
                .with_step(TransactionStep::SetNodeAttrs { node: image, attrs }),
        )
        .unwrap();
}

#[gpui::test]
fn nullable_external_urls_are_never_fetched_and_no_service_stays_neutral(cx: &mut TestAppContext) {
    let resolver = Rc::new(PendingResolver::default());
    for (raw, service) in [
        (
            all_null("src", "https://example.invalid/image.png"),
            Some(resolver.clone()),
        ),
        (all_null("asset", "sample"), None),
    ] {
        let (view, image) = fixture(raw.clone(), service);
        let view = cx.new(|_| view);
        view.update(cx, |view, _| {
            sync(view);
            assert_eq!(resolver.requests.borrow().len(), 0);
            assert_eq!(view.image_load_state(image), None);
            assert!(view.image_render_source(image).is_none());
            assert_eq!(view.image_placeholder_presentation(image).0, "图片");
            assert_raw_and_a11y(view, image, &raw, None);
        });
    }
}

#[gpui::test]
fn malformed_image_metadata_is_rejected_consistently_without_raw_mutation(cx: &mut TestAppContext) {
    for (key, value) in [
        ("asset", AttrValue::Null),
        ("src", AttrValue::Null),
        ("alt", AttrValue::Bool(false)),
        ("title", AttrValue::Integer(12)),
        ("width", AttrValue::Integer(0)),
        ("height", AttrValue::String("120".into())),
    ] {
        let raw = changed(&all_null("asset", "sample"), key, Some(value));
        let resolver = Rc::new(PendingResolver::default());
        let (view, image) = fixture(raw.clone(), Some(resolver.clone()));
        let view = cx.new(|_| view);
        view.update(cx, |view, _| {
            sync(view);
            assert!(resolver.requests.borrow().is_empty());
            assert_eq!(view.image_load_state(image), None);
            assert!(view.image_render_source(image).is_none());
            assert_eq!(
                view.image_placeholder_presentation(image).0,
                "invalid image attrs"
            );
            assert_raw_and_a11y(view, image, &raw, None);
            assert_eq!(view.session.borrow().history_depths(), (0, 0));
        });
    }
}

#[gpui::test]
fn same_string_source_kind_switch_never_displays_or_fetches_external_source(
    cx: &mut TestAppContext,
) {
    let resolver = Rc::new(PendingResolver::default());
    let asset = all_null("asset", "same");
    let external = all_null("src", "same");
    let (view, image) = fixture(asset.clone(), Some(resolver.clone()));
    let view = cx.new(|_| view);
    view.update(cx, |view, _| {
        sync(view);
        resolver.finish(0, payload("same", 1, 11));
        assert_eq!(view.image_render_source(image).unwrap().bytes, [11]);
        replace_attrs(view, image, external.clone());
        // Public reads must stop exposing the asset even before the next sync.
        assert_eq!(view.image_load_state(image), None);
        assert!(view.image_render_source(image).is_none());
        assert_eq!(view.image_placeholder_presentation(image).0, "图片");
        sync(view);
        assert_eq!(resolver.requests.borrow().len(), 1);
        assert_eq!(view.image_loads.fresh_state(image, "same"), None);
        assert!(view.image_loads.render_source(image, "same").is_none());
        assert_raw_and_a11y(view, image, &external, None);

        replace_attrs(view, image, asset.clone());
        assert_eq!(view.image_load_state(image), None);
        assert!(view.image_render_source(image).is_none());
        sync(view);
        assert_eq!(resolver.requests.borrow().len(), 2);
        assert_eq!(view.image_load_state(image), Some(ImageLoadState::Loading));
        resolver.finish(1, payload("same", 2, 22));
        assert_eq!(view.image_render_source(image).unwrap().bytes, [22]);
        assert_raw_and_a11y(view, image, &asset, None);
    });
}

#[gpui::test]
fn pending_load_is_discarded_for_external_or_invalid_attrs_even_without_service(
    cx: &mut TestAppContext,
) {
    for raw in [
        all_null("src", "same"),
        changed(
            &all_null("asset", "same"),
            "alt",
            Some(AttrValue::Bool(false)),
        ),
    ] {
        let resolver = Rc::new(PendingResolver::default());
        let (view, image) = fixture(all_null("asset", "same"), Some(resolver.clone()));
        let view = cx.new(|_| view);
        view.update(cx, |view, _| {
            sync(view);
            replace_attrs(view, image, raw.clone());
            view.asset_service = None;
            sync(view);
            resolver.finish(0, payload("same", 1, 11));
            assert_eq!(resolver.requests.borrow().len(), 1);
            assert_eq!(view.image_loads.fresh_state(image, "same"), None);
            assert!(view.image_loads.render_source(image, "same").is_none());
            assert_eq!(view.image_load_state(image), None);
            assert!(view.image_render_source(image).is_none());
            assert_raw_and_a11y(view, image, &raw, None);
        });
    }
}

#[gpui::test]
fn returning_to_same_asset_rejects_old_request_before_and_after_new_result(
    cx: &mut TestAppContext,
) {
    for old_result in [payload("same", 1, 11), Err(AssetError::NotFound)] {
        for old_first in [false, true] {
            let resolver = Rc::new(PendingResolver::default());
            let raw = all_null("asset", "same");
            let (view, image) = fixture(raw.clone(), Some(resolver.clone()));
            let view = cx.new(|_| view);
            view.update(cx, |view, _| {
                sync(view);
                replace_attrs(view, image, all_null("src", "same"));
                sync(view);
                replace_attrs(view, image, raw.clone());
                sync(view);
                assert_eq!(resolver.requests.borrow().len(), 2);
                if old_first {
                    resolver.finish(0, old_result.clone());
                    assert_eq!(view.image_load_state(image), Some(ImageLoadState::Loading));
                    assert!(view.image_render_source(image).is_none());
                }
                resolver.finish(1, payload("same", 2, 22));
                if !old_first {
                    resolver.finish(0, old_result.clone());
                }
                assert_eq!(
                    view.image_load_state(image),
                    Some(ImageLoadState::Resolved {
                        revision: 2,
                        byte_len: 1
                    })
                );
                assert_eq!(view.image_render_source(image).unwrap().bytes, [22]);
                assert_raw_and_a11y(view, image, &raw, None);
            });
        }
    }
}
