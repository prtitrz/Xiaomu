//! Native input protocol + actual shaped layout, including atom seam ordinals.
use super::*;
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::session::DocumentSelection;

pub(super) fn open(
    cx: &mut TestAppContext,
    ordinal: usize,
) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let offset = TextBuffer::from_string("A中Z".into()).offset_at(1).unwrap();
    let atoms: Vec<_> = ["@Ann", "🙂"]
        .into_iter()
        .map(|text| {
            b.insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new(text).unwrap()),
            )
            .unwrap()
        })
        .collect();
    let p = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [
                        TextRun::new("A", MarkSet::empty()).unwrap(),
                        TextRun::new("中", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                        TextRun::new("Z", MarkSet::empty()).unwrap(),
                    ],
                    atoms
                        .into_iter()
                        .map(|atom| InlineAtomPlacement::new(atom, offset)),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([p]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let at = InlinePoint::new(p, offset, ordinal, CursorAffinity::Before);
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
                    p,
                    cx,
                );
                window.focus(&view.focus_handle);
                view
            })
        })
        .unwrap()
    });
    cx.background_executor.run_until_parked();
    (window, session, p)
}

fn visual(ordinal: usize, preedit: &str) -> String {
    let chips = ["@Ann", "🙂"];
    format!(
        "A{}{}{}中Z",
        chips[..ordinal].concat(),
        preedit,
        chips[ordinal..].concat()
    )
}

#[gpui::test]
fn preedit_is_visible_before_between_and_after_atoms_without_hiding_chips(cx: &mut TestAppContext) {
    for ordinal in 0..=2 {
        let (window, session, _) = open(cx, ordinal);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                assert_eq!(
                    view.selected_text_range(false, window, cx).unwrap().range,
                    1..1
                );
                view.replace_and_mark_text_in_range(None, "nihao", Some(2..2), window, cx);
                assert!(view.is_composing(), "seam ordinal {ordinal}");
                assert_eq!(
                    view.display_content().0,
                    "Anihao中Z",
                    "platform text excludes renderer bytes"
                );
                let (layout, segments) = view.layout_content();
                assert_eq!(layout, visual(ordinal, "nihao"));
                assert!(segments.iter().any(|s| s.text == "nihao" && s.underline));
                assert_eq!(view.marked_text_range(window, cx), Some(1..6));
                assert_eq!(
                    view.selected_text_range(false, window, cx).unwrap().range,
                    3..3
                );
                let preceding: usize = ["@Ann", "🙂"][..ordinal].iter().map(|s| s.len()).sum();
                assert_eq!(view.composing_caret_byte(), Some(1 + preceding + 2));
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        window
            .update(cx, |view, window, cx| {
                // Candidate geometry must refer to the same atom-aware layout as paint.
                let bounds = view.last_bounds.unwrap();
                let caret = view.bounds_for_range(3..3, bounds, window, cx).unwrap();
                let byte = view.composing_caret_byte().unwrap();
                let expected = view
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_index(byte)
                    .unwrap();
                assert_eq!(caret.left(), bounds.left() + expected.x);
                assert_eq!(
                    view.character_index_for_point(caret.origin, window, cx),
                    Some(3)
                );
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert!(!view.is_composing());
                assert_eq!(view.layout_content().0, "A@Ann🙂中Z");
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
}

#[gpui::test]
fn ime_commit_preserves_the_chosen_atom_gap_and_exact_undo(cx: &mut TestAppContext) {
    for ordinal in 0..=2 {
        let (window, session, p) = open(cx, ordinal);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                // Explicit UTF-16 echo from the platform must not reset the seam ordinal.
                view.replace_and_mark_text_in_range(Some(1..1), "你好🙂", Some(4..4), window, cx);
                assert_eq!(view.layout_content().0, visual(ordinal, "你好🙂"));
                view.replace_text_in_range(Some(1..5), "你好🙂", window, cx);
                assert_eq!(view.layout_content().0, visual(ordinal, "你好🙂"));
            })
            .unwrap();
        assert_eq!(session.borrow().history_depths(), (1, 0));
        let inline = session
            .borrow()
            .document()
            .node(p)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .clone();
        for (index, atom) in inline.atoms().iter().enumerate() {
            assert_eq!(
                atom.text_offset().as_usize(),
                if index < ordinal { 1 } else { 11 }
            );
        }
        let after = session.borrow().document().clone();
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
        assert_eq!(session.borrow().selection(), selection);
        session.borrow_mut().redo().unwrap();
        assert_eq!(session.borrow().document().store(), after.store());
    }
}

#[gpui::test]
fn native_range_echo_and_idle_candidate_bounds_preserve_each_gap(cx: &mut TestAppContext) {
    for ordinal in 0..=2 {
        let (window, _, _) = open(cx, ordinal);
        window
            .update(cx, |view, window, cx| {
                assert_eq!(
                    view.selected_text_range(false, window, cx).unwrap().range,
                    1..1
                );
                let byte = 1 + ["@Ann", "🙂"][..ordinal]
                    .iter()
                    .map(|s| s.len())
                    .sum::<usize>();
                let bounds = view.last_bounds.unwrap();
                let caret = view.bounds_for_range(1..1, bounds, window, cx).unwrap();
                let expected = view
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .position_for_index(byte)
                    .unwrap();
                assert_eq!(caret.left(), bounds.left() + expected.x);
                view.replace_text_in_range(Some(1..1), "X", window, cx);
                assert_eq!(view.layout_content().0, visual(ordinal, "X"));
            })
            .unwrap();
    }
}

#[gpui::test]
fn ime_replaces_text_beside_atoms_and_rejects_ranges_spanning_them(cx: &mut TestAppContext) {
    let (window, session, p) = open(cx, 2);
    let inline = session
        .borrow()
        .document()
        .node(p)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .clone();
    let start = InlinePoint::new(p, inline.offset_at(1).unwrap(), 2, CursorAffinity::Before);
    let end = InlinePoint::new(p, inline.offset_at(4).unwrap(), 0, CursorAffinity::Before);
    session
        .borrow_mut()
        .set_inline_selection(end, start)
        .unwrap();
    let before = session.borrow().document().clone();
    window
        .update(cx, |view, window, cx| {
            let selected = view.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selected.range, 1..2);
            assert!(selected.reversed);
            view.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            assert_eq!(view.layout_content().0, "A@Ann🙂niZ");
            view.replace_and_mark_text_in_range(None, "你好🙂", Some(4..4), window, cx);
            assert_eq!(view.layout_content().0, "A@Ann🙂你好🙂Z");
            assert_eq!(view.marked_text_range(window, cx), Some(1..5));
            let mut adjusted = None;
            assert_eq!(
                view.text_for_range(1..5, &mut adjusted, window, cx),
                Some("你好🙂".into())
            );
            view.replace_text_in_range(None, "你好🙂", window, cx);
            assert_eq!(view.layout_content().0, "A@Ann🙂你好🙂Z");
        })
        .unwrap();
    assert_eq!(session.borrow().history_depths(), (1, 0));
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    let zero = InlinePoint::at_start_of(p);
    session
        .borrow_mut()
        .set_inline_selection(zero, end)
        .unwrap();
    let rejected_selection = session.borrow().selection();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            assert!(view.rejected_composition);
            view.replace_and_mark_text_in_range(None, "nihao", None, window, cx);
            view.replace_text_in_range(None, "你好", window, cx);
            assert!(!view.is_composing());
            assert_eq!(view.layout_content().0, "A@Ann🙂中Z");
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), rejected_selection);
}

#[gpui::test]
fn long_unicode_preedit_wraps_with_chips_and_candidate_geometry(cx: &mut TestAppContext) {
    let (window, session, _) = open(cx, 1);
    let before = session.borrow().document().clone();
    let preedit = "中文🙂 e\u{301} nihao ".repeat(60);
    let units = preedit.encode_utf16().count();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, &preedit, Some(units..units), window, cx);
            let (layout, _) = view.layout_content();
            assert_eq!(layout, visual(1, &preedit));
            let chips = view.layout_atom_ranges();
            assert_eq!(&layout[chips[0].clone()], "@Ann");
            assert_eq!(&layout[chips[1].clone()], "🙂");
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    window
        .update(cx, |view, window, cx| {
            let layout = view.last_layout.as_ref().unwrap();
            assert!(
                layout.size().height > layout.line_height() * 2.0,
                "fixture must wrap"
            );
            let expected = layout
                .position_for_index(view.composing_caret_byte().unwrap())
                .unwrap();
            let bounds = view.last_bounds.unwrap();
            let actual = view
                .bounds_for_range(1 + units..1 + units, bounds, window, cx)
                .unwrap();
            assert_eq!(actual.origin, bounds.origin + expected);
            view.unmark_text(window, cx);
            assert_eq!(view.layout_content().0, "A@Ann🙂中Z");
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn cancelling_preedit_repaints_canonical_layout_without_a_followup_edit(cx: &mut TestAppContext) {
    let (window, session, _) = open(cx, 2);
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    for text in ["z", "zhong", "zhongwen"] {
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, text, None, window, cx);
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        window
            .update(cx, |view, _, _| {
                let shaped: String = view
                    .last_layout
                    .as_ref()
                    .unwrap()
                    .lines()
                    .iter()
                    .map(|line| line.text.as_ref())
                    .collect();
                assert_eq!(shaped, visual(2, text));
            })
            .unwrap();
    }
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "", None, window, cx);
            assert!(!view.is_composing());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    window
        .update(cx, |view, _, _| {
            let shaped: String = view
                .last_layout
                .as_ref()
                .unwrap()
                .lines()
                .iter()
                .map(|line| line.text.as_ref())
                .collect();
            assert_eq!(shaped, "A@Ann🙂中Z");
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().selection(), selection);
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
