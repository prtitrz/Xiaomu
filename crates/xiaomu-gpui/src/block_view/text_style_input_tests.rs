//! Runtime inheritance -> native transient projection -> committed run parity.
//! These tests do not alter the platform transport or composition state machine.

use super::*;
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, LinkAttributes, LinkMark, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, StringAttribute, TextRun,
    TextStyleAttributes, TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::{DocumentSelection, SessionOutcome};

fn style(color: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(StringAttribute::Value(color.into()))
            .with_font_family(StringAttribute::Value("'Noto Sans SC', sans-serif".into())),
    ))
}

fn open(
    cx: &mut TestAppContext,
    atoms: bool,
) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
    open_with_marks(
        cx,
        atoms,
        MarkSet::new([Mark::Bold, style("red")]).unwrap(),
        MarkSet::new([style("green")]).unwrap(),
    )
}

fn open_with_marks(
    cx: &mut TestAppContext,
    atoms: bool,
    first: MarkSet,
    rest: MarkSet,
) -> (WindowHandle<ParagraphView>, SharedSession, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = InlineContent::new([
        TextRun::new("A", first).unwrap(),
        TextRun::new("中Z", rest).unwrap(),
    ])
    .unwrap();
    let offset = inline.offset_at(1).unwrap();
    let inline = if atoms {
        let atom = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::new("@Ann").unwrap()),
            )
            .unwrap();
        InlineContent::with_atoms(
            inline.runs().iter().cloned(),
            [InlineAtomPlacement::new(atom, offset)],
        )
        .unwrap()
    } else {
        inline
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
    let point = InlinePoint::new(node, offset, usize::from(atoms), CursorAffinity::Before);
    let session = Rc::new(RefCell::new(
        DocumentSession::new(document, DocumentSelection::collapsed(point)).unwrap(),
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
    (window, session, node)
}

fn preedit_style(view: &ParagraphView, expected: &Mark) {
    let Mark::TextStyle(mark) = expected else {
        panic!("fixture is a style")
    };
    for (_, segments) in [view.display_content(), view.layout_content()] {
        let preedit = segments
            .iter()
            .find(|segment| segment.text == "中文🙂")
            .unwrap();
        assert_eq!(preedit.text_style.as_ref(), Some(mark.attributes()));
        assert!(preedit.underline);
        let fonts = text_style::FontCatalog::from_names(&["Noto Sans SC"]);
        let runs = text_style::text_runs(
            std::slice::from_ref(preedit),
            gpui::font("Host UI"),
            gpui::rgba(0x111111ff).into(),
            &fonts,
        );
        assert_eq!(runs[0].font.family.as_ref(), "Noto Sans SC");
        assert_eq!(runs[0].len, "中文🙂".len());
    }
}

#[gpui::test]
fn pending_style_nochange_is_visible_in_both_preedit_projections_without_epoch_or_document_edits(
    cx: &mut TestAppContext,
) {
    for atoms in [false, true] {
        let (window, session, _) = open(cx, atoms);
        let before = session.borrow().document().clone();
        let selection = session.borrow().selection();
        window
            .update(cx, |view, window, cx| {
                view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
                preedit_style(view, &style("red"));
                let epoch = view.epoch.get();
                assert!(matches!(
                    session
                        .borrow_mut()
                        .apply_intent(&EditIntent::SetMark {
                            mark: style("blue")
                        })
                        .unwrap(),
                    SessionOutcome::NoChange
                ));
                assert_eq!(view.epoch.get(), epoch);
                preedit_style(view, &style("blue"));
                let (_, segments) = view.layout_content();
                if atoms {
                    let chip = segments
                        .iter()
                        .find(|segment| segment.text == "@Ann")
                        .unwrap();
                    assert!(chip.text_style.is_none() && !chip.bold && !chip.underline);
                }
                assert_eq!(session.borrow().document().store(), before.store());
                assert_eq!(session.borrow().selection(), selection);
                assert_eq!(session.borrow().history_depths(), (0, 0));
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert!(!view.is_composing());
                assert!(
                    session
                        .borrow()
                        .stored_marks()
                        .unwrap()
                        .as_slice()
                        .contains(&style("blue"))
                );
                view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
                preedit_style(view, &style("blue"));
                view.replace_text_in_range(None, "中文🙂", window, cx);
                let (_, segments) = view.layout_content();
                let inserted = segments
                    .iter()
                    .find(|segment| segment.text == "中文🙂")
                    .unwrap();
                let Mark::TextStyle(mark) = style("blue") else {
                    unreachable!()
                };
                assert_eq!(inserted.text_style.as_ref(), Some(mark.attributes()));
            })
            .unwrap();
        assert_eq!(session.borrow().history_depths(), (1, 0));
        session.borrow_mut().undo().unwrap();
        assert_eq!(session.borrow().document().store(), before.store());
    }
}

#[gpui::test]
fn replacement_preedit_inherits_range_start_not_reverse_selection_focus_and_matches_commit(
    cx: &mut TestAppContext,
) {
    for atoms in [false, true] {
        for reverse in [false, true] {
            let (window, session, node) = open(cx, atoms);
            let inline = session
                .borrow()
                .document()
                .node(node)
                .unwrap()
                .content()
                .as_inline()
                .unwrap()
                .clone();
            let start = InlinePoint::new(
                node,
                inline.offset_at(1).unwrap(),
                usize::from(atoms),
                CursorAffinity::Before,
            );
            let end = InlinePoint::new(
                node,
                inline.offset_at(4).unwrap(),
                0,
                CursorAffinity::Before,
            );
            session
                .borrow_mut()
                .set_inline_selection(
                    if reverse { end } else { start },
                    if reverse { start } else { end },
                )
                .unwrap();
            let before = session.borrow().document().clone();
            let selection = session.borrow().selection();
            window
                .update(cx, |view, window, cx| {
                    view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
                    preedit_style(view, &style("red"));
                    assert_eq!(session.borrow().document().store(), before.store());
                    assert_eq!(session.borrow().selection(), selection);
                    view.replace_text_in_range(None, "中文🙂", window, cx);
                    let (_, segments) = view.layout_content();
                    let inserted = segments
                        .iter()
                        .find(|segment| segment.text.contains("中文🙂"))
                        .unwrap();
                    let Mark::TextStyle(mark) = style("red") else {
                        unreachable!()
                    };
                    assert_eq!(inserted.text_style.as_ref(), Some(mark.attributes()));
                })
                .unwrap();
            session.borrow_mut().undo().unwrap();
            assert_eq!(session.borrow().document().store(), before.store());
        }
    }
}

#[gpui::test]
fn styled_unicode_runs_reach_the_gpui_shaper_without_changing_bytes(cx: &mut TestAppContext) {
    let (window, session, _) = open(cx, false);
    let before = session.borrow().document().clone();
    window
        .update(cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
            let (text, segments) = view.layout_content();
            let base = window.text_style().font();
            let fonts = text_style::FontCatalog::from_system(window.text_system());
            let runs =
                text_style::text_runs(&segments, base, gpui::rgba(0x111111ff).into(), &fonts);
            assert_eq!(runs[0].color, gpui::rgba(0xff0000ff).into());
            assert_eq!(runs.last().unwrap().color, gpui::rgba(0x008000ff).into());
            assert!(text.contains("🙂"));
            let lines = window
                .text_system()
                .shape_text(
                    text.clone().into(),
                    gpui::px(18.0),
                    &runs,
                    Some(gpui::px(240.0)),
                    None,
                )
                .unwrap();
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.text.as_ref())
                    .collect::<String>(),
                text
            );
            assert!(
                lines
                    .iter()
                    .flat_map(|line| &line.unwrapped_layout.runs)
                    .any(|run| !run.glyphs.is_empty())
            );
            view.replace_and_mark_text_in_range(None, "", None, window, cx);
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
}

#[gpui::test]
fn inherited_basic_marks_fix_default_only_preedit_while_unmarked_input_stays_unchanged(
    cx: &mut TestAppContext,
) {
    for marks in [
        MarkSet::empty(),
        MarkSet::new([
            Mark::Bold,
            Mark::Italic,
            Mark::Underline,
            Mark::Strike,
            Mark::Code,
            Mark::Link(LinkMark::from_attributes(LinkAttributes::default())),
        ])
        .unwrap(),
    ] {
        for atoms in [false, true] {
            let (window, session, _) = open_with_marks(cx, atoms, marks.clone(), MarkSet::empty());
            let before = session.borrow().document().clone();
            let selection = session.borrow().selection();
            window
                .update(cx, |view, window, cx| {
                    view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
                    let expected = DisplaySegment::preedit(0, "中文🙂", &marks);
                    let base = gpui::font("Host UI");
                    let color = gpui::rgba(0x111111ff).into();
                    let fonts = text_style::FontCatalog::from_names(&[]);
                    let expected_runs =
                        text_style::text_runs(&[expected], base.clone(), color, &fonts);
                    for (_, segments) in [view.display_content(), view.layout_content()] {
                        let overlay = segments
                            .iter()
                            .find(|segment| segment.text == "中文🙂")
                            .unwrap();
                        assert!(overlay.text_style.is_none());
                        assert_eq!(
                            text_style::text_runs(
                                std::slice::from_ref(overlay),
                                base.clone(),
                                color,
                                &fonts
                            ),
                            expected_runs
                        );
                    }
                    view.replace_and_mark_text_in_range(None, "", None, window, cx);
                    assert_eq!(session.borrow().document().store(), before.store());
                    assert_eq!(session.borrow().selection(), selection);
                    assert_eq!(session.borrow().history_depths(), (0, 0));
                    view.replace_and_mark_text_in_range(None, "中文🙂", None, window, cx);
                    view.replace_text_in_range(None, "中文🙂", window, cx);
                    let (_, segments) = view.layout_content();
                    let mut committed = segments
                        .iter()
                        .find(|segment| segment.text.contains("中文🙂"))
                        .unwrap()
                        .clone();
                    committed.text = "中文🙂".into();
                    committed.underline = true; // account only for the transient IME decoration
                    assert_eq!(
                        text_style::text_runs(&[committed], base, color, &fonts),
                        expected_runs
                    );
                })
                .unwrap();
        }
    }
}
