//! Clicking an atomic block must acquire native action focus independently of
//! canonical selection, including after a cold host reopen or focus loss.
use gpui::{AppContext as _, Modifiers, Point, TestAppContext, VisualTestContext, px};
use xiaomu_core::{
    document::{
        ImageAttrs, ImageSource, InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder,
        XiaomuDocument,
    },
    selection::{CursorAffinity, TextPoint},
};
use xiaomu_gpui::{
    document_view::DocumentView,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
};
use xiaomu_runtime::session::DocumentSelection;

fn exercise(cx: &mut TestAppContext, image: bool, key: &str) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap();
    let (kind, attrs) = if image {
        (
            NodeKind::Image,
            ImageAttrs::new(
                ImageSource::AssetRef("synthetic-image".into()),
                "image".into(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap(),
        )
    } else {
        (NodeKind::HorizontalRule, NodeAttrs::empty())
    };
    let atom = builder.insert(kind, attrs, NodeContent::Atomic).unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, atom]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let offset = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(0)
        .unwrap();
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(TextPoint::new(paragraph, offset, CursorAffinity::Before)),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |_: &mut DocumentView, window, _| {
            window.activate_window();
            window.blur();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let mut view = VisualTestContext::from_window(window.into(), cx);
    let point = Point::new(px(48.), px(if image { 100. } else { 57. }));
    view.simulate_click(point, Modifiers::default());
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().selection().as_atomic_node(), Some(atom));
    assert!(
        window
            .update(cx, |_, window, cx| window.focused(cx).is_some())
            .unwrap(),
        "cold atomic click must acquire keyboard focus"
    );
    // Same canonical selection now returns NoChange, but another click must
    // reclaim focus after an unrelated control or host replacement took it.
    window.update(cx, |_, window, _| window.blur()).unwrap();
    view.simulate_click(point, Modifiers::default());
    cx.background_executor.run_until_parked();
    assert!(
        window
            .update(cx, |_, window, cx| window.focused(cx).is_some())
            .unwrap(),
        "NoChange atomic click must reacquire keyboard focus"
    );
    cx.simulate_keystrokes(window.into(), key);
    cx.background_executor.run_until_parked();
    assert!(session.borrow().document().node(atom).is_none());
    cx.simulate_keystrokes(window.into(), "ctrl-z");
    cx.background_executor.run_until_parked();
    assert!(session.borrow().document().node(atom).is_some());
    assert_eq!(session.borrow().selection().as_atomic_node(), Some(atom));
}

#[gpui::test]
fn cold_image_click_and_reclick_focus_delete_and_undo(cx: &mut TestAppContext) {
    exercise(cx, true, "delete");
}
#[gpui::test]
fn cold_rule_click_and_reclick_focus_backspace_and_undo(cx: &mut TestAppContext) {
    exercise(cx, false, "backspace");
}
