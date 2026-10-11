//! Local image widths in structural containers, independent of table admission.
use super::*;

#[derive(Clone, Copy, Debug)]
enum Container {
    Quote,
    List,
    Task,
    Table,
}

fn nested(container: Container) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "b");
    let image = builder
        .insert(
            NodeKind::Image,
            attrs(Some(1600), Some(3)),
            NodeContent::Atomic,
        )
        .unwrap();
    let after = paragraph(&mut builder, "a");
    let content = [before, image, after];
    let parent = match container {
        Container::Quote => builder
            .insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children(content),
            )
            .unwrap(),
        Container::List | Container::Task => {
            let task = matches!(container, Container::Task);
            let item = builder
                .insert(
                    if task {
                        NodeKind::TaskItem
                    } else {
                        NodeKind::ListItem
                    },
                    if task {
                        NodeAttrs::new([("checked".into(), AttrValue::Bool(false))].into()).unwrap()
                    } else {
                        NodeAttrs::empty()
                    },
                    NodeContent::children(content),
                )
                .unwrap();
            builder
                .insert(
                    if task {
                        NodeKind::TaskList
                    } else {
                        NodeKind::BulletList
                    },
                    NodeAttrs::empty(),
                    NodeContent::children([item]),
                )
                .unwrap()
        }
        Container::Table => {
            let cell = builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children(content),
                )
                .unwrap();
            let row = builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children([cell]),
                )
                .unwrap();
            builder
                .insert(
                    NodeKind::Table,
                    NodeAttrs::empty(),
                    NodeContent::children([row]),
                )
                .unwrap()
        }
    };
    let mut children = vec![parent];
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

#[gpui::test]
fn responsive_images_measure_local_quote_list_task_and_table_content(cx: &mut TestAppContext) {
    for container in [
        Container::Quote,
        Container::List,
        Container::Task,
        Container::Table,
    ] {
        let f = nested(container);
        let selection = f.session.borrow().selection();
        let resolver = Resolver::new(Sample::Landscape, false);
        let handle = open(&f, resolver.clone(), 432.0, cx);
        if matches!(container, Container::Table) {
            handle
                .update(cx, |host, _, cx| {
                    host.editor.update(cx, |view, cx| {
                        view.set_measured_table_layout(true);
                        cx.notify();
                    })
                })
                .unwrap();
        }
        cx.background_executor.run_until_parked();
        let wide = geometry(handle, &f, cx);
        assert_loaded(wide, Sample::Landscape, Some(1600.0));
        resize(handle, 172.0, cx);
        let narrow = geometry(handle, &f, cx);
        assert_loaded(narrow, Sample::Landscape, Some(1600.0));
        assert!(
            narrow.image.size.width < wide.image.size.width,
            "{container:?}"
        );
        assert_reflow(wide, narrow);
        resize(handle, 432.0, cx);
        let restored = geometry(handle, &f, cx);
        assert_eq!(wide.image.size, restored.image.size, "{container:?}");
        assert_reflow(narrow, restored);
        assert_eq!(resolver.count(), 1);
        assert_untouched(&f, selection);
    }
}

#[gpui::test]
fn default_fit_policy_ignores_dimension_hints_as_before(cx: &mut TestAppContext) {
    assert_eq!(
        ImageSizingPolicy::default(),
        ImageSizingPolicy::FitContainer
    );
    let f = fixture(attrs(None, None));
    let resolver = Resolver::new(Sample::Landscape, false);
    let handle = open(&f, resolver.clone(), 432.0, cx);
    handle
        .update(cx, |host, _, cx| {
            host.editor.update(cx, |view, cx| {
                view.set_image_sizing_policy(ImageSizingPolicy::default());
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let before = handle
        .update(cx, |host, _, cx| {
            let view = host.editor.read(cx);
            (
                view.block_bounds(f.after).unwrap(),
                view.scroll_handle.max_offset(),
            )
        })
        .unwrap();
    f.session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::SetNodeAttrs {
                node: f.image,
                attrs: attrs(Some(48), Some(200)),
            },
        ))
        .unwrap();
    repaint(handle, cx);
    let after = handle
        .update(cx, |host, _, cx| {
            let view = host.editor.read(cx);
            (
                view.block_bounds(f.after).unwrap(),
                view.scroll_handle.max_offset(),
            )
        })
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(resolver.count(), 1);
}

#[gpui::test]
fn nested_list_inside_quote_consumes_each_image_indent_once(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "b");
    let image = builder
        .insert(
            NodeKind::Image,
            attrs(Some(1600), None),
            NodeContent::Atomic,
        )
        .unwrap();
    let after = paragraph(&mut builder, "a");
    let inner_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([before, image, after]),
        )
        .unwrap();
    let inner = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([inner_item]),
        )
        .unwrap();
    let outer_text = paragraph(&mut builder, "o");
    let outer_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([outer_text, inner]),
        )
        .unwrap();
    let outer = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([outer_item]),
        )
        .unwrap();
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([outer]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([quote]),
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
    let f = Fixture {
        document,
        session,
        image,
        before,
        after,
    };
    let selection = f.session.borrow().selection();
    let handle = open(&f, Resolver::new(Sample::Landscape, false), 432.0, cx);
    cx.background_executor.run_until_parked();
    for width in [432.0, 172.0, 432.0] {
        resize(handle, width, cx);
        let selector = Box::leak(format!("image-content-{image:?}").into_boxed_str());
        let actual = VisualTestContext::from_window(handle.into(), cx)
            .debug_bounds(selector)
            .unwrap();
        // Document16 + quote18 + two list steps48. The image clamps to
        // the remaining local width, independently of inline marker layout.
        near(actual.left(), 82.0);
        near(actual.right(), width - 16.0);
        near(
            actual.size.height,
            (width - 16.0 - 82.0 - 2.0) * 64.0 / 96.0 + 2.0,
        );
    }
    assert_untouched(&f, selection);
}

#[test]
fn image_policy_builder_is_instance_local_and_preserves_the_default() {
    use crate::editor::{EditorHooks, EditorInstance};
    let f = fixture(attrs(None, None));
    let selection = f.session.borrow().selection();
    let default =
        EditorInstance::new(f.document.clone(), selection, EditorHooks::default()).unwrap();
    let responsive = EditorInstance::new(f.document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_image_sizing_policy(ImageSizingPolicy::ResponsiveIntrinsic);
    assert_eq!(
        default.build_view().image_sizing,
        ImageSizingPolicy::FitContainer
    );
    assert_eq!(
        responsive.build_view().image_sizing,
        ImageSizingPolicy::ResponsiveIntrinsic
    );
    assert_eq!(
        default.build_view().image_sizing,
        ImageSizingPolicy::FitContainer
    );
    assert_untouched(&f, selection);
}
