//! Real mounted shape-input/cache provenance, not a stale-pixel claim.
//! Request-layout widths can differ while prepaint outer bounds coincide.
use super::{DocumentView, cache_key::LayoutCacheKey};
use crate::{
    block_alignment::{BlockAlignment, BlockAlignmentProvider},
    editor::{EditorHooks, EditorInstance},
    font_size::FontSizeContext,
    inline_atom::InlineAtomRendererRegistry,
    text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider},
};
use gpui::{
    AppContext as _, Context, Entity, IntoElement, Pixels, Render, Styled, TestAppContext, Window,
    WindowHandle, div, font, prelude::ParentElement, px,
};
use std::rc::Rc;
use xiaomu_core::{
    document::{
        InlineContent, Mark, MarkSet, Node, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes, TextStyleMark,
        XiaomuDocument,
    },
    selection::InlinePoint,
};
use xiaomu_runtime::session::DocumentSelection;

struct Style;
impl TextSizeStyleProvider for Style {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(18., 18., 18.).unwrap(),
            1.4,
        )
    }
}
struct Align;
impl BlockAlignmentProvider for Align {
    fn alignment(&self, _: &Node) -> BlockAlignment {
        BlockAlignment::Center
    }
}
struct Host {
    view: Entity<DocumentView>,
    width: f32,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(self.width)).h(px(600.)).child(self.view.clone())
    }
}
fn fixture(mixed: bool) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let mark = Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value("36px".into())),
    ));
    let inline = InlineContent::new([
        TextRun::new("small prefix ", MarkSet::empty()).unwrap(),
        TextRun::new(
            "larger suffix and more words to wrap across rows",
            if mixed {
                MarkSet::new([mark]).unwrap()
            } else {
                MarkSet::empty()
            },
        )
        .unwrap(),
    ])
    .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
fn measure(
    handle: WindowHandle<Host>,
    node: NodeId,
    cx: &mut TestAppContext,
) -> (f32, Pixels, Pixels, LayoutCacheKey) {
    handle
        .update(cx, |host, _, cx| {
            let view = host.view.read(cx);
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == node)
                .unwrap()
                .1
                .read(cx);
            (
                host.width,
                child.last_bounds.unwrap().size.width,
                child
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .shaping_width()
                    .expect("definite shaping width"),
                child.cache_key.expect("measured request remains cacheable"),
            )
        })
        .unwrap()
}
fn assert_provenance(cx: &mut TestAppContext, sized: bool, mixed: bool, requested_width: f32) {
    let (document, node) = fixture(mixed);
    let before = document.clone();
    let mut editor = None;
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let mut instance = EditorInstance::new(
                document,
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
                EditorHooks::default(),
            )
            .unwrap();
            if sized {
                // Deliberately no FirstLineInsetProvider: this checks the generic path.
                let capability = Rc::new(TextSizeCapability::new(
                    window.text_system().clone(),
                    Rc::new(Style),
                ));
                capability
                    .validate_document(
                        instance.session().borrow().document(),
                        &InlineAtomRendererRegistry::default(),
                    )
                    .unwrap();
                instance = instance.with_text_size_capability(capability);
            } else {
                // Deliberately no TextSizeCapability: only optional block alignment.
                instance = instance.with_block_alignment_provider(Rc::new(Align));
            }
            let view = cx.new(|_| instance.build_view());
            editor = Some(instance);
            cx.new(|_| Host {
                view,
                width: 260.25,
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.background_executor.run_until_parked();
    let first = measure(handle, node, cx);
    handle
        .update(cx, |host, _, cx| {
            host.width = requested_width;
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let second = measure(handle, node, cx);
    // Compare with a fresh production shape at the same second width. This
    // invalidates only the real paragraph cache, never supplies fake geometry.
    handle
        .update(cx, |host, _, cx| {
            host.view.update(cx, |view, cx| {
                let child = view
                    .children
                    .iter()
                    .find(|(id, _)| *id == node)
                    .unwrap()
                    .1
                    .clone();
                child.update(cx, |child, cx| {
                    child.cache_key = None;
                    cx.notify();
                });
                cx.notify();
            });
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let fresh = measure(handle, node, cx);
    eprintln!(
        "fresh production shape sized={sized} mixed={mixed}: requested={} outer={:?} shaping={:?} key={:?}",
        fresh.0, fresh.1, fresh.2, fresh.3
    );
    eprintln!(
        "layout-cache provenance sized={sized} mixed={mixed}: requested={} outer={:?} shaping={:?} key={:?}; requested={} outer={:?} shaping={:?} key={:?}",
        first.0, first.1, first.2, first.3, second.0, second.1, second.2, second.3
    );
    assert_eq!(
        second, fresh,
        "a warmed layout must have fresh-shape input provenance at the same requested width"
    );
    assert_eq!(first.0, 260.25);
    assert_eq!(second.0, requested_width);
    assert_eq!(
        first.1, second.1,
        "fixture isolates coincident prepaint bounds"
    );
    assert_eq!(first.2, px(228.25));
    assert_eq!(second.2, px(requested_width - 32.));
    assert_ne!(
        first.3, second.3,
        "distinct request shaping widths must have distinct exact cache keys"
    );
    let alignment = if sized {
        BlockAlignment::Left
    } else {
        BlockAlignment::Center
    };
    for (_, _, shaping, key) in [first, second] {
        assert_eq!(
            key,
            key.with_alignment(Some(alignment), shaping)
                .with_text_sizes(sized, shaping),
            "stored exact bits describe request shaping inputs, not coincident prepaint outer bounds"
        );
    }
    handle
        .update(cx, |host, _, cx| {
            host.view.update(cx, |_, cx| cx.notify());
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(
        measure(handle, node, cx),
        second,
        "unchanged redraw preserves provenance"
    );
    let editor = editor.unwrap();
    let session = editor.session().borrow();
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.history_depths(), (0, 0));
    assert_eq!(
        session.selection(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node))
    );
}
#[gpui::test]
fn uniform_size_without_inset_preserves_request_layout_cache_provenance(cx: &mut TestAppContext) {
    assert_provenance(cx, true, false, 260.4);
}
#[gpui::test]
fn mixed_size_without_inset_preserves_request_layout_cache_provenance(cx: &mut TestAppContext) {
    assert_provenance(cx, true, true, 260.4);
}
#[gpui::test]
fn alignment_only_without_size_capability_preserves_request_layout_cache_provenance(
    cx: &mut TestAppContext,
) {
    assert_provenance(cx, false, false, 260.4);
}

#[gpui::test]
fn requesting_previous_rounded_outer_width_cannot_reuse_wrong_shaping_provenance(
    cx: &mut TestAppContext,
) {
    // First request shapes at228.25 but paints into228.5. A subsequent actual
    // request for228.5 must not hit a cache mislabeled from those old bounds.
    for (sized, mixed) in [(true, false), (true, true), (false, false)] {
        assert_provenance(cx, sized, mixed, 260.5);
    }
}
