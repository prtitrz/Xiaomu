//! Additional mounted first-line inset regression contracts.
use super::*;

struct ObservedBaseInset(Rc<RefCell<Vec<(XiaomuDocument, Node, f32)>>>);
impl FirstLineInsetProvider for ObservedBaseInset {
    fn first_line_inset(&self, document: &XiaomuDocument, node: &Node, base_font_px: f32) -> f32 {
        assert_eq!(document.parent_of(node.id()), Some(document.root()));
        self.0
            .borrow_mut()
            .push((document.clone(), node.clone(), base_font_px));
        2.0 * base_font_px
    }
}

#[gpui::test]
fn provider_receives_canonical_snapshot_and_block_base_not_largest_run_or_preedit(
    cx: &mut TestAppContext,
) {
    use xiaomu_core::document::{Mark, StringAttribute, TextStyleAttributes, TextStyleMark};
    let mut builder = NodeStoreBuilder::new();
    let large = Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value("36px".into())),
    ));
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("large ", MarkSet::new([large]).unwrap()).unwrap(),
                    TextRun::new("small", MarkSet::empty()).unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mounted = open(cx, document.clone(), node, None);
    let seen = Rc::new(RefCell::new(Vec::new()));
    mounted
        .handle
        .update(cx, |host, window, cx| {
            let capability = Rc::new(
                TextSizeCapability::new(
                    window.text_system().clone(),
                    Rc::new(Style {
                        base: 18.,
                        family: ".SystemUIFont",
                    }),
                )
                .with_first_line_inset_provider(Rc::new(ObservedBaseInset(seen.clone()))),
            );
            capability
                .validate_document(&document, &InlineAtomRendererRegistry::default())
                .unwrap();
            *mounted.policy.borrow_mut() = capability.clone();
            host.view.update(cx, |view, cx| {
                view.attach_text_size_capability(Some(capability));
                cx.notify();
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    state(&mounted, 36., cx);
    assert!(!seen.borrow().is_empty());
    for (observed, observed_node, base) in seen.borrow().iter() {
        assert_eq!(observed.store(), document.store());
        assert_eq!(observed_node, document.node(node).unwrap());
        assert_eq!(*base, 18.);
    }
    seen.borrow_mut().clear();
    mounted
        .handle
        .update(cx, |host, window, cx| {
            host.view.update(cx, |view, cx| {
                view.children[0].1.clone().update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "PREEDIT", Some(2..2), window, cx);
                    assert!(child.is_composing());
                });
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert!(!seen.borrow().is_empty());
    for (observed, observed_node, base) in seen.borrow().iter() {
        assert_eq!(observed.store(), document.store());
        assert_eq!(observed_node, document.node(node).unwrap());
        assert_eq!(*base, 18.);
    }
    mounted
        .handle
        .update(cx, |host, window, cx| {
            host.view.update(cx, |view, cx| {
                view.children[0].1.clone().update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "", None, window, cx);
                    child.unmark_text(window, cx);
                });
            })
        })
        .unwrap();
    assert_clean(
        &mounted,
        &document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
    );
}

#[test]
fn inset_layout_cache_keeps_exact_fractional_width_inputs() {
    let (_, node) = fixture("cache");
    // The mounted host widths 260.25 and 260.4 produce the same child text box.
    // Exercise the actual production cache-key builder separately to establish
    // that a native fractional-width input cannot reuse a different width.
    let first = LayoutCacheKey::new(node, 0, 229.25)
        .with_text_sizes(true, px(229.25))
        .with_alignment(Some(BlockAlignment::Left), px(229.25));
    let next = LayoutCacheKey::new(node, 0, 229.4)
        .with_text_sizes(true, px(229.4))
        .with_alignment(Some(BlockAlignment::Left), px(229.4));
    assert_ne!(first, next);
    assert_eq!(
        LayoutCacheKey::new(node, 0, 229.25),
        LayoutCacheKey::new(node, 0, 229.4),
        "non-opt-in legacy cache quantization remains unchanged"
    );
}

#[gpui::test]
fn mounted_fractional_width_diagnostic_records_requested_and_actual_cache_inputs(
    cx: &mut TestAppContext,
) {
    let (document, node) = fixture("fractional width diagnostic");
    let mounted = open(cx, document, node, Some(36.));
    let measured = |cx: &mut TestAppContext| {
        mounted
            .handle
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
                    child.cache_key,
                    child.last_layout.as_ref().unwrap().shaping_width().unwrap(),
                )
            })
            .unwrap()
    };
    let first = measured(cx);
    // Only change the host box; provider, font, document, and capability stay fixed.
    mounted
        .handle
        .update(cx, |host, _, cx| {
            host.width = 260.4;
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let second = measured(cx);
    eprintln!(
        "first-line inset width diagnostic: requested={:?} actual={:?} key={:?} shaping={:?}; requested={:?} actual={:?} key={:?} shaping={:?}",
        first.0, first.1, first.2, first.3, second.0, second.1, second.2, second.3
    );
    assert_eq!(first.3, px(228.25), "logical request width is retained");
    assert_eq!(second.3, px(228.4), "new logical request width is retained");
    assert!(
        first.2.is_some() && second.2.is_some(),
        "measured rows remain cacheable"
    );
    assert_eq!(first.0, 260.25);
    assert_eq!(second.0, 260.4);
    assert_eq!(
        first.1,
        px(228.5),
        "pinned TestPlatform mounted content width"
    );
    assert_eq!(
        second.1,
        px(228.5),
        "distinct requested host widths produce the same rounded allocated box"
    );
    assert_ne!(
        first.2, second.2,
        "distinct logical wrap widths must not share a partition key"
    );
    for (_, _, key, shaping) in [first, second] {
        let key = key.unwrap();
        assert_eq!(
            key,
            key.with_alignment(Some(BlockAlignment::Left), shaping)
                .with_text_sizes(true, shaping),
            "stored exact width bits must remain the request shaping inputs"
        );
    }
}
