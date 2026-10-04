//! Real GPUI pointer dispatch through the task control, not callback-only tests.

#[path = "task_checkbox_race_tests.rs"]
mod race_tests;

use super::DocumentView;
use crate::{
    block_view::SharedSession,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
};
use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler, MouseButton, Render, TestAppContext,
    VisualTestContext, Window, WindowHandle, div, prelude::*, px,
};
use xiaomu_core::{
    document::{
        AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, TextPoint},
};
use xiaomu_runtime::session::{
    DocumentSelection, EditIntent, IntentDisposition, PolicyError, SessionContext, SessionPolicy,
};

pub(super) fn fixture() -> (XiaomuDocument, [NodeId; 2], [NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let blocks = std::array::from_fn(|_| {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("task text", MarkSet::empty()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap()
    });
    let items = blocks.map(|block| {
        builder
            .insert(
                NodeKind::TaskItem,
                NodeAttrs::new(
                    [
                        ("checked".into(), AttrValue::Null),
                        ("host-data".into(), AttrValue::String("preserve".into())),
                    ]
                    .into(),
                )
                .unwrap(),
                NodeContent::children([block]),
            )
            .unwrap()
    });
    let list = builder
        .insert(
            NodeKind::TaskList,
            NodeAttrs::empty(),
            NodeContent::children(items),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([list]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        items,
        blocks,
    )
}

pub(super) fn other_item_range(document: &XiaomuDocument, block: NodeId) -> DocumentSelection {
    let inline = document.node(block).unwrap().content().as_inline().unwrap();
    DocumentSelection::new(
        TextPoint::new(block, inline.offset_at(7).unwrap(), CursorAffinity::After),
        TextPoint::new(block, inline.offset_at(2).unwrap(), CursorAffinity::Before),
    )
}

pub(super) fn checked_value(session: &SharedSession, item: NodeId) -> Option<AttrValue> {
    session
        .borrow()
        .document()
        .node(item)
        .unwrap()
        .attrs()
        .get("checked")
        .cloned()
}

fn checkbox_point(
    handle: impl Into<gpui::AnyWindowHandle>,
    item: NodeId,
    cx: &mut TestAppContext,
) -> gpui::Point<gpui::Pixels> {
    // GPUI's test-only lookup accepts a static selector. This tiny per-test
    // allocation does not escape into production control identity or attrs.
    let selector = Box::leak(format!("task-checkbox-{item:?}").into_boxed_str());
    VisualTestContext::from_window(handle.into(), cx)
        .debug_bounds(selector)
        .expect("native checkbox hitbox was painted")
        .center()
}

pub(super) fn open(editor: EditorInstance, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    let handle = cx.update(|cx| {
        bind_default_editor_keys(cx);
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
}

#[gpui::test]
fn native_checkbox_preserves_other_item_range_and_undo_redo(cx: &mut TestAppContext) {
    let (document, items, blocks) = fixture();
    let selection = other_item_range(&document, blocks[1]);
    let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    // Merely painting null never normalizes it to false.
    assert_eq!(session.borrow().document().store(), document.store());
    let point = checkbox_point(handle, items[0], cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_down(point, MouseButton::Left, Default::default());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(checked_value(&session, items[0]), Some(AttrValue::Null));
    handle
        .update(cx, |view, _, _| assert!(!view.is_dragging))
        .unwrap();
    visual.simulate_mouse_up(point, MouseButton::Left, Default::default());
    assert_eq!(
        checked_value(&session, items[0]),
        Some(AttrValue::Bool(true))
    );
    assert_eq!(checked_value(&session, items[1]), Some(AttrValue::Null));
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(
        session
            .borrow()
            .document()
            .node(items[0])
            .unwrap()
            .attrs()
            .get("host-data"),
        Some(&AttrValue::String("preserve".into()))
    );
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(blocks[1])
            );
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
    cx.simulate_keystrokes(handle.into(), "ctrl-shift-z");
    assert_eq!(
        checked_value(&session, items[0]),
        Some(AttrValue::Bool(true))
    );
    assert_eq!(session.borrow().selection(), selection);
    VisualTestContext::from_window(handle.into(), cx).simulate_click(point, Default::default());
    assert_eq!(
        checked_value(&session, items[0]),
        Some(AttrValue::Bool(false))
    );
    assert_eq!(session.borrow().history_depths(), (2, 0));
}

struct Pair {
    first: Entity<DocumentView>,
    second: Entity<DocumentView>,
}
impl Render for Pair {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(div().w(px(300.)).h_full().child(self.first.clone()))
            .child(div().w(px(300.)).h_full().child(self.second.clone()))
    }
}

struct CheckboxPolicy {
    no_change: bool,
}
impl SessionPolicy for CheckboxPolicy {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::SetTaskChecked { .. }) {
            if self.no_change {
                Ok(IntentDisposition::NoChange)
            } else {
                Err(PolicyError::new("read-only document"))
            }
        } else {
            Ok(IntentDisposition::Continue)
        }
    }
}

#[gpui::test]
fn native_checkbox_cross_pane_focus_includes_accepted_no_change(cx: &mut TestAppContext) {
    for no_change in [false, true] {
        let (document, items, blocks) = fixture();
        let selection = other_item_range(&document, blocks[1]);
        let first = if no_change {
            EditorInstance::new_with_policy(
                document.clone(),
                selection,
                EditorHooks::default(),
                Box::new(CheckboxPolicy { no_change: true }),
            )
            .unwrap()
        } else {
            EditorInstance::new(document.clone(), selection, EditorHooks::default()).unwrap()
        };
        // A different document allocation keeps debug selectors unambiguous.
        let mut builder = NodeStoreBuilder::new();
        let block = builder
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
                NodeContent::children([block]),
            )
            .unwrap();
        let second = EditorInstance::new(
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            DocumentSelection::collapsed(TextPoint::at_start_of(block)),
            EditorHooks::default(),
        )
        .unwrap();
        let first_session = first.session().clone();
        let second_session = second.session().clone();
        let second_before = second_session.borrow().document().clone();
        let handle = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Pair {
                    first: cx.new(|_| first.build_view()),
                    second: cx.new(|_| second.build_view()),
                })
            })
            .unwrap()
        });
        handle
            .update(cx, |pair, window, cx| {
                window.activate_window();
                pair.second
                    .update(cx, |view, cx| view.focus_selection(window, cx));
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        let point = checkbox_point(handle, items[0], cx);
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.simulate_mouse_down(point, MouseButton::Left, Default::default());
        handle
            .update(cx, |pair, window, cx| {
                assert_eq!(
                    pair.second
                        .read(cx)
                        .accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner(),
                    Some(block)
                );
                assert_eq!(
                    pair.first
                        .read(cx)
                        .accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner(),
                    None
                );
            })
            .unwrap();
        visual.simulate_mouse_up(point, MouseButton::Left, Default::default());
        handle
            .update(cx, |pair, window, cx| {
                assert_eq!(
                    pair.first
                        .read(cx)
                        .accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner(),
                    Some(blocks[1])
                );
                assert_eq!(
                    pair.second
                        .read(cx)
                        .accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner(),
                    None
                );
            })
            .unwrap();
        assert_eq!(first_session.borrow().selection(), selection);
        assert_eq!(
            checked_value(&first_session, items[0]),
            Some(if no_change {
                AttrValue::Null
            } else {
                AttrValue::Bool(true)
            })
        );
        assert_eq!(
            first_session.borrow().history_depths(),
            (usize::from(!no_change), 0)
        );
        assert_eq!(
            second_session.borrow().document().store(),
            second_before.store()
        );
        assert_eq!(second_session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn native_checkbox_read_only_policy_keeps_document_selection_and_history(cx: &mut TestAppContext) {
    let (document, items, blocks) = fixture();
    let selection = other_item_range(&document, blocks[1]);
    let editor = EditorInstance::new_with_policy(
        document.clone(),
        selection,
        EditorHooks::default(),
        Box::new(CheckboxPolicy { no_change: false }),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    let epoch = handle.update(cx, |view, _, _| view.epoch.get()).unwrap();
    let point = checkbox_point(handle, items[0], cx);
    VisualTestContext::from_window(handle.into(), cx).simulate_click(point, Default::default());
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
    handle
        .update(cx, |view, _, _| assert_eq!(view.epoch.get(), epoch))
        .unwrap();
}

#[gpui::test]
fn platform_unmark_before_checkbox_pointer_keeps_committed_text_and_separate_undo(
    cx: &mut TestAppContext,
) {
    let (document, items, blocks) = fixture();
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(TextPoint::at_start_of(blocks[1])),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = open(editor, cx);
    handle
        .update(cx, |view, window, cx| {
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == blocks[1])
                .unwrap()
                .1
                .clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "你好", None, window, cx)
            });
            assert!(view.has_active_composition(cx));
            // Model the stock platform callback order; the virtual platform has
            // no XIM. Unmark is the platform's work, not the checkbox's work.
            child.update(cx, |child, cx| child.unmark_text(window, cx));
            assert!(!view.has_active_composition(cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let before_click = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let point = checkbox_point(handle, items[0], cx);
    VisualTestContext::from_window(handle.into(), cx).simulate_click(point, Default::default());
    assert_eq!(
        checked_value(&session, items[0]),
        Some(AttrValue::Bool(true))
    );
    assert_eq!(session.borrow().selection(), selection);
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    assert_eq!(session.borrow().document().store(), before_click.store());
    assert_eq!(session.borrow().selection(), selection);
}
