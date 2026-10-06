use super::*;
use crate::block_view::SharedSession;
use crate::code_presentation::CodeBlockPresentation;
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use std::cell::Cell;
use xiaomu_core::{
    document::{
        InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    },
    selection::InlinePoint,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

const SOURCE: &str = "A\t中\r\nZ\n";

fn open(
    cx: &mut TestAppContext,
    kind: NodeKind,
    presentation: Option<CodeBlockPresentation>,
) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(SOURCE, MarkSet::empty()).unwrap()]).unwrap(),
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
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document,
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ));
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.set_code_block_presentation(presentation);
                window.focus(&view.focus_handle);
                view
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, session, node)
}

#[gpui::test]
fn code_style_and_padding_are_opt_in_and_do_not_leak_to_ordinary_blocks(cx: &mut TestAppContext) {
    for (kind, presentation, enabled) in [
        (NodeKind::CodeBlock, None, false),
        (
            NodeKind::Paragraph,
            Some(CodeBlockPresentation::default()),
            false,
        ),
        (
            NodeKind::CodeBlock,
            Some(CodeBlockPresentation::default()),
            true,
        ),
    ] {
        let (handle, session, _) = open(cx, kind, presentation);
        let before = session.borrow().document().clone();
        handle
            .update(cx, |view, window, _| {
                let inherited = window.text_style();
                let body_size = inherited.font_size.to_pixels(window.rem_size());
                let layout = view.last_layout.as_ref().unwrap();
                let bounds = view.last_bounds.unwrap();
                assert_eq!(bounds.left(), px(if enabled { 17.0 } else { 0.0 }));
                assert_eq!(bounds.top(), px(if enabled { 15.0 } else { 0.0 }));
                let code_size = body_size * CodeBlockPresentation::FONT_SCALE;
                assert_eq!(
                    layout.lines()[0].font_size(),
                    if enabled { code_size } else { body_size }
                );
                assert_eq!(
                    layout.line_height(),
                    if enabled {
                        body_size * CodeBlockPresentation::LINE_HEIGHT
                    } else {
                        window.line_height()
                    }
                );
                // GPUI splits LF into logical lines; the source CR/tab bytes
                // remain literal and trailing LF still has its empty row.
                let shaped = layout
                    .lines()
                    .iter()
                    .map(|line| line.text.as_ref())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(shaped, SOURCE);
                assert_eq!(view.layout_content().0, SOURCE);
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn code_preedit_paint_candidate_and_hit_test_use_the_same_padded_layout(cx: &mut TestAppContext) {
    let (handle, session, _) = open(
        cx,
        NodeKind::CodeBlock,
        Some(CodeBlockPresentation::default()),
    );
    let original = session.borrow().document().clone();
    let selection = session.borrow().selection();
    handle
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "中文🙂", Some(4..4), window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(view.layout_content().0, format!("中文🙂{SOURCE}"));
            assert!(
                view.cache_key.is_none(),
                "preedit is not a reusable source cache"
            );
            let bounds = view.last_bounds.unwrap();
            let layout = view.last_layout.as_ref().unwrap();
            let line_height = layout.line_height();
            let caret_byte = view.composing_caret_byte().unwrap();
            let expected = layout.position_for_index(caret_byte).unwrap();
            let candidate = view.bounds_for_range(4..4, bounds, window, cx).unwrap();
            assert_eq!(candidate.origin, bounds.origin + expected);
            assert_eq!(candidate.size.height, line_height);
            assert_eq!(
                view.character_index_for_point(candidate.origin, window, cx),
                Some(4)
            );
            assert_eq!(view.bounds_registry.borrow().last().unwrap().1, bounds);
            view.replace_and_mark_text_in_range(None, "", None, window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            assert_eq!(view.layout_content().0, SOURCE);
            assert!(view.cache_key.is_some());
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), original.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn code_theme_changes_reshape_cached_runs_without_document_edits(cx: &mut TestAppContext) {
    let (handle, session, _) = open(
        cx,
        NodeKind::CodeBlock,
        Some(CodeBlockPresentation::default()),
    );
    let before = session.borrow().document().clone();
    let first = handle
        .update(cx, |view, _, _| view.cache_key.unwrap())
        .unwrap();
    let presentation = CodeBlockPresentation {
        text_color: Some(rgba(0x112233ff).into()),
        ..Default::default()
    };
    handle
        .update(cx, |view, _, cx| {
            view.set_code_block_presentation(Some(presentation.clone()));
            assert!(view.cache_key.is_none());
            assert!(view.last_layout.is_none());
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, window, _| {
            assert_ne!(view.cache_key.unwrap(), first);
            assert_eq!(view.epoch.get(), 0);
            let fonts = FontCatalog::from_system(window.text_system());
            let style = block_text_style(window, view.active_code_presentation(), &fonts);
            assert_eq!(style.color, presentation.text_color.unwrap());
            let runs = text_runs(
                &view.layout_content().1,
                style.font.clone(),
                style.color,
                &fonts,
            );
            assert!(
                runs.iter()
                    .all(|run| run.font == style.font && run.color == style.color)
            );
            let same = view.cache_key;
            view.set_code_block_presentation(Some(presentation));
            assert_eq!(
                view.cache_key, same,
                "same config does not discard source layout"
            );
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
