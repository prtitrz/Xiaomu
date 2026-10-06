//! Virtual fail-closed IME and marked hard-break production integration.
//! NoopTextSystem coverage does not certify native shaping or OS IME behavior.
use super::*;
use crate::document_view::{EditorRejection, EditorRejectionReason, EditorRejectionStage};
use xiaomu_core::document::{AtomKind, InlineAtomContent, InlineAtomPlacement};

#[gpui::test]
fn unsupported_mixed_script_and_grapheme_preedit_cannot_commit_or_replace_canonical_text(
    cx: &mut TestAppContext,
) {
    for preedit in ["אב", "عربي", "\u{301}"] {
        for (select_suffix, end_with_unmark) in [(false, false), (false, true), (true, false)] {
            if select_suffix && preedit == "\u{301}" {
                // Replacement inherits the preceding 48px z; its combining
                // continuation is valid. The collapsed case tests a size seam.
                continue;
            }
            let canonical = if select_suffix { "ezz" } else { "ez" };
            let suffix = if select_suffix { "zz" } else { "z" };
            let (handle, session, node) = open(
                cx,
                &[("e", "12px"), (suffix, "48px")],
                BlockAlignment::Center,
                200.0,
                false,
            );
            set_caret(&session, node, 1, CursorAffinity::Before);
            session
                .borrow_mut()
                .apply_intent(&EditIntent::SetMark {
                    mark: size_mark("48px"),
                })
                .unwrap();
            if select_suffix {
                set_caret(&session, node, 2, CursorAffinity::Before);
                let start = session
                    .borrow()
                    .selection()
                    .as_same_node_inline()
                    .unwrap()
                    .1;
                set_caret(&session, node, 3, CursorAffinity::Before);
                let end = session
                    .borrow()
                    .selection()
                    .as_same_node_inline()
                    .unwrap()
                    .1;
                session
                    .borrow_mut()
                    .set_inline_selection(start, end)
                    .unwrap();
            }
            let before = session.borrow().document().clone();
            let selection = session.borrow().selection();
            let marks = session.borrow().stored_marks().cloned();
            let entity = handle.update(cx, |host, _, _| host.input.clone()).unwrap();
            let events = Rc::new(RefCell::new(Vec::new()));
            let seen = events.clone();
            let _subscription = cx.update(|cx| {
                cx.subscribe(&entity, move |_, event: &EditorRejection, _| {
                    seen.borrow_mut().push(*event);
                })
            });
            handle
                .update(cx, |host, window, cx| {
                    host.input.update(cx, |view, cx| {
                        view.replace_and_mark_text_in_range(None, preedit, None, window, cx);
                        assert!(view.rejected_composition);
                        assert!(view.composition.is_none());
                        assert_eq!(view.display_content().0, canonical);
                        assert_eq!(view.marked_text_range(window, cx), None);
                        // The remainder of the rejected native session must be consumed.
                        view.replace_and_mark_text_in_range(
                            None,
                            "accepted-looking",
                            None,
                            window,
                            cx,
                        );
                        assert!(view.rejected_composition);
                        if end_with_unmark {
                            view.unmark_text(window, cx);
                        } else {
                            view.replace_text_in_range(None, preedit, window, cx);
                        }
                        assert!(!view.is_composing());
                        assert_eq!(view.canonical_text(), canonical);
                    })
                })
                .unwrap();
            assert_eq!(session.borrow().document().store(), before.store());
            cx.background_executor.run_until_parked();
            let events = events.borrow();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].stage(), EditorRejectionStage::TextSizePreedit);
            assert_eq!(
                events[0].reason(),
                EditorRejectionReason::UnsupportedTextSize
            );
            assert_eq!(events[0].document_revision(), before.revision());
            assert_eq!(session.borrow().document().revision(), before.revision());
            assert_eq!(session.borrow().selection(), selection);
            assert_eq!(session.borrow().stored_marks(), marks.as_ref());
            assert_eq!(session.borrow().history_depths(), (0, 0));
        }
    }
}
#[gpui::test]
fn marked_hard_break_size_sets_preceding_row_without_changing_canonical_offsets(
    cx: &mut TestAppContext,
) {
    let mut builder = NodeStoreBuilder::new();
    let inline = InlineContent::new([TextRun::new("AB", MarkSet::empty()).unwrap()]).unwrap();
    let offset = inline.offset_at(1).unwrap();
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break()
                    .with_marks(MarkSet::new([size_mark("48px")]).unwrap()),
            ),
        )
        .unwrap();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    inline.runs().iter().cloned(),
                    [InlineAtomPlacement::new(atom, offset)],
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
    let (handle, session, _) = mount(
        cx,
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        node,
        BlockAlignment::Center,
        200.0,
        false,
    );
    let before = session.borrow().document().clone();
    handle
        .update(cx, |host, window, cx| {
            host.input.update(cx, |view, cx| {
                assert_eq!(view.canonical_text(), "AB");
                assert_eq!(view.layout_content().0, "A\nB");
                let layout = view.last_layout.as_ref().unwrap();
                let first = layout
                    .caret_rect(1, CursorAffinity::Before, px(1.0))
                    .unwrap();
                let next = layout
                    .caret_rect(2, CursorAffinity::Before, px(1.0))
                    .unwrap();
                near(first.size.height, px(48.0 * 1.4));
                near(next.size.height, px(28.0));
                near(next.top(), first.bottom());
                let newline = layout.selection_rects(1..2);
                assert_eq!(newline.len(), 1);
                near(newline[0].size.height, first.size.height);
                session
                    .borrow_mut()
                    .set_document_selection(DocumentSelection::collapsed(InlinePoint::new(
                        node,
                        offset,
                        1,
                        CursorAffinity::Before,
                    )))
                    .unwrap();
                assert_eq!(
                    view.selected_text_range(true, window, cx).unwrap().range,
                    1..1
                );
                let caret = native_caret(view, window, cx);
                near(caret.top(), view.last_bounds.unwrap().top() + next.top());
                near(caret.size.height, next.size.height);
            })
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), before.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
