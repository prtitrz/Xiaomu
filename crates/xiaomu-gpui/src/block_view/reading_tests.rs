//! Display-only reading paint preserves exact atom seams and canonical marks.
use super::super::*;
use crate::document_view::{ReadingRange, reading::SharedReadingState};
use gpui::{AppContext as _, TestAppContext};
use xiaomu_core::{
    document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, MarkSet, NodeAttrs, NodeContent,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
    text::TextBuffer,
};
use xiaomu_runtime::session::DocumentSelection;

#[gpui::test]
fn reading_highlights_share_atom_projection_and_do_not_change_shape(cx: &mut TestAppContext) {
    let mut builder = NodeStoreBuilder::new();
    let offset = TextBuffer::from_string("A中".into()).offset_at(1).unwrap();
    let hard_break = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let label = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("@reference").unwrap()),
        )
        .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new("A中", MarkSet::empty()).unwrap()],
                    [
                        InlineAtomPlacement::new(hard_break, offset),
                        InlineAtomPlacement::new(label, offset),
                    ],
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
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ));
    let state: SharedReadingState = Default::default();
    let snapshot = state.borrow().snapshot(&session.borrow());
    let point = |ordinal| InlinePoint::new(node, offset, ordinal, CursorAffinity::Before);
    {
        let mut state = state.borrow_mut();
        state.snapshot = Some(snapshot.clone());
        state.highlights.insert(
            node,
            vec![
                (ReadingRange::new(point(0), point(1)), false),
                (ReadingRange::new(point(1), point(2)), true),
            ],
        );
    }
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.attach_reading_state(state.clone());
                view
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            let layout = view.last_layout.as_ref().unwrap();
            let bounds = view.last_bounds.unwrap();
            let quads = view.reading_highlight_quads(layout, bounds, bounds);
            assert_eq!(
                quads.len(),
                2,
                "HardBreak EOL and renderer label have separate quads"
            );
            let projection = view.atom_display_projection().unwrap();
            for (quad, (start, end)) in quads
                .iter()
                .zip([(point(0), point(1)), (point(1), point(2))])
            {
                let start = projection.display_offset_for_inline_point(start).unwrap();
                let end = projection.display_offset_for_inline_point(end).unwrap();
                let mut expected = layout.selection_rects(start..end)[0];
                expected.origin += bounds.origin;
                assert_eq!(quad.bounds, expected);
            }
            let before = view.layout_content();
            state.borrow_mut().highlights.clear();
            assert!(
                view.reading_highlight_quads(layout, bounds, bounds)
                    .is_empty()
            );
            assert_eq!(view.layout_content().0, before.0);
            assert_eq!(session.borrow().document().store(), document.store());
            assert_eq!(session.borrow().history_depths(), (0, 0));
            // Reusing even identical canonical data in a replacement session
            // makes the old decoration capability unpaintable.
            *session.borrow_mut() = DocumentSession::new(
                document,
                DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
            )
            .unwrap();
            state
                .borrow_mut()
                .highlights
                .insert(node, vec![(ReadingRange::new(point(1), point(2)), true)]);
            assert!(
                view.reading_highlight_quads(layout, bounds, bounds)
                    .is_empty()
            );
        })
        .unwrap();
}

#[gpui::test]
fn reading_highlights_cull_large_single_block_before_rectangles(cx: &mut TestAppContext) {
    use super::super::layout::BlockTextLayout;
    use gpui::{Bounds, point, px, size};
    let mut builder = NodeStoreBuilder::new();
    let text = "x\n".repeat(3000);
    let inline = InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap();
    let offsets: Vec<_> = (0..3000)
        .map(|index| {
            (
                inline.offset_at(index * 2).unwrap(),
                inline.offset_at(index * 2 + 1).unwrap(),
            )
        })
        .collect();
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
    let session = Rc::new(RefCell::new(
        DocumentSession::new(
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ));
    let state: SharedReadingState = Default::default();
    let snapshot = state.borrow().snapshot(&session.borrow());
    state.borrow_mut().snapshot = Some(snapshot);
    state.borrow_mut().highlights.insert(
        node,
        offsets
            .into_iter()
            .map(|(start, end)| {
                (
                    ReadingRange::new(
                        InlinePoint::new(node, start, 0, CursorAffinity::Before),
                        InlinePoint::new(node, end, 0, CursorAffinity::Before),
                    ),
                    false,
                )
            })
            .collect(),
    );
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session.clone(),
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.attach_reading_state(state.clone());
                view
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, _, _| {
            let layout = view.last_layout.as_ref().unwrap();
            let bounds = view.last_bounds.unwrap();
            let clip = Bounds::new(
                point(bounds.left(), bounds.top() + layout.line_height() * 1200.0),
                size(bounds.size.width, layout.line_height() * 2.0),
            );
            BlockTextLayout::reading_row_visits(true);
            let quads = view.reading_highlight_quads(layout, bounds, clip);
            assert!(!quads.is_empty());
            assert!(quads.len() <= 3);
            assert!(
                BlockTextLayout::reading_row_visits(false) <= 3,
                "only visible matched rows may be visited"
            );
            assert_eq!(
                state.borrow().highlights[&node].len(),
                3000,
                "culling never truncates the stored result set"
            );
            assert!(
                quads
                    .iter()
                    .all(|quad| quad.bounds.intersect(&clip).size.height > px(0.0))
            );
        })
        .unwrap();
}
