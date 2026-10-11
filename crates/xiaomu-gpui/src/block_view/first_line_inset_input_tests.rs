//! Mounted production-provider/element tests using GPUI's NoopTextSystem.
//! These are engine geometry contracts, not native font or browser-caret proof.
use super::*;
use crate::{
    block_alignment::BlockAlignment,
    first_line_inset::FirstLineInsetProvider,
    font_size::FontSizeContext,
    input::utf16,
    text_size::{TextSizeCapability, TextSizeStyle, TextSizeStyleProvider},
};
use gpui::{Entity, EntityInputHandler, TestAppContext, WindowHandle, font, point, px};
use xiaomu_core::{
    document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, Mark, MarkSet, Node, NodeAttrs,
        NodeContent, NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes,
        TextStyleMark, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
};
use xiaomu_runtime::session::{DocumentSelection, PolicyError, SessionPolicy};

struct FixedStyle(f32);
impl TextSizeStyleProvider for FixedStyle {
    fn style(&self, _: &XiaomuDocument, _: &Node) -> TextSizeStyle {
        TextSizeStyle::new(
            font(".SystemUIFont"),
            FontSizeContext::new(self.0, self.0, self.0).unwrap(),
            1.4,
        )
    }
}
struct FixedInset(f32);
impl FirstLineInsetProvider for FixedInset {
    fn first_line_inset(&self, _: &XiaomuDocument, _: &Node, _: f32) -> f32 {
        self.0
    }
}
struct Policy(Rc<TextSizeCapability>);
impl SessionPolicy for Policy {
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        self.0
            .validate_document(document, &InlineAtomRendererRegistry::default())
            .map_err(|_| PolicyError::new("unsupported inset candidate"))
    }
}
struct Host {
    input: Entity<ParagraphView>,
    width: Pixels,
}
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w_full().child(
            div()
                .ml(px(17.))
                .mt(px(23.))
                .w(self.width)
                .font_family(".SystemUIFont")
                .text_size(px(18.))
                .child(self.input.clone()),
        )
    }
}
fn mark(size: &str) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default().with_font_size(StringAttribute::Value(size.into())),
    ))
}
fn document(parts: &[(&str, Option<&str>)], hard_break: Option<usize>) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if parts.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new(parts.iter().map(|(text, size)| {
            TextRun::new(
                *text,
                size.map_or_else(MarkSet::empty, |size| MarkSet::new([mark(size)]).unwrap()),
            )
            .unwrap()
        }))
        .unwrap()
    };
    let inline = if let Some(byte) = hard_break {
        let atom = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::hard_break()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(InlineAtomContent::hard_break()),
            )
            .unwrap();
        InlineContent::with_atoms(
            inline.runs().iter().cloned(),
            [InlineAtomPlacement::new(
                atom,
                inline.offset_at(byte).unwrap(),
            )],
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
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}
fn mount(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    node: NodeId,
    width: f32,
    inset: Option<f32>,
    alignment: BlockAlignment,
) -> (WindowHandle<Host>, SharedSession) {
    let mut saved = None;
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let mut capability =
                TextSizeCapability::new(window.text_system().clone(), Rc::new(FixedStyle(18.)));
            if let Some(inset) = inset {
                capability = capability.with_first_line_inset_provider(Rc::new(FixedInset(inset)));
            }
            let capability = Rc::new(capability);
            capability
                .validate_document(&document, &InlineAtomRendererRegistry::default())
                .unwrap();
            let session = Rc::new(RefCell::new(
                DocumentSession::new_with_policy(
                    document,
                    DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
                    Box::new(Policy(capability.clone())),
                )
                .unwrap(),
            ));
            saved = Some(session.clone());
            let input = cx.new(|cx| {
                let mut view = ParagraphView::new(
                    session,
                    Rc::new(Cell::new(0)),
                    Rc::new(RefCell::new(Vec::new())),
                    node,
                    cx,
                );
                view.set_block_alignment(Some(alignment));
                view.attach_text_size_capability(Some(capability));
                window.focus(&view.focus_handle);
                view
            });
            cx.new(|_| Host {
                input,
                width: px(width),
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, saved.unwrap())
}
fn near(actual: Pixels, expected: f32) {
    assert!(
        (f32::from(actual) - expected).abs() < 0.02,
        "{actual:?} != {expected}"
    );
}
fn unchanged(session: &SharedSession, document: &XiaomuDocument, selection: DocumentSelection) {
    let session = session.borrow();
    assert_eq!(session.document().store(), document.store());
    assert_eq!(session.document().revision(), document.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (0, 0));
}

#[gpui::test]
fn positive_inset_first_cluster_overflows_without_an_empty_first_row(cx: &mut TestAppContext) {
    // R2 original observations establish x=36 and no generated empty first row,
    // including width <= inset. The native glyph widths are deliberately not asserted.
    for text in ["ABCDEF", "中文测试", "👨‍👩‍👧‍👦🙂尾", "e\u{301}abc"] {
        for width in [1., 20., 35., 36., 40., 60.] {
            let (document, node) = document(&[(text, None)], None);
            let before = document.clone();
            let (handle, session) =
                mount(cx, document, node, width, Some(36.), BlockAlignment::Left);
            let selection = session.borrow().selection();
            handle
                .update(cx, |host, _, cx| {
                    let view = host.input.read(cx);
                    let layout = view.last_layout.as_ref().expect("mounted inset layout");
                    let first = layout
                        .position_for_caret(0, CursorAffinity::Before)
                        .unwrap();
                    near(first.x, 36.);
                    near(first.y, 0.);
                    let boundary = layout
                        .visual_line_edge(0, CursorAffinity::Before, true)
                        .unwrap()
                        .0;
                    assert!(
                        boundary > 0,
                        "first row must contain a cluster at width {width}: {text}"
                    );
                    if text.starts_with("👨") {
                        assert!(boundary >= "👨‍👩‍👧‍👦".len());
                    }
                    if text.starts_with('e') {
                        assert!(boundary >= "e\u{301}".len());
                    }
                    if boundary < text.len() {
                        let next = layout
                            .position_for_caret(boundary, CursorAffinity::After)
                            .unwrap();
                        near(next.x, 0.);
                        assert!(next.y > first.y);
                    }
                    let rects = layout.selection_rects(0..boundary);
                    assert!(!rects.is_empty());
                    near(rects[0].origin.x, 36.);
                    near(rects[0].origin.y, 0.);
                    assert_eq!(view.canonical_text(), text);
                })
                .unwrap();
            unchanged(&session, &before, selection);
        }
    }
}

#[gpui::test]
fn inset_is_consumed_once_across_soft_wrap_and_canonical_hard_break(cx: &mut TestAppContext) {
    for parts in [
        vec![("Alpha beta gamma delta end", None)],
        vec![
            ("Alpha ", Some("12px")),
            ("beta gamma delta end", Some("36px")),
        ],
    ] {
        for hard_break in [None, Some(0), Some(6)] {
            let (document, node) = document(&parts, hard_break);
            let before = document.clone();
            let (handle, session) = mount(cx, document, node, 95., Some(36.), BlockAlignment::Left);
            let selection = session.borrow().selection();
            handle
                .update(cx, |host, _, cx| {
                    let view = host.input.read(cx);
                    let text = view.layout_content().0;
                    let layout = view.last_layout.as_ref().unwrap();
                    near(
                        layout
                            .position_for_caret(0, CursorAffinity::Before)
                            .unwrap()
                            .x,
                        36.,
                    );
                    let mut starts = vec![0];
                    for byte in text
                        .char_indices()
                        .map(|(byte, _)| byte)
                        .chain([text.len()])
                    {
                        if layout.is_soft_wrap_boundary(byte) {
                            starts.push(byte);
                        }
                    }
                    for (byte, _) in text.match_indices('\n') {
                        starts.push(byte + 1);
                    }
                    assert!(starts.len() > 1);
                    for byte in starts.into_iter().filter(|byte| *byte != 0) {
                        let p = layout
                            .position_for_caret(byte, CursorAffinity::After)
                            .unwrap();
                        near(p.x, 0.);
                        assert!(p.y > px(0.));
                    }
                    if hard_break == Some(0) {
                        assert!(text.starts_with('\n'));
                        near(
                            layout
                                .position_for_caret(1, CursorAffinity::Before)
                                .unwrap()
                                .x,
                            0.,
                        );
                    }
                })
                .unwrap();
            unchanged(&session, &before, selection);
        }
    }
}

#[gpui::test]
fn inset_composes_with_alignment_and_selection_in_the_remaining_first_row_box(
    cx: &mut TestAppContext,
) {
    for alignment in [
        BlockAlignment::Left,
        BlockAlignment::Center,
        BlockAlignment::Right,
    ] {
        let (document, node) = document(&[("AB\nCD", None)], None);
        let (handle, _) = mount(cx, document, node, 200.25, Some(36.), alignment);
        handle
            .update(cx, |host, _, cx| {
                let view = host.input.read(cx);
                let layout = view.last_layout.as_ref().unwrap();
                let first = layout
                    .position_for_caret(0, CursorAffinity::Before)
                    .unwrap();
                let first_end = layout
                    .position_for_caret(2, CursorAffinity::Before)
                    .unwrap();
                let next = layout
                    .position_for_caret(3, CursorAffinity::Before)
                    .unwrap();
                let next_end = layout
                    .position_for_caret(5, CursorAffinity::Before)
                    .unwrap();
                let width = f32::from(view.last_bounds.unwrap().size.width);
                match alignment {
                    BlockAlignment::Left => {
                        near(first.x, 36.);
                        near(next.x, 0.);
                    }
                    BlockAlignment::Center => {
                        near(first.x + first_end.x, width + 36.);
                        near(next.x + next_end.x, width);
                    }
                    BlockAlignment::Right => {
                        near(first_end.x, width);
                        near(next_end.x, width);
                    }
                }
                let repeated = layout
                    .clone()
                    .with_alignment(Some(alignment), view.last_bounds.unwrap().size.width)
                    .with_alignment(Some(alignment), view.last_bounds.unwrap().size.width);
                assert_eq!(
                    repeated.position_for_caret(0, CursorAffinity::Before),
                    Some(first)
                );
                assert_eq!(
                    repeated.position_for_caret(3, CursorAffinity::Before),
                    Some(next)
                );
                assert_eq!(
                    repeated.selection_rects(0..5),
                    layout.selection_rects(0..5),
                    "request/prepaint alignment must not accumulate the inset"
                );
                let rects = layout.selection_rects(0..2);
                assert_eq!(rects.len(), 1);
                assert_eq!(rects[0].origin, first);
                near(rects[0].size.width, f32::from(first_end.x - first.x));
                assert_eq!(
                    layout.caret_for_position(first + point(px(0.), px(1.))),
                    (0, CursorAffinity::After)
                );
                assert_eq!(
                    layout.caret_for_position(next + point(px(0.), px(1.))),
                    (3, CursorAffinity::After)
                );
            })
            .unwrap();
    }
}

#[gpui::test]
fn native_utf16_caret_hit_and_ime_bounds_follow_inset_rows_without_text_mutation(
    cx: &mut TestAppContext,
) {
    let text = "A🙂中 e\u{301} Z more text to wrap";
    let (document, node) = document(&[(text, None)], None);
    let before = document.clone();
    let (handle, session) = mount(cx, document, node, 95., Some(36.), BlockAlignment::Left);
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                for byte in [0, 1, 5, 8, text.len()] {
                    let offset = view.inline().unwrap().offset_at(byte).unwrap();
                    session
                        .borrow_mut()
                        .set_document_selection(DocumentSelection::collapsed(InlinePoint::new(
                            node,
                            offset,
                            0,
                            CursorAffinity::Before,
                        )))
                        .unwrap();
                    let utf = utf16::utf16_offset(text, byte);
                    assert_eq!(
                        view.selected_text_range(true, window, cx).unwrap().range,
                        utf..utf
                    );
                    let expected = view
                        .last_layout
                        .as_ref()
                        .unwrap()
                        .position_for_caret(byte, CursorAffinity::Before)
                        .unwrap();
                    let bounds = view.last_bounds.unwrap();
                    let native = view.bounds_for_range(utf..utf, bounds, window, cx).unwrap();
                    assert_eq!(native.origin, bounds.origin + expected);
                    assert_eq!(
                        view.character_index_for_point(
                            native.origin + point(px(0.), px(1.)),
                            window,
                            cx
                        ),
                        Some(utf)
                    );
                }
                session
                    .borrow_mut()
                    .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                        node,
                    )))
                    .unwrap();
                view.replace_and_mark_text_in_range(None, "中文🙂", Some(4..4), window, cx);
                assert!(view.is_composing());
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                assert_eq!(view.marked_text_range(window, cx), Some(0..4));
                assert_eq!(session.borrow().document().store(), before.store());
                let layout = view.last_layout.as_ref().unwrap();
                near(
                    layout
                        .position_for_caret(0, CursorAffinity::Before)
                        .unwrap()
                        .x,
                    36.,
                );
                let byte = view.composing_caret_byte().unwrap();
                let expected = layout
                    .position_for_caret(byte, CursorAffinity::Before)
                    .unwrap();
                let bounds = view.last_bounds.unwrap();
                let native = view.bounds_for_range(4..4, bounds, window, cx).unwrap();
                assert_eq!(native.origin, bounds.origin + expected);
                view.replace_and_mark_text_in_range(None, "", None, window, cx);
                view.unmark_text(window, cx);
            })
        })
        .unwrap();
    unchanged(
        &session,
        &before,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
    );
}

#[gpui::test]
fn positive_inset_rejects_unsupported_preedit_and_committed_candidates_atomically(
    cx: &mut TestAppContext,
) {
    for text in ["אב", "عربي"] {
        let (document, node) = document(&[("safe", None)], None);
        let before = document.clone();
        let (handle, session) = mount(cx, document, node, 120., Some(36.), BlockAlignment::Left);
        let selection = session.borrow().selection();
        let marks = session.borrow().stored_marks().cloned();
        assert!(
            session
                .borrow_mut()
                .apply_intent(&EditIntent::InsertText { text: text.into() })
                .is_err()
        );
        unchanged(&session, &before, selection);
        handle
            .update(cx, |host, window, cx| {
                host.input.update(cx, |view, cx| {
                    view.replace_and_mark_text_in_range(None, text, None, window, cx);
                    assert!(view.rejected_composition);
                    // A rejected native session remains active until its terminal event.
                    assert!(view.composition.is_none());
                    assert!(view.is_composing());
                    assert_eq!(view.marked_text_range(window, cx), None);
                    assert_eq!(view.canonical_text(), "safe");
                    view.replace_text_in_range(None, text, window, cx);
                    assert!(!view.is_composing());
                })
            })
            .unwrap();
        unchanged(&session, &before, selection);
        assert_eq!(session.borrow().stored_marks(), marks.as_ref());
    }
}

#[path = "first_line_inset_history_tests.rs"]
mod additional;
