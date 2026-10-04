//! The public host command entry uses the existing composition/render path.

use super::DocumentView;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext};
use xiaomu_core::document::{
    HeadingLevel, InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument,
};
use xiaomu_core::selection::TextPoint;
use xiaomu_runtime::session::{DocumentSelection, EditIntent};

#[gpui::test]
fn host_intents_preserve_composition_guard_and_structural_focus_routing(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
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
    let editor = EditorInstance::new(
        document.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
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

    handle
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
            });
            let epoch = view.epoch.get();
            view.apply_edit_intent(
                EditIntent::TurnInto {
                    kind: NodeKind::Heading(HeadingLevel::new(2).unwrap()),
                },
                window,
                cx,
            );
            assert_eq!(view.epoch.get(), epoch);
            assert_eq!(session.borrow().document().store(), document.store());
            assert_eq!(session.borrow().history_depths(), (0, 0));
            child.update(cx, |child, cx| {
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
            });

            view.apply_edit_intent(EditIntent::SplitBlock, window, cx);
            assert_eq!(view.epoch.get(), epoch + 1);
            assert_eq!(view.children.len(), 2);
            let focus = session
                .borrow()
                .selection()
                .as_single_node()
                .unwrap()
                .focus()
                .node_id();
            assert_ne!(focus, node);
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(focus)
            );
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    cx.simulate_input(handle.into(), "tail");
    let borrowed = session.borrow();
    let focus = borrowed
        .selection()
        .as_single_node()
        .unwrap()
        .focus()
        .node_id();
    assert_eq!(
        borrowed
            .document()
            .node(focus)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .text()
            .as_str(),
        "tail"
    );
    assert_eq!(
        borrowed
            .document()
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .len_bytes(),
        0
    );
}
