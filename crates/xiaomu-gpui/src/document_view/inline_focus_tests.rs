//! Real GPUI pointer dispatch must reclaim input even at an unchanged caret.
use super::*;
use crate::editor::{EditorHooks, EditorInstance};
use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle, div, point, px};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
};
use xiaomu_runtime::session::DocumentSelection;

struct Panes {
    editors: [Entity<DocumentView>; 2],
}
impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().flex().size_full().children(
            self.editors
                .iter()
                .map(|editor| div().flex_1().min_w_0().h_full().child(editor.clone())),
        )
    }
}
fn open(text: &str, read_only: bool, cx: &mut TestAppContext) -> WindowHandle<Panes> {
    let mut builder = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    };
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
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            window.activate_window();
            let editors = std::array::from_fn(|_| {
                let editor = EditorInstance::new_with_policy(
                    document.clone(),
                    DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
                    EditorHooks::default(),
                    Box::new(InputPolicy { read_only }),
                )
                .unwrap();
                cx.new(|_| editor.build_view())
            });
            cx.new(|_| Panes { editors })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
}
// Click the editor padding, which hit-tests to the same inline start but
// would let the scroll surface take default focus without explicit routing.
fn click_start(handle: WindowHandle<Panes>, pane: usize, cx: &mut TestAppContext) {
    let position = handle
        .update(cx, |panes, _, cx| {
            let view = panes.editors[pane].read(cx);
            let registry = view.registry.borrow();
            let bounds = registry[0].1;
            point(bounds.left() - px(8.0), bounds.top() + px(14.0))
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_click(position, Default::default());
    cx.background_executor.run_until_parked();
}
fn assert_owner(handle: WindowHandle<Panes>, owner: usize, cx: &mut TestAppContext) {
    handle
        .update(cx, |panes, window, cx| {
            for (index, entity) in panes.editors.iter().enumerate() {
                let view = entity.read(cx);
                assert_eq!(
                    view.children[0]
                        .1
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window),
                    index == owner
                );
            }
        })
        .unwrap();
}

#[gpui::test]
fn inline_unchanged_pointer_reclaims_empty_and_existing_caret_across_editors(
    cx: &mut TestAppContext,
) {
    for text in ["", "existing"] {
        let handle = open(text, false, cx);
        for pane in [0, 0, 1, 0, 1] {
            let before = handle
                .update(cx, |panes, _, cx| {
                    let view = panes.editors[pane].read(cx);
                    let session = view.session.borrow();
                    (
                        session.selection(),
                        session.document().revision(),
                        session.history_depths(),
                        view.scroll_handle.offset(),
                    )
                })
                .unwrap();
            click_start(handle, pane, cx);
            assert_owner(handle, pane, cx);
            handle
                .update(cx, |panes, _, cx| {
                    let view = panes.editors[pane].read(cx);
                    let session = view.session.borrow();
                    assert_eq!(
                        (
                            session.selection(),
                            session.document().revision(),
                            session.history_depths(),
                            view.scroll_handle.offset()
                        ),
                        before
                    );
                })
                .unwrap();
        }
        cx.simulate_input(handle.into(), "X");
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |panes, _, cx| {
                for (index, entity) in panes.editors.iter().enumerate() {
                    let session = entity.read(cx).session.borrow();
                    assert_eq!(
                        session.history_depths(),
                        if index == 1 { (1, 0) } else { (0, 0) }
                    );
                }
                let view = panes.editors[1].read(cx);
                let mut session = view.session.borrow_mut();
                session.undo().unwrap();
                let node = view.children[0].0;
                assert_eq!(
                    session
                        .document()
                        .node(node)
                        .unwrap()
                        .content()
                        .as_inline()
                        .unwrap()
                        .runs()
                        .iter()
                        .map(|run| run.text().as_str())
                        .collect::<String>(),
                    text
                );
                assert_eq!(
                    session.selection(),
                    DocumentSelection::collapsed(InlinePoint::at_start_of(node))
                );
            })
            .unwrap();
    }
}

#[gpui::test]
fn inline_rejected_selection_does_not_steal_native_focus(cx: &mut TestAppContext) {
    let handle = open("", false, cx);
    click_start(handle, 1, cx);
    assert_owner(handle, 1, cx);
    handle
        .update(cx, |panes, window, cx| {
            panes.editors[0].update(cx, |view, cx| {
                let invalid = InlinePoint::at_start_of(view.session.borrow().document().root());
                let before = view.session.borrow().selection();
                let default_prevented = window.default_prevented();
                assert!(!view.set_inline_selection(invalid, invalid, window, cx));
                assert_eq!(window.default_prevented(), default_prevented);
                assert_eq!(view.session.borrow().selection(), before);
                assert_eq!(view.session.borrow().history_depths(), (0, 0));
            });
        })
        .unwrap();
    assert_owner(handle, 1, cx);
}

#[gpui::test]
fn inline_unchanged_shift_pointer_preserves_range_and_undo(cx: &mut TestAppContext) {
    let handle = open("existing", false, cx);
    let before = handle
        .update(cx, |panes, _, cx| {
            let view = panes.editors[0].read(cx);
            let mut session = view.session.borrow_mut();
            session
                .apply_intent(&EditIntent::InsertText { text: "X".into() })
                .unwrap();
            let node = view.children[0].0;
            let start = InlinePoint::at_start_of(node);
            let end = InlinePoint::new(
                node,
                session
                    .document()
                    .node(node)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(2)
                    .unwrap(),
                0,
                xiaomu_core::selection::CursorAffinity::Before,
            );
            session.set_inline_selection(end, start).unwrap();
            (
                session.selection(),
                session.document().revision(),
                session.history_depths(),
            )
        })
        .unwrap();
    click_start(handle, 1, cx);
    let position = handle
        .update(cx, |panes, _, cx| {
            let bounds = panes.editors[0].read(cx).registry.borrow()[0].1;
            point(bounds.left() - px(8.0), bounds.top() + px(14.0))
        })
        .unwrap();
    VisualTestContext::from_window(handle.into(), cx).simulate_click(
        position,
        gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
    );
    cx.background_executor.run_until_parked();
    assert_owner(handle, 0, cx);
    handle
        .update(cx, |panes, _, cx| {
            let view = panes.editors[0].read(cx);
            let mut session = view.session.borrow_mut();
            assert_eq!(
                (
                    session.selection(),
                    session.document().revision(),
                    session.history_depths()
                ),
                before
            );
            session.undo().unwrap();
            assert_eq!(
                session
                    .document()
                    .node(view.children[0].0)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect::<String>(),
                "existing"
            );
        })
        .unwrap();
}

struct InputPolicy {
    read_only: bool,
}
impl xiaomu_runtime::session::SessionPolicy for InputPolicy {
    fn prepare_intent(
        &self,
        _: xiaomu_runtime::session::SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<xiaomu_runtime::session::IntentDisposition, xiaomu_runtime::session::PolicyError>
    {
        if self.read_only {
            Err(xiaomu_runtime::session::PolicyError::new("read only"))
        } else {
            Ok(xiaomu_runtime::session::IntentDisposition::Continue)
        }
    }
}
#[gpui::test]
fn inline_readonly_pointer_retains_selection_focus_without_enabling_edits(cx: &mut TestAppContext) {
    let handle = open("existing", true, cx);
    click_start(handle, 0, cx);
    assert_owner(handle, 0, cx);
    cx.simulate_input(handle.into(), "X");
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, _, cx| {
            let view = panes.editors[0].read(cx);
            let session = view.session.borrow();
            assert_eq!(session.history_depths(), (0, 0));
            assert_eq!(
                session.selection(),
                DocumentSelection::collapsed(InlinePoint::at_start_of(view.children[0].0))
            );
            assert_eq!(
                session
                    .document()
                    .node(view.children[0].0)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect::<String>(),
                "existing"
            );
        })
        .unwrap();
}
