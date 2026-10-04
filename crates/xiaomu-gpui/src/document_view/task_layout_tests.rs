//! Painted layout contracts for task content and mixed nested list families.

use super::{DocumentView, markers::marker_for_block};
use crate::{
    code_presentation::CodeBlockPresentation,
    editor::{EditorHooks, EditorInstance},
};
use gpui::{AppContext as _, Bounds, Pixels, TestAppContext, VisualTestContext, px};
use xiaomu_core::{
    document::{
        AttrValue, ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId,
        NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::TextPoint,
};
use xiaomu_runtime::session::DocumentSelection;

fn block(builder: &mut NodeStoreBuilder, kind: NodeKind, text: &str) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}
fn container(
    builder: &mut NodeStoreBuilder,
    kind: NodeKind,
    children: impl IntoIterator<Item = NodeId>,
) -> NodeId {
    builder
        .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}
fn bounds(visual: &mut VisualTestContext, prefix: &str, node: NodeId) -> Bounds<Pixels> {
    let selector = Box::leak(format!("{prefix}-{node:?}").into_boxed_str());
    visual.debug_bounds(selector).expect("painted task element")
}

#[gpui::test]
fn task_content_column_contains_code_image_tail_and_mixed_nested_lists(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let head = block(&mut b, NodeKind::Paragraph, "head");
    let code = block(&mut b, NodeKind::CodeBlock, "let x = 1;");
    let image = b
        .insert(
            NodeKind::Image,
            ImageAttrs::new(
                ImageSource::AssetRef("fixture-image".into()),
                "image".into(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let normal_nested = block(&mut b, NodeKind::Paragraph, "ordinary nested");
    let normal_item = container(&mut b, NodeKind::ListItem, [normal_nested]);
    let normal_list = container(&mut b, NodeKind::BulletList, [normal_item]);
    let task_nested = block(&mut b, NodeKind::Paragraph, "task nested");
    let nested_item = container(&mut b, NodeKind::TaskItem, [task_nested]);
    let nested_list = container(&mut b, NodeKind::TaskList, [nested_item]);
    let tail = block(&mut b, NodeKind::Paragraph, "tail");
    let checked_item = b
        .insert(
            NodeKind::TaskItem,
            NodeAttrs::new([("checked".into(), AttrValue::Bool(true))].into()).unwrap(),
            NodeContent::children([head, code, image, normal_list, nested_list, tail]),
        )
        .unwrap();
    let task_list = b
        .insert(
            NodeKind::TaskList,
            NodeAttrs::new([("host-list".into(), AttrValue::Null)].into()).unwrap(),
            NodeContent::children([checked_item]),
        )
        .unwrap();
    let normal_head = block(&mut b, NodeKind::Paragraph, "outer ordinary");
    let task_in_normal = block(&mut b, NodeKind::Paragraph, "task in ordinary");
    let task_in_normal_item = container(&mut b, NodeKind::TaskItem, [task_in_normal]);
    let task_in_normal_list = container(&mut b, NodeKind::TaskList, [task_in_normal_item]);
    let outer_item = container(
        &mut b,
        NodeKind::ListItem,
        [normal_head, task_in_normal_list],
    );
    let outer_list = container(&mut b, NodeKind::BulletList, [outer_item]);
    let root = container(&mut b, NodeKind::Document, [task_list, outer_list]);
    let document = XiaomuDocument::new(root, b.finish()).unwrap();
    assert!(marker_for_block(&document, head, None).is_none());
    assert!(marker_for_block(&document, task_nested, None).is_none());
    assert_eq!(
        marker_for_block(&document, normal_nested, None)
            .unwrap()
            .depth,
        2
    );
    let editor = EditorInstance::new(
        document.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(head)),
        EditorHooks::default(),
    )
    .unwrap()
    .with_code_block_presentation(CodeBlockPresentation::default());
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    let checkbox = bounds(&mut visual, "task-checkbox", checked_item);
    let content_left = checkbox.left() + px(24.0);
    for child in [head, code, image, normal_list, nested_list, tail] {
        assert_eq!(
            bounds(&mut visual, "task-content", child).left(),
            content_left
        );
    }
    assert_eq!(
        bounds(&mut visual, "task-checkbox", nested_item).left(),
        content_left
    );
    handle
        .update(cx, |view, _, cx| {
            assert_eq!(view.block_bounds(head).unwrap().left(), content_left);
            assert_eq!(view.block_bounds(tail).unwrap().left(), content_left);
            assert_eq!(
                view.block_bounds(normal_nested).unwrap().left(),
                content_left + px(24.0)
            );
            assert_eq!(
                view.block_bounds(task_nested).unwrap().left(),
                content_left + px(24.0)
            );
            // Only code's own internal padding/border offsets its editable text.
            assert_eq!(
                view.block_bounds(code).unwrap().left(),
                content_left
                    + px(CodeBlockPresentation::PADDING_X + CodeBlockPresentation::BORDER_WIDTH)
            );
            assert_eq!(
                view.block_bounds(task_in_normal).unwrap().left(),
                view.block_bounds(normal_head).unwrap().left() + px(24.0)
            );
            let child = &view.children.iter().find(|(id, _)| *id == head).unwrap().1;
            assert_eq!(child.read(cx).display_content().0, "head");
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn image_first_task_still_has_a_checkbox_and_one_content_column(cx: &mut TestAppContext) {
    let mut b = NodeStoreBuilder::new();
    let image = b
        .insert(
            NodeKind::Image,
            ImageAttrs::new(
                ImageSource::AssetRef("fixture-image".into()),
                "image".into(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap(),
            NodeContent::Atomic,
        )
        .unwrap();
    let tail = block(&mut b, NodeKind::Paragraph, "tail");
    let item = container(&mut b, NodeKind::TaskItem, [image, tail]);
    let list = container(&mut b, NodeKind::TaskList, [item]);
    let root = container(&mut b, NodeKind::Document, [list]);
    let document = XiaomuDocument::new(root, b.finish()).unwrap();
    let editor = EditorInstance::new(
        document.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(tail)),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    cx.background_executor.run_until_parked();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    let checkbox = bounds(&mut visual, "task-checkbox", item);
    let image_bounds = bounds(&mut visual, "task-content", image);
    let tail_bounds = bounds(&mut visual, "task-content", tail);
    assert_eq!(image_bounds.left(), checkbox.left() + px(24.0));
    assert_eq!(tail_bounds.left(), image_bounds.left());
    assert!(tail_bounds.top() >= image_bounds.bottom());
    assert_eq!(session.borrow().document().store(), document.store());
}
