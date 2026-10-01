use super::DocumentView;
use crate::{block_view::SharedSession, editor::bind_default_editor_keys};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

fn fixture() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let intro = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("intro", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let mut cells = Vec::new();
    for _ in 0..2 {
        let rule = builder
            .insert(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap();
        cells.push(
            builder
                .insert(
                    NodeKind::TableCell,
                    NodeAttrs::empty(),
                    NodeContent::children([rule]),
                )
                .unwrap(),
        );
    }
    let row = builder
        .insert(
            NodeKind::TableRow,
            NodeAttrs::empty(),
            NodeContent::children(cells.clone()),
        )
        .unwrap();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children([row]),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([intro, table]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        intro,
        cells[0],
        cells[1],
    )
}

fn open(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    intro: NodeId,
    left: NodeId,
    right: NodeId,
) -> (WindowHandle<DocumentView>, SharedSession) {
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document,
            DocumentSelection::collapsed(InlinePoint::at_start_of(intro)),
        )
        .unwrap(),
    ));
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| DocumentView::new(session.clone()))
        })
        .unwrap()
    });
    window
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            view.install_cell_range(left, right, window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (window, session)
}

#[gpui::test]
fn rectangular_native_ime_proxy_cancels_without_mutation_and_commits_once(cx: &mut TestAppContext) {
    let (document, intro, left, right) = fixture();
    let before = document.clone();
    let (window, session) = open(cx, document, intro, left, right);
    let selection = session.borrow().selection();
    window
        .update(cx, |view, window, cx| {
            let input = view.range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                assert_eq!(
                    input.selected_text_range(false, window, cx).unwrap().range,
                    0..0
                );
                input.replace_and_mark_text_in_range(None, "nihao", Some(2..2), window, cx);
                assert_eq!(input.marked_text_range(window, cx), Some(0..5));
                assert_eq!(
                    input.selected_text_range(false, window, cx).unwrap().range,
                    2..2
                );
                assert_eq!(input.display_content().0, "nihao");
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
    window
        .update(cx, |view, window, cx| {
            let input = view.range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert_eq!(input.marked_text_range(window, cx), None);
                assert_eq!(input.display_content().0, "");
            });
        })
        .unwrap();
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().document().store(), before.store());
    window
        .update(cx, |view, window, cx| {
            let input = view.range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(Some(0..0), "你好🙂", Some(4..4), window, cx);
                input.replace_text_in_range(Some(0..4), "你好🙂", window, cx);
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().history_depths(), (1, 0));
    cx.simulate_input(window.into(), "!");
    let focus = session
        .borrow()
        .selection()
        .as_single_node()
        .unwrap()
        .focus();
    assert_eq!(
        session.borrow().document().parent_of(focus.node_id()),
        Some(left)
    );
    let text: String = session
        .borrow()
        .document()
        .node(focus.node_id())
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    assert_eq!(text, "你好🙂!");
    cx.simulate_keystrokes(window.into(), "ctrl-z ctrl-z");
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), selection);
}

#[gpui::test]
fn rectangular_input_and_clipboard_do_not_leak_to_another_editor(cx: &mut TestAppContext) {
    let (document, intro, left, right) = fixture();
    let (a, session_a) = open(cx, document.clone(), intro, left, right);
    let (_b, session_b) = open(cx, document.clone(), intro, left, right);
    let selection_b = session_b.borrow().selection();
    a.update(cx, |view, window, cx| {
        window.activate_window();
        view.focus_selection(window, cx);
    })
    .unwrap();
    cx.simulate_input(a.into(), "A");
    cx.simulate_keystrokes(a.into(), "ctrl-z ctrl-c ctrl-v ctrl-z");
    assert_eq!(session_b.borrow().document().store(), document.store());
    assert_eq!(session_b.borrow().selection(), selection_b);
    assert_eq!(session_b.borrow().history_depths(), (0, 0));
    assert_eq!(session_a.borrow().document().store(), document.store());
}
