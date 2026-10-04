//! Successful native edits may retain range input but change its owner node.

use super::*;

struct SwitchRange {
    edited: NodeId,
    target: DocumentSelection,
    seen: Seen,
}
impl SessionPolicy for SwitchRange {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.seen
            .borrow_mut()
            .push((context.selection(), intent.clone()));
        let text = match intent {
            EditIntent::InsertText { text } | EditIntent::CommitComposition { text, .. } => text,
            _ => return Ok(IntentDisposition::Continue),
        };
        let zero = context
            .document()
            .node(self.edited)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(0)
            .unwrap();
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::ReplaceText {
                node: self.edited,
                range: xiaomu_core::text::TextRange::new(zero, zero).unwrap(),
                replacement: text.clone(),
            },
        );
        Ok(IntentDisposition::Apply(EditPlan::new(
            transaction,
            SelectionUpdate::Exact {
                selection: self.target,
            },
            None,
        )))
    }
}

fn targets(f: &Fixture) -> Vec<(DocumentSelection, NodeId)> {
    let document = &f.document;
    let table = *f.targets.last().unwrap();
    let row = document
        .node(table)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    let cell = document.node(row).unwrap().content().as_children().unwrap()[0];
    let paragraph = document
        .node(cell)
        .unwrap()
        .content()
        .as_children()
        .unwrap()[0];
    vec![
        (DocumentSelection::node(document, f.rule).unwrap(), f.rule),
        (DocumentSelection::all(document), document.root()),
        (
            DocumentSelection::cell_range(cell, cell, InlinePoint::at_start_of(paragraph).into()),
            cell,
        ),
    ]
}

#[gpui::test]
fn node_native_commit_transfers_focus_to_new_node_all_and_cell_proxies(cx: &mut TestAppContext) {
    let f = fixture();
    for composing in [false, true] {
        for (target, anchor) in targets(&f) {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let (handle, session, counts) = open(
                cx,
                &f,
                Some(Box::new(SwitchRange {
                    edited: f.intro,
                    target,
                    seen: seen.clone(),
                })),
            );
            select(cx, handle, f.quote);
            let before = session.borrow().selection();
            let old = proxy(cx, handle);
            counts.set((0, 0));
            handle
                .update(cx, |_, window, cx| {
                    old.update(cx, |input, cx| {
                        if composing {
                            input.replace_and_mark_text_in_range(None, "你", None, window, cx);
                        }
                        input.replace_text_in_range(None, "你", window, cx);
                        input.unmark_text(window, cx);
                    })
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            assert_eq!(session.borrow().selection(), target);
            assert_eq!(session.borrow().history_depths(), (1, 0));
            assert_eq!(counts.get().0, 1);
            assert_eq!(seen.borrow().len(), 1);
            assert_eq!(seen.borrow()[0].0, before);
            handle
                .update(cx, |view, window, cx| {
                    let (actual_anchor, input) = view.range_input.as_ref().unwrap();
                    assert_eq!(*actual_anchor, anchor);
                    assert_ne!(input.entity_id(), old.entity_id());
                    assert!(view.range_input_is_focused(window, cx));
                })
                .unwrap();
            // A subsequent real platform input must reach the replacement
            // proxy, with the new range identity intact at policy preflight.
            cx.simulate_input(handle.into(), "!");
            cx.background_executor.run_until_parked();
            assert_eq!(seen.borrow().len(), 2);
            assert_eq!(seen.borrow()[1].0, target);
            assert_eq!(session.borrow().history_depths(), (2, 0));
            assert_eq!(
                session
                    .borrow()
                    .document()
                    .node(f.intro)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .runs()
                    .iter()
                    .map(|run| run.text().as_str())
                    .collect::<String>(),
                "!你intro"
            );
        }
    }
}

#[gpui::test]
fn background_node_to_node_all_and_cell_proxy_changes_do_not_take_another_panes_focus(
    cx: &mut TestAppContext,
) {
    struct Panes {
        a: Entity<DocumentView>,
        b: Entity<DocumentView>,
    }
    impl gpui::Render for Panes {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div()
                .flex()
                .size_full()
                .child(div().flex_1().min_w_0().h_full().child(self.a.clone()))
                .child(div().flex_1().min_w_0().h_full().child(self.b.clone()))
        }
    }
    let f = scroll::tall_fixture();
    for (target, anchor) in targets(&f) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let initial = DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro));
        let a = EditorInstance::new_with_policy(
            f.document.clone(),
            initial,
            EditorHooks::default(),
            Box::new(SwitchRange {
                edited: f.intro,
                target,
                seen: seen.clone(),
            }),
        )
        .unwrap();
        let b = EditorInstance::new(f.document.clone(), initial, EditorHooks::default()).unwrap();
        let session_a = a.session().clone();
        let session_b = b.session().clone();
        cx.update(bind_default_editor_keys);
        let handle = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                let a = cx.new(|_| a.build_view());
                let b = cx.new(|_| b.build_view());
                cx.new(|_| Panes { a, b })
            })
            .unwrap()
        });
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |panes, window, cx| {
                window.activate_window();
                panes.a.update(cx, |view, cx| {
                    view.select_node(f.quote, window, cx).unwrap();
                });
                panes
                    .b
                    .update(cx, |view, cx| view.focus_selection(window, cx));
                let old = panes.a.read(cx).range_input.as_ref().unwrap().1.clone();
                old.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "late", window, cx)
                });
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |panes, window, cx| {
                assert_eq!(panes.a.read(cx).range_input.as_ref().unwrap().0, anchor);
                assert!(!panes.a.read(cx).range_input_is_focused(window, cx));
                assert_eq!(panes.a.read(cx).scroll_handle.offset().y, px(0.0));
                assert!(
                    panes.b.read(cx).children[0]
                        .1
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                );
            })
            .unwrap();
        assert_eq!(session_a.borrow().selection(), target);
        assert_eq!(session_a.borrow().history_depths(), (1, 0));
        assert_eq!(session_b.borrow().selection(), initial);
        assert_eq!(session_b.borrow().history_depths(), (0, 0));
        cx.simulate_input(handle.into(), "B");
        cx.background_executor.run_until_parked();
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(session_a.borrow().history_depths(), (1, 0));
        assert_eq!(session_b.borrow().history_depths(), (1, 0));
    }
}
