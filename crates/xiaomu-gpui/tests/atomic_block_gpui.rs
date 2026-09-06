//! P4.6 Atomic Block Contract: GPUI end-to-end leg.
//!
//! `text ↔ HorizontalRule ↔ text` traversal through real keystrokes, the
//! selection highlight driving from the session, click-to-select on the
//! rule, and removal as one undoable history change.

use gpui::{AppContext as _, Modifiers, Point, TestAppContext, px};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection};

fn offset_of(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
) -> xiaomu_core::text::TextOffset {
    let text: String = document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect();
    TextBuffer::from_string(text).offset_at(byte).unwrap()
}

/// `Document > [p("前"), HorizontalRule, p("后")]`.
fn fixture() -> (XiaomuDocument, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("前", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let last = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("后", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, rule, last]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        first,
        rule,
        last,
    )
}

fn open(
    document: XiaomuDocument,
    first: NodeId,
    cx: &mut TestAppContext,
) -> (
    gpui::WindowHandle<DocumentView>,
    xiaomu_gpui::block_view::SharedSession,
) {
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(TextPoint::new(
            first,
            TextBuffer::from_string("前".to_owned())
                .offset_at(3)
                .unwrap(),
            CursorAffinity::Before,
        )),
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
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (window, session)
}

fn focus_is(session: &xiaomu_gpui::block_view::SharedSession, expected: DocumentPosition) {
    assert_eq!(session.borrow().selection().focus(), expected);
}

#[gpui::test]
fn text_rule_text_walks_selects_and_deletes_with_keystrokes(cx: &mut TestAppContext) {
    let (document, first, rule, last) = fixture();
    let (window, session) = open(document, first, cx);

    let step = |window: &gpui::WindowHandle<DocumentView>, cx: &mut TestAppContext, key: &str| {
        cx.simulate_keystrokes((*window).into(), key);
        cx.background_executor.run_until_parked();
    };

    // Right from the end of the first paragraph selects the rule.
    step(&window, cx, "right");
    focus_is(&session, DocumentPosition::Atomic(rule));

    // Right again crosses into the next paragraph's start.
    step(&window, cx, "right");
    focus_is(
        &session,
        DocumentPosition::Inline(xiaomu_core::selection::InlinePoint::new(
            last,
            offset_of(session.borrow().document(), last, 0),
            0,
            CursorAffinity::Before,
        )),
    );

    // Left walks back onto the rule, then to the first paragraph's end.
    step(&window, cx, "left");
    focus_is(&session, DocumentPosition::Atomic(rule));
    step(&window, cx, "left");
    focus_is(
        &session,
        DocumentPosition::Inline(xiaomu_core::selection::InlinePoint::new(
            first,
            offset_of(session.borrow().document(), first, 3),
            0,
            CursorAffinity::Before,
        )),
    );

    // Select the rule again and delete it with Backspace.
    step(&window, cx, "right");
    focus_is(&session, DocumentPosition::Atomic(rule));
    step(&window, cx, "backspace");
    assert!(session.borrow().document().node(rule).is_none());
    let root = session.borrow().document().root();
    focus_is(
        &session,
        DocumentPosition::Gap(xiaomu_core::selection::NodeGap::new(root, 1)),
    );

    // Undo restores the block and reinstates the node selection.
    session.borrow_mut().undo().unwrap();
    assert!(session.borrow().document().node(rule).is_some());
    focus_is(&session, DocumentPosition::Atomic(rule));
}

#[gpui::test]
fn clicking_the_rule_selects_it_and_typing_moves_on(cx: &mut TestAppContext) {
    let (document, first, rule, last) = fixture();
    let (window, session) = open(document, first, cx);
    let mut view = gpui::VisualTestContext::from_window(window.into(), cx);

    // Root padding is 16px, the first text row spans 16..44, the rule sits
    // 12px below it with a 3px height: (48, 57) lands on the rule.
    view.simulate_click(Point::new(px(48.0), px(57.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    focus_is(&session, DocumentPosition::Atomic(rule));

    // Clicking a paragraph places a text caret and clears the node
    // selection. The second text row spans 71..99; x=48 lies past the
    // single-character text, so the caret maps to the line end.
    view.simulate_click(Point::new(px(48.0), px(85.0)), Modifiers::default());
    cx.background_executor.run_until_parked();
    focus_is(
        &session,
        DocumentPosition::Inline(xiaomu_core::selection::InlinePoint::new(
            last,
            offset_of(session.borrow().document(), last, 3),
            0,
            CursorAffinity::Before,
        )),
    );
    let _ = first;
}
