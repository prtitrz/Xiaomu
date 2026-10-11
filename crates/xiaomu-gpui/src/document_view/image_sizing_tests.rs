//! Mounted image layout contracts, using real PNG/JPEG decode and painted bounds.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

use gpui::Styled as _;
use gpui::{
    AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement, Pixels, Render,
    TestAppContext, VisualTestContext, Window, WindowHandle, div, px,
};
use xiaomu_core::{
    document::{
        AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::InlinePoint,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink, ResolvedAsset},
    session::{DocumentSelection, DocumentSession},
};

use super::DocumentView;
use crate::{
    block_view::SharedSession,
    image_block::{ImageLoadState, ImageSizingPolicy},
};

#[path = "image_sizing_navigation_tests.rs"]
mod navigation;

#[path = "image_sizing_context_tests.rs"]
mod contexts;

#[derive(Clone, Copy)]
enum Sample {
    Landscape,
    SmallJpeg,
    Tall,
    Wide,
}

impl Sample {
    fn data(self) -> (AssetFormat, &'static [u8], f32, f32) {
        match self {
            Self::Landscape => (
                AssetFormat::Png,
                include_bytes!("../image_block/fixtures/landscape.png"),
                96.0,
                64.0,
            ),
            Self::SmallJpeg => (
                AssetFormat::Jpeg,
                include_bytes!("../image_block/fixtures/small.jpg"),
                80.0,
                48.0,
            ),
            Self::Tall => (
                AssetFormat::Png,
                include_bytes!("../image_block/fixtures/tall.png"),
                80.0,
                800.0,
            ),
            Self::Wide => (
                AssetFormat::Png,
                include_bytes!("../image_block/fixtures/wide.png"),
                1200.0,
                120.0,
            ),
        }
    }

    fn payload(self, source: AssetRef) -> ResolvedAsset {
        let (format, bytes, _, _) = self.data();
        ResolvedAsset::new(source, 7, format, bytes.to_vec())
    }
}

struct Resolver {
    sample: Sample,
    delayed: bool,
    requests: RefCell<Vec<(AssetRef, Rc<dyn AssetSink>)>>,
}

impl Resolver {
    fn new(sample: Sample, delayed: bool) -> Rc<Self> {
        Rc::new(Self {
            sample,
            delayed,
            requests: RefCell::new(Vec::new()),
        })
    }

    fn finish(&self, index: usize) {
        let (source, sink) = self.requests.borrow()[index].clone();
        sink.resolved(Ok(self.sample.payload(source)));
    }

    fn count(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl AssetService for Resolver {
    fn resolve(&self, source: AssetRef, sink: Rc<dyn AssetSink>) {
        self.requests
            .borrow_mut()
            .push((source.clone(), sink.clone()));
        if !self.delayed {
            sink.resolved(Ok(self.sample.payload(source)));
        }
    }
}

fn attrs(width: Option<i64>, height: Option<i64>) -> NodeAttrs {
    NodeAttrs::new(BTreeMap::from([
        ("asset".into(), AttrValue::String("mounted-image".into())),
        ("alt".into(), AttrValue::String("fixture".into())),
        ("title".into(), AttrValue::Null),
        (
            "width".into(),
            width.map_or(AttrValue::Null, AttrValue::Integer),
        ),
        (
            "height".into(),
            height.map_or(AttrValue::Null, AttrValue::Integer),
        ),
    ]))
    .unwrap()
}

struct Fixture {
    document: XiaomuDocument,
    session: SharedSession,
    image: NodeId,
    before: NodeId,
    after: NodeId,
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn fixture(attrs: NodeAttrs) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    // One-character paragraphs stay one line even in the narrowest host.
    let before = paragraph(&mut builder, "b");
    let image = builder
        .insert(NodeKind::Image, attrs, NodeContent::Atomic)
        .unwrap();
    let after = paragraph(&mut builder, "a");
    let mut children = vec![before, image, after];
    // Keep the scroll range positive in every case so its delta is measurable.
    for _ in 0..16 {
        children.push(paragraph(&mut builder, "x"));
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(before)),
        )
        .unwrap(),
    ));
    Fixture {
        document,
        session,
        image,
        before,
        after,
    }
}

struct SizedHost {
    editor: Entity<DocumentView>,
    width: f32,
}

impl Render for SizedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(240.0))
            .child(self.editor.clone())
    }
}

// Deliberately do not drain tasks here: two windows can mount against one cold
// GPUI image-cache entry before either decode-completion callback executes.
fn open(
    fixture: &Fixture,
    resolver: Rc<Resolver>,
    width: f32,
    cx: &mut TestAppContext,
) -> WindowHandle<SizedHost> {
    cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let editor = cx.new(|_| {
                let mut view = DocumentView::new(fixture.session.clone());
                view.set_image_sizing_policy(ImageSizingPolicy::ResponsiveIntrinsic);
                view.set_asset_service(resolver);
                view
            });
            cx.new(|_| SizedHost { editor, width })
        })
        .unwrap()
    })
}

fn repaint(handle: WindowHandle<SizedHost>, cx: &mut TestAppContext) {
    handle
        .update(cx, |host, _, cx| {
            host.editor.update(cx, |_, cx| cx.notify())
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}

fn resize(handle: WindowHandle<SizedHost>, width: f32, cx: &mut TestAppContext) {
    handle
        .update(cx, |host, _, cx| {
            host.width = width;
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
}

#[derive(Clone, Copy)]
struct Geometry {
    image: Bounds<Pixels>,
    after_y: Pixels,
    scroll_height: Pixels,
    available_width: Pixels,
}

fn geometry(handle: WindowHandle<SizedHost>, f: &Fixture, cx: &mut TestAppContext) -> Geometry {
    let selector = Box::leak(format!("image-content-{:?}", f.image).into_boxed_str());
    let image = VisualTestContext::from_window(handle.into(), cx)
        .debug_bounds(selector)
        .expect("actual painted image content");
    let measured = handle
        .update(cx, |host, _, cx| {
            let view = host.editor.read(cx);
            let before = view
                .block_bounds(f.before)
                .expect("painted preceding paragraph");
            assert!(image.left() >= before.left() - px(1.0));
            assert!(
                image.right() <= before.right() + px(1.0),
                "image must fit its local content width"
            );
            Geometry {
                image,
                after_y: view
                    .block_bounds(f.after)
                    .expect("painted following paragraph")
                    .top(),
                scroll_height: view.scroll_handle.max_offset().height,
                available_width: before.size.width,
            }
        })
        .unwrap();
    eprintln!(
        "IMAGE_GEOMETRY node={:?} box={:?} following_y={:?} scroll_max={:?} local_width={:?}",
        f.image, measured.image, measured.after_y, measured.scroll_height, measured.available_width
    );
    measured
}

// Stock GPUI enables Taffy whole-device-pixel rounding. Pure policy tests
// use 0.001px; actual painted layouts allow one physical pixel at scale1.
fn near(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() <= 1.0,
        "actual {actual:?}, expected approximately {expected}px"
    );
}

fn assert_loaded(g: Geometry, sample: Sample, requested: Option<f32>) {
    let (_, _, natural_w, natural_h) = sample.data();
    let outer_w = requested
        .unwrap_or(natural_w + 2.0)
        .min(f32::from(g.available_width));
    near(g.image.size.width, outer_w);
    near(
        g.image.size.height,
        (outer_w - 2.0).max(0.0) * natural_h / natural_w + 2.0,
    );
}

fn assert_reflow(before: Geometry, after: Geometry) {
    let delta = f32::from(after.image.size.height - before.image.size.height);
    near(after.after_y - before.after_y, delta);
    assert!(before.scroll_height > px(0.0) && after.scroll_height > px(0.0));
    near(after.scroll_height - before.scroll_height, delta);
}

fn assert_untouched(f: &Fixture, selection: DocumentSelection) {
    let session = f.session.borrow();
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[gpui::test]
fn responsive_png_and_jpeg_decode_naturally_then_shrink_and_restore(cx: &mut TestAppContext) {
    for sample in [Sample::Landscape, Sample::SmallJpeg] {
        let f = fixture(attrs(None, None));
        let selection = f.session.borrow().selection();
        let resolver = Resolver::new(sample, false);
        let handle = open(&f, resolver.clone(), 432.0, cx);
        cx.background_executor.run_until_parked();
        let natural = geometry(handle, &f, cx);
        assert_loaded(natural, sample, None);
        resize(handle, 72.0, cx);
        let narrow = geometry(handle, &f, cx);
        assert_loaded(narrow, sample, None);
        assert!(narrow.image.size.width < natural.image.size.width);
        assert_reflow(natural, narrow);
        resize(handle, 632.0, cx);
        let restored = geometry(handle, &f, cx);
        assert_loaded(restored, sample, None);
        assert_eq!(
            restored.image.size, natural.image.size,
            "no intrinsic upscaling"
        );
        assert_reflow(narrow, restored);
        assert_eq!(resolver.count(), 1);
        assert_untouched(&f, selection);
    }
}

#[gpui::test]
fn responsive_wide_and_tall_images_change_following_layout_and_scroll_extent(
    cx: &mut TestAppContext,
) {
    for (sample, initial_width, narrow_width, expanded_width) in [
        (Sample::Wide, 432.0, 132.0, 1432.0),
        (Sample::Tall, 432.0, 68.0, 632.0),
    ] {
        // A stale canonical height remains preserved but cannot distort ratio.
        let f = fixture(attrs(None, Some(17)));
        let selection = f.session.borrow().selection();
        let resolver = Resolver::new(sample, false);
        let handle = open(&f, resolver.clone(), initial_width, cx);
        cx.background_executor.run_until_parked();
        let initial = geometry(handle, &f, cx);
        assert_loaded(initial, sample, None);
        if matches!(sample, Sample::Tall) {
            near(initial.image.size.height, 802.0);
            assert!(
                initial.image.size.height > px(320.0),
                "responsive mode has no legacy height cap"
            );
        }
        resize(handle, narrow_width, cx);
        let narrow = geometry(handle, &f, cx);
        assert_loaded(narrow, sample, None);
        assert_reflow(initial, narrow);
        resize(handle, expanded_width, cx);
        let expanded = geometry(handle, &f, cx);
        assert_loaded(expanded, sample, None);
        assert_reflow(narrow, expanded);
        resize(handle, initial_width, cx);
        let restored = geometry(handle, &f, cx);
        assert_eq!(restored.image.size, initial.image.size);
        assert_reflow(expanded, restored);
        assert_eq!(resolver.count(), 1);
        assert_untouched(&f, selection);
    }
}

#[gpui::test]
fn responsive_dimension_edits_reuse_resolved_source_and_undo_reflows(cx: &mut TestAppContext) {
    let f = fixture(attrs(Some(194), Some(999)));
    let selection = f.session.borrow().selection();
    let resolver = Resolver::new(Sample::Landscape, false);
    let handle = open(&f, resolver.clone(), 632.0, cx);
    cx.background_executor.run_until_parked();
    let source = handle
        .update(cx, |host, _, cx| {
            host.editor.read(cx).image_render_source(f.image).unwrap()
        })
        .unwrap();
    let mut previous = geometry(handle, &f, cx);
    assert_loaded(previous, Sample::Landscape, Some(194.0));
    for (width, height) in [(Some(50), Some(1)), (Some(1000), None), (None, Some(9999))] {
        let raw = attrs(width, height);
        f.session
            .borrow_mut()
            .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
                TransactionStep::SetNodeAttrs {
                    node: f.image,
                    attrs: raw.clone(),
                },
            ))
            .unwrap();
        let depths = f.session.borrow().history_depths();
        let revision = f.session.borrow().document().revision();
        repaint(handle, cx);
        let current = geometry(handle, &f, cx);
        assert_loaded(current, Sample::Landscape, width.map(|v| v as f32));
        assert_reflow(previous, current);
        assert_eq!(
            f.session.borrow().document().node(f.image).unwrap().attrs(),
            &raw
        );
        assert_eq!(f.session.borrow().document().revision(), revision);
        assert_eq!(f.session.borrow().selection(), selection);
        assert_eq!(f.session.borrow().history_depths(), depths);
        assert_eq!(resolver.count(), 1);
        handle
            .update(cx, |host, _, cx| {
                assert!(Arc::ptr_eq(
                    &source,
                    &host.editor.read(cx).image_render_source(f.image).unwrap()
                ));
            })
            .unwrap();
        previous = current;
    }
    f.session.borrow_mut().undo().unwrap();
    let depths = f.session.borrow().history_depths();
    repaint(handle, cx);
    let undone = geometry(handle, &f, cx);
    assert_loaded(undone, Sample::Landscape, Some(1000.0));
    assert_reflow(previous, undone);
    assert_eq!(f.session.borrow().history_depths(), depths);
    f.session.borrow_mut().redo().unwrap();
    let depths = f.session.borrow().history_depths();
    repaint(handle, cx);
    let redone = geometry(handle, &f, cx);
    assert_eq!(redone.image.size, previous.image.size);
    assert_reflow(undone, redone);
    assert_eq!(f.session.borrow().history_depths(), depths);
    assert_eq!(resolver.count(), 1);
}

#[gpui::test]
fn responsive_delayed_resolve_keeps_placeholder_then_reflows_at_latest_width(
    cx: &mut TestAppContext,
) {
    let f = fixture(attrs(Some(194), Some(1)));
    let selection = f.session.borrow().selection();
    let resolver = Resolver::new(Sample::Landscape, true);
    let handle = open(&f, resolver.clone(), 432.0, cx);
    cx.background_executor.run_until_parked();
    let loading = geometry(handle, &f, cx);
    near(loading.image.size.height, 96.0);
    handle
        .update(cx, |host, _, cx| {
            assert_eq!(
                host.editor.read(cx).image_load_state(f.image),
                Some(ImageLoadState::Loading)
            );
        })
        .unwrap();
    resize(handle, 112.0, cx);
    let pending_narrow = geometry(handle, &f, cx);
    near(pending_narrow.image.size.height, 96.0);
    assert_reflow(loading, pending_narrow);
    // Host delivery invalidates the view once. Decode completion must itself
    // publish the final layout; no post-decode manual repaint is performed.
    handle
        .update(cx, |host, _, cx| {
            host.editor.update(cx, |_, cx| {
                resolver.finish(0);
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let decoded = geometry(handle, &f, cx);
    assert_loaded(decoded, Sample::Landscape, Some(194.0));
    assert_reflow(pending_narrow, decoded);
    resize(handle, 432.0, cx);
    let expanded = geometry(handle, &f, cx);
    assert_loaded(expanded, Sample::Landscape, Some(194.0));
    assert_reflow(decoded, expanded);
    assert_eq!(resolver.count(), 1);
    assert_untouched(&f, selection);
}

#[gpui::test]
fn responsive_cold_shared_decode_updates_both_windows_without_an_extra_repaint(
    cx: &mut TestAppContext,
) {
    let f = fixture(attrs(None, None));
    let selection = f.session.borrow().selection();
    let resolver = Resolver::new(Sample::Landscape, false);
    let wide = open(&f, resolver.clone(), 432.0, cx);
    let narrow = open(&f, resolver.clone(), 72.0, cx);
    cx.background_executor.run_until_parked();
    assert_loaded(geometry(wide, &f, cx), Sample::Landscape, None);
    let narrow_before = geometry(narrow, &f, cx);
    assert_loaded(narrow_before, Sample::Landscape, None);
    assert_eq!(
        resolver.count(),
        2,
        "per-view resolve, shared GPUI decoded bytes"
    );
    resize(wide, 82.0, cx);
    assert_loaded(geometry(wide, &f, cx), Sample::Landscape, None);
    let narrow_after = geometry(narrow, &f, cx);
    assert_eq!(
        narrow_after.image, narrow_before.image,
        "pane widths are independent"
    );
    assert_eq!(narrow_after.after_y, narrow_before.after_y);
    assert_eq!(resolver.count(), 2);
    assert_untouched(&f, selection);
}

#[gpui::test]
fn responsive_image_click_and_explicit_node_selection_do_not_change_box_geometry(
    cx: &mut TestAppContext,
) {
    let f = fixture(attrs(None, None));
    let resolver = Resolver::new(Sample::Landscape, false);
    let handle = open(&f, resolver.clone(), 432.0, cx);
    cx.background_executor.run_until_parked();
    let initial = geometry(handle, &f, cx);
    assert_loaded(initial, Sample::Landscape, None);
    VisualTestContext::from_window(handle.into(), cx)
        .simulate_click(initial.image.center(), Default::default());
    assert_eq!(
        f.session.borrow().selection().as_atomic_node(),
        Some(f.image)
    );
    let clicked = geometry(handle, &f, cx);
    assert_eq!(clicked.image.size, initial.image.size);
    assert_reflow(initial, clicked);
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                view.select_node(f.image, window, cx).unwrap();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(
        f.session.borrow().selection().as_node_selection(),
        Some(f.image)
    );
    let selected = geometry(handle, &f, cx);
    assert_eq!(
        selected.image.size, initial.image.size,
        "selection border is layout-stable"
    );
    assert_reflow(clicked, selected);
    let selection = f.session.borrow().selection();
    resize(handle, 72.0, cx);
    let narrow = geometry(handle, &f, cx);
    assert_loaded(narrow, Sample::Landscape, None);
    assert_reflow(selected, narrow);
    resize(handle, 432.0, cx);
    assert_eq!(geometry(handle, &f, cx).image.size, initial.image.size);
    assert_eq!(resolver.count(), 1);
    assert_untouched(&f, selection);
}

#[gpui::test]
fn responsive_resolve_and_decode_failures_remain_stable_placeholders(cx: &mut TestAppContext) {
    for corrupt_bytes in [false, true] {
        let f = fixture(attrs(None, None));
        let selection = f.session.borrow().selection();
        let resolver = Resolver::new(Sample::Landscape, true);
        let handle = open(&f, resolver.clone(), 432.0, cx);
        cx.background_executor.run_until_parked();
        let loading = geometry(handle, &f, cx);
        let (source, sink) = resolver.requests.borrow()[0].clone();
        sink.resolved(if corrupt_bytes {
            Ok(ResolvedAsset::new(
                source,
                7,
                AssetFormat::Png,
                vec![1, 2, 3],
            ))
        } else {
            Err(AssetError::NotFound)
        });
        repaint(handle, cx);
        let failed = geometry(handle, &f, cx);
        near(failed.image.size.height, 96.0);
        assert_reflow(loading, failed);
        resize(handle, 72.0, cx);
        let narrow = geometry(handle, &f, cx);
        near(narrow.image.size.height, 96.0);
        assert_reflow(failed, narrow);
        handle
            .update(cx, |host, _, cx| {
                let view = host.editor.read(cx);
                if corrupt_bytes {
                    assert_eq!(
                        view.image_load_state(f.image),
                        Some(ImageLoadState::Resolved {
                            revision: 7,
                            byte_len: 3,
                        })
                    );
                } else {
                    assert_eq!(
                        view.image_load_state(f.image),
                        Some(ImageLoadState::Failed(AssetError::NotFound))
                    );
                    assert!(view.image_render_source(f.image).is_none());
                }
            })
            .unwrap();
        assert_eq!(resolver.count(), 1);
        assert_untouched(&f, selection);
    }
}

#[gpui::test]
fn responsive_session_replacement_rejects_old_sink_with_same_node_and_source(
    cx: &mut TestAppContext,
) {
    let old = fixture(attrs(None, None));
    let old_selection = old.session.borrow().selection();
    let resolver = Resolver::new(Sample::Landscape, true);
    let handle = open(&old, resolver.clone(), 432.0, cx);
    cx.background_executor.run_until_parked();
    assert_eq!(resolver.count(), 1);
    let fresh = fixture(attrs(None, None));
    assert_eq!(
        old.image, fresh.image,
        "fixture deliberately reuses NodeId and source"
    );
    let fresh_selection = fresh.session.borrow().selection();
    handle
        .update(cx, |host, _, cx| {
            host.editor.update(cx, |view, cx| {
                view.session = fresh.session.clone();
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(
        resolver.count(),
        2,
        "new session must issue its own request"
    );
    let (source, stale_sink) = resolver.requests.borrow()[0].clone();
    stale_sink.resolved(Ok(Sample::Tall.payload(source)));
    repaint(handle, cx);
    near(geometry(handle, &fresh, cx).image.size.height, 96.0);
    handle
        .update(cx, |host, _, cx| {
            assert_eq!(
                host.editor.read(cx).image_load_state(fresh.image),
                Some(ImageLoadState::Loading)
            );
        })
        .unwrap();
    resolver.finish(1);
    repaint(handle, cx);
    let current = geometry(handle, &fresh, cx);
    assert_loaded(current, Sample::Landscape, None);
    // A repeated late completion cannot overwrite the already-decoded source.
    let (source, stale_sink) = resolver.requests.borrow()[0].clone();
    stale_sink.resolved(Ok(Sample::Tall.payload(source)));
    repaint(handle, cx);
    assert_eq!(geometry(handle, &fresh, cx).image.size, current.image.size);
    assert_eq!(resolver.count(), 2);
    assert_untouched(&old, old_selection);
    assert_untouched(&fresh, fresh_selection);
}
