//! Projection/topology tests use GPUI's virtual backend, not real-font metrics.
use super::*;
use crate::inline_atom::{InlineAtomRenderer, InlineAtomRendererRegistry, InlineAtomView};
use crate::inline_atom_display::InlineAtomDisplayProjection;
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::{
    document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, Mark, MarkSet, NodeAttrs, NodeContent,
        NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
    text::TextBuffer,
};
use xiaomu_runtime::session::DocumentSelection;

fn fixture() -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let text = "A\n中";
    let offset = TextBuffer::from_string(text.into()).offset_at(1).unwrap();
    let atoms: Vec<_> = [Mark::Bold, Mark::Underline]
        .into_iter()
        .map(|mark| {
            builder
                .insert(
                    NodeKind::InlineAtom(AtomKind::hard_break()),
                    NodeAttrs::empty(),
                    NodeContent::InlineAtom(
                        InlineAtomContent::hard_break().with_marks(MarkSet::new([mark]).unwrap()),
                    ),
                )
                .unwrap()
        })
        .collect();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(text, MarkSet::empty()).unwrap()],
                    atoms
                        .into_iter()
                        .map(|id| InlineAtomPlacement::new(id, offset)),
                )
                .unwrap(),
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
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

struct Override;
impl InlineAtomRenderer for Override {
    fn display_text(&self, _: &InlineAtomView) -> String {
        "overridden".into()
    }
}

#[test]
fn typed_break_is_intrinsic_even_with_same_named_extension_renderer() {
    let (doc, node) = fixture();
    let mut registry = InlineAtomRendererRegistry::new();
    let extension = AtomKind::new("hardBreak").unwrap();
    registry.register(&extension, Rc::new(Override));
    registry.register(&AtomKind::hard_break(), Rc::new(Override));
    assert!(registry.has_custom_renderer(&extension));
    assert!(!registry.has_custom_renderer(&AtomKind::hard_break()));
    let view = InlineAtomView::new(node, extension.clone(), "fallback", NodeAttrs::empty());
    assert_eq!(
        registry.renderer_for(&extension).display_text(&view),
        "overridden"
    );
    let projection = InlineAtomDisplayProjection::build(&doc, node, &registry).unwrap();
    assert_eq!(projection.canonical_text(), "A\n中");
    assert_eq!(projection.display_text(), "A\n\n\n中");
    assert!(projection.atoms().iter().all(|atom| atom.is_hard_break()));
    assert_eq!(
        projection.atoms()[0].marks(),
        &MarkSet::new([Mark::Bold]).unwrap()
    );
}

#[test]
fn typed_break_ordinals_and_literal_lf_have_distinct_round_trip_coordinates() {
    let (doc, node) = fixture();
    let projection =
        InlineAtomDisplayProjection::build(&doc, node, &InlineAtomRendererRegistry::new()).unwrap();
    let inline = doc.node(node).unwrap().content().as_inline().unwrap();
    for ordinal in 0..=2 {
        let point = InlinePoint::new(
            node,
            inline.offset_at(1).unwrap(),
            ordinal,
            CursorAffinity::Before,
        );
        assert_eq!(
            projection.display_offset_for_inline_point(point),
            Some(1 + ordinal)
        );
        assert_eq!(
            projection.inline_point_for_display_boundary(1 + ordinal, CursorAffinity::Before),
            Some(point)
        );
    }
    let after_literal = InlinePoint::new(
        node,
        inline.offset_at(2).unwrap(),
        0,
        CursorAffinity::Before,
    );
    assert_eq!(
        projection.display_offset_for_inline_point(after_literal),
        Some(4)
    );
    assert_eq!(
        projection.inline_point_for_display_boundary(4, CursorAffinity::Before),
        Some(after_literal)
    );
}

fn open(cx: &mut TestAppContext, ordinal: usize) -> (WindowHandle<ParagraphView>, SharedSession) {
    let (doc, node) = fixture();
    let at = InlinePoint::new(
        node,
        doc.node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap(),
        ordinal,
        CursorAffinity::Before,
    );
    let session = Rc::new(RefCell::new(
        DocumentSession::new(doc, DocumentSelection::collapsed(at)).unwrap(),
    ));
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            cx.new(|cx| {
                let view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                window.focus(&view.focus_handle);
                view
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    (window, session)
}

#[gpui::test]
fn intrinsic_lf_shapes_rows_has_no_chip_and_selection_paints_eol(cx: &mut TestAppContext) {
    let (window, _) = open(cx, 1);
    window
        .update(cx, |view, _, _| {
            let (text, segments) = view.layout_content();
            assert_eq!(text, "A\n\n\n中");
            assert!(segments.iter().any(|s| s.text == "\n" && s.bold));
            assert!(view.layout_atom_ranges().is_empty());
            let layout = view.last_layout.as_ref().expect("virtual shaped layout");
            assert_eq!(layout.lines().len(), 4);
            for byte in 1..=3 {
                let before = layout.position_for_index(byte).unwrap();
                let after = layout.position_for_index(byte + 1).unwrap();
                assert_eq!(after.y - before.y, layout.line_height());
                let selected = layout.selection_rects(byte..byte + 1);
                assert_eq!(selected.len(), 1);
                assert_eq!(selected[0].origin, before);
                assert!(selected[0].size.width > gpui::px(0.));
                assert_eq!(selected[0].size.height, layout.line_height());
            }
        })
        .unwrap();
}

#[gpui::test]
fn preedit_between_breaks_splices_into_correct_display_row_without_canonical_changes(
    cx: &mut TestAppContext,
) {
    for ordinal in 0..=2 {
        let (window, session) = open(cx, ordinal);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "nihao", Some(5..5), window, cx);
                let expected = format!(
                    "A{}nihao{}\n中",
                    "\n".repeat(ordinal),
                    "\n".repeat(2 - ordinal)
                );
                assert_eq!(view.layout_content().0, expected);
                assert_eq!(view.display_content().0, "Anihao\n中");
                assert!(view.layout_atom_ranges().is_empty());
                assert_eq!(view.composing_caret_byte(), Some(1 + ordinal + 5));
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert_eq!(view.layout_content().0, "A\n\n\n中");
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn preedit_and_commit_inherit_the_same_marks_at_each_hard_break_gap(cx: &mut TestAppContext) {
    for ordinal in 0..=2 {
        let (window, session) = open(cx, ordinal);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        let expected = match ordinal {
            0 => MarkSet::empty(),
            1 => MarkSet::new([Mark::Bold]).unwrap(),
            _ => MarkSet::new([Mark::Underline]).unwrap(),
        };
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "你好", Some(2..2), window, cx);
                assert_eq!(view.preedit_marks(), expected);
                assert_eq!(session.borrow().document().store(), before.store());
                view.replace_text_in_range(None, "你好", window, cx);
                let inline = view.inline().unwrap();
                let inserted = inline
                    .runs()
                    .iter()
                    .find(|run| run.text().as_str().contains("你好"))
                    .unwrap();
                assert_eq!(inserted.marks(), &expected);
                assert_eq!(inline.atoms().len(), 2);
            })
            .unwrap();
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
    }
}
