use gpui::{AppContext as _, TestAppContext};
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_gpui::document_view::DocumentView;
use xiaomu_gpui::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use xiaomu_runtime::session::{DocumentPosition, DocumentSelection, EditIntent, SessionOutcome};

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn point(document: &XiaomuDocument, node: NodeId, raw: usize) -> TextPoint {
    let inline = document.node(node).unwrap().content().as_inline().unwrap();
    TextPoint::new(node, inline.offset_at(raw).unwrap(), CursorAffinity::Before)
}

fn focus(session: &xiaomu_gpui::block_view::SharedSession) -> TextPoint {
    match session.borrow().selection().focus() {
        DocumentPosition::Text(point) => point,
        DocumentPosition::Gap(_) => panic!("visual navigation must keep a text focus"),
    }
}

#[gpui::test]
fn unicode_wrapped_navigation_matrix_keeps_canonical_boundaries(cx: &mut TestAppContext) {
    let samples = [
        ("ascii", "alpha "),
        ("cjk", "中文测试"),
        ("mixed", "A中B "),
        ("emoji", "👍🏽🚀"),
        ("combining", "e\u{301} "),
        ("cjk_emoji", "中👍文"),
        ("bidi", "abc אבג مرحبا "),
    ];

    let mut builder = NodeStoreBuilder::new();
    let mut cases = Vec::new();
    for (label, sample) in samples {
        let text = sample.repeat(256);
        let node = paragraph(&mut builder, &text);
        cases.push((label, node, text));
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(cases.iter().map(|(_, node, _)| *node)),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let first = &cases[0];
    let initial = DocumentSelection::collapsed(point(&document, first.1, first.2.len()));
    let editor = EditorInstance::new(document, initial, EditorHooks::default()).unwrap();
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

    for (label, node, text) in cases {
        let end = point(session.borrow().document(), node, text.len());
        let move_outcome = session
            .borrow_mut()
            .apply_intent(&EditIntent::SetSelection {
                anchor: end,
                focus: end,
            })
            .unwrap();
        assert!(
            matches!(
                move_outcome,
                SessionOutcome::NoChange | SessionOutcome::SelectionChanged
            ),
            "case {label} must land on its target block"
        );
        window
            .update(cx, |view: &mut DocumentView, window, cx| {
                view.focus_selection(window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();

        cx.simulate_keystrokes(window.into(), "home");
        let row_start = focus(&session);
        assert_eq!(row_start.node_id(), node, "home left block for {label}");
        assert!(
            row_start.offset().as_usize() > 0,
            "long {label} fixture must wrap so Home is visual-line-local"
        );
        assert!(row_start.offset().as_usize() < text.len());
        session
            .borrow()
            .selection()
            .validate(session.borrow().document())
            .unwrap();

        cx.simulate_keystrokes(window.into(), "end");
        let row_end = focus(&session);
        assert_eq!(row_end.node_id(), node, "end left block for {label}");
        assert_eq!(
            row_end.offset().as_usize(),
            text.len(),
            "End must return to the final visual-row edge for {label}"
        );

        cx.simulate_keystrokes(window.into(), "up");
        let above = focus(&session);
        assert_eq!(
            above.node_id(),
            node,
            "Up crossed block unexpectedly for {label}"
        );
        session
            .borrow()
            .selection()
            .validate(session.borrow().document())
            .unwrap();

        cx.simulate_keystrokes(window.into(), "down");
        let down = focus(&session);
        assert_eq!(
            down.node_id(),
            node,
            "Down crossed block unexpectedly for {label}"
        );
        session
            .borrow()
            .selection()
            .validate(session.borrow().document())
            .unwrap();
    }
}
