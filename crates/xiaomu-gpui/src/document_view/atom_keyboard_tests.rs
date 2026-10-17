//! Real focus traversal and input dispatch; no direct chip focus for reachability.
use super::*;
use gpui::{
    Context, Entity, EntityInputHandler, FocusHandle, IntoElement, Render, Window, div, prelude::*,
};

gpui::actions!(atom_keyboard_test, [HostShortcut]);

struct SurroundingControls {
    editor: Entity<DocumentView>,
    second_editor: Option<Entity<DocumentView>>,
    before: FocusHandle,
    after: FocusHandle,
    show_editor: bool,
    shortcut_hits: Rc<std::cell::Cell<usize>>,
}

impl Render for SurroundingControls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let control = |handle: &FocusHandle| {
            div()
                .track_focus(handle)
                .h(px(24.))
                .child("outside control")
                .on_key_down(|event, window, cx| {
                    if event.keystroke.key == "tab" {
                        if event.keystroke.modifiers.shift {
                            window.focus_prev();
                        } else {
                            window.focus_next();
                        }
                        cx.stop_propagation();
                    }
                })
        };
        let shortcut_hits = self.shortcut_hits.clone();
        div()
            .on_action(move |_: &HostShortcut, _, _| shortcut_hits.set(shortcut_hits.get() + 1))
            .size_full()
            .flex()
            .flex_col()
            .child(control(&self.before))
            .when(self.show_editor, |container| {
                container.child(div().flex_1().min_h_0().child(self.editor.clone()))
            })
            .when_some(self.second_editor.clone(), |container, editor| {
                container.child(div().flex_1().min_h_0().child(editor))
            })
            .child(control(&self.after))
    }
}

fn keyboard_host() -> Rc<Host> {
    Rc::new(Host {
        keyboard: true,
        capture: true,
        ..Default::default()
    })
}

fn mount(
    fixture: &Fixture,
    host: SharedAtomCapability,
    cx: &mut TestAppContext,
) -> (
    WindowHandle<SurroundingControls>,
    Entity<DocumentView>,
    SharedSession,
) {
    let editor = EditorInstance::new(
        fixture.document.clone(),
        fixture.selection,
        EditorHooks {
            atom_capability: Some(host),
            ..Default::default()
        },
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        bind_default_editor_keys(cx);
        cx.bind_keys([gpui::KeyBinding::new("f8", HostShortcut, None)]);
        cx.open_window(Default::default(), |_, cx| {
            let editor = cx.new(|_| editor.build_view());
            cx.new(|cx| SurroundingControls {
                editor,
                second_editor: None,
                before: cx.focus_handle().tab_stop(true),
                after: cx.focus_handle().tab_stop(true),
                show_editor: true,
                shortcut_hits: Default::default(),
            })
        })
        .unwrap()
    });
    let view = handle
        .update(cx, |frame, window, _| {
            window.activate_window();
            window.focus(&frame.before);
            frame.editor.clone()
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, view, session)
}

fn assert_chip(handle: WindowHandle<SurroundingControls>, atom: NodeId, cx: &mut TestAppContext) {
    handle
        .update(cx, |frame, window, cx| {
            let view = frame.editor.read(cx);
            assert!(
                view.atom_keyboard.borrow().entries[&atom]
                    .handle
                    .is_focused(window)
            );
            assert!(view.focused_child(window, cx).is_none());
        })
        .unwrap();
}

#[gpui::test]
fn atom_keyboard_actual_tab_enters_duplicates_and_exits_both_ends(cx: &mut TestAppContext) {
    let f = fixture();
    let host = keyboard_host();
    let (handle, _, session) = mount(&f, host.clone(), cx);
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[0], cx);
    cx.simulate_keystrokes(handle.into(), "enter ctrl-shift-space");
    assert_eq!(host.keys.borrow().len(), 2);
    assert_eq!(host.keys.borrow()[0].0.node, f.atoms[0]);
    assert!(host.keys.borrow()[1].1.control && host.keys.borrow()[1].1.shift);
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[1], cx);
    cx.simulate_keystrokes(handle.into(), "space tab");
    handle
        .update(cx, |frame, window, _| {
            assert!(frame.after.is_focused(window))
        })
        .unwrap();
    assert_eq!(host.keys.borrow()[2].0.node, f.atoms[1]);
    cx.simulate_keystrokes(handle.into(), "shift-tab shift-tab");
    assert_chip(handle, f.atoms[0], cx);
    cx.simulate_keystrokes(handle.into(), "shift-tab");
    handle
        .update(cx, |frame, window, _| {
            assert!(frame.before.is_focused(window))
        })
        .unwrap();
    unchanged(&f, &session);
}

#[gpui::test]
fn atom_keyboard_never_types_or_pastes_into_retained_caret(cx: &mut TestAppContext) {
    let f = fixture();
    let host = keyboard_host();
    let (handle, view, session) = mount(&f, host, cx);
    // Materialize the paragraph's normal platform input before moving to a chip.
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.focus_selection(window, cx))
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |frame, window, _| window.focus(&frame.before))
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[0], cx);
    cx.simulate_input(handle.into(), "abc");
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("must not paste".into())));
    cx.simulate_keystrokes(handle.into(), "backspace delete ctrl-v ctrl-x ctrl-z");
    cx.dispatch_action(handle.into(), crate::block_view::ClipboardPaste);
    cx.dispatch_action(handle.into(), crate::block_view::ClipboardCut);
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                let input = view
                    .children
                    .iter()
                    .find(|(id, _)| *id == f.selected)
                    .unwrap()
                    .1
                    .clone();
                input.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "late platform text", window, cx);
                    input.replace_and_mark_text_in_range(None, "预编辑", None, window, cx);
                    input.unmark_text(window, cx);
                    assert!(!input.is_composing());
                });
                view.paste(&crate::block_view::ClipboardPaste, window, cx);
                view.cut(&crate::block_view::ClipboardCut, window, cx);
            })
        })
        .unwrap();
    unchanged(&f, &session);
}

#[gpui::test]
fn atom_keyboard_plain_caret_enter_space_tab_keep_editing(cx: &mut TestAppContext) {
    for key in ["enter", "space", "tab"] {
        let f = fixture();
        let host = keyboard_host();
        let (handle, view, session) = mount(&f, host.clone(), cx);
        handle
            .update(cx, |_, window, cx| {
                view.update(cx, |view, cx| view.focus_selection(window, cx))
            })
            .unwrap();
        cx.simulate_keystrokes(handle.into(), key);
        assert!(host.keys.borrow().is_empty());
        assert_ne!(
            session.borrow().document().revision(),
            f.document.revision()
        );
    }
}

#[gpui::test]
fn atom_keyboard_removed_replaced_or_unmounted_chips_cannot_activate(cx: &mut TestAppContext) {
    for change in 0..4 {
        let f = fixture();
        let host = keyboard_host();
        let (handle, view, session) = mount(&f, host.clone(), cx);
        cx.simulate_keystrokes(handle.into(), "tab");
        assert_chip(handle, f.atoms[0], cx);
        handle
            .update(cx, |frame, _, cx| {
                match change {
                    0 => {
                        session
                            .borrow_mut()
                            .apply(
                                &Transaction::new(TransactionOrigin::System).with_step(
                                    TransactionStep::RemoveInlineAtom { atom: f.atoms[0] },
                                ),
                            )
                            .unwrap();
                    }
                    1 => {
                        session
                            .borrow_mut()
                            .apply(&Transaction::new(TransactionOrigin::System).with_step(
                                TransactionStep::SetNodeAttrs {
                                    node: f.atoms[0],
                                    attrs: NodeAttrs::empty(),
                                },
                            ))
                            .unwrap();
                    }
                    2 => view.update(cx, |view, _| view.set_atom_capability(keyboard_host())),
                    _ => {
                        frame.show_editor = false;
                        cx.notify();
                    }
                }
                // Deliberately no editor notify for mutation/capability cases:
                // retained painted handlers must revalidate before activation.
            })
            .unwrap();
        let before = {
            let live = session.borrow();
            (
                live.document().clone(),
                live.selection(),
                live.history_depths(),
            )
        };
        cx.simulate_keystrokes(handle.into(), "enter space");
        assert!(host.keys.borrow().is_empty());
        let live = session.borrow();
        assert_eq!(live.document().store(), before.0.store());
        assert_eq!(live.selection(), before.1);
        assert_eq!(live.history_depths(), before.2);
    }
}

#[gpui::test]
fn atom_keyboard_same_node_ids_in_other_window_do_not_steal_keys(cx: &mut TestAppContext) {
    let f = fixture();
    let first = keyboard_host();
    let second = keyboard_host();
    let (one, _, _) = mount(&f, first.clone(), cx);
    let (two, _, _) = mount(&f, second.clone(), cx);
    cx.simulate_keystrokes(one.into(), "tab enter");
    assert_eq!(first.keys.borrow().len(), 1);
    assert!(second.keys.borrow().is_empty());
    cx.simulate_keystrokes(two.into(), "tab space");
    assert_eq!(first.keys.borrow().len(), 1);
    assert_eq!(second.keys.borrow().len(), 1);
}

#[gpui::test]
fn atom_keyboard_default_off_keeps_outside_traversal_and_pointer_contract(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, view, _) = mount(&f, host.clone(), cx);
    cx.simulate_keystrokes(handle.into(), "tab");
    handle
        .update(cx, |frame, window, cx| {
            assert!(frame.after.is_focused(window));
            assert!(view.read(cx).atom_keyboard.borrow().entries.is_empty());
        })
        .unwrap();
    let pointer_host = keyboard_host();
    let (pointer_window, session) = open(&f, Some(pointer_host.clone()), cx);
    let point = chip_point(pointer_window, f.paragraph, f.atoms[0], cx);
    down(pointer_window, point, Modifiers::default(), cx);
    up(pointer_window, point, Modifiers::default(), cx);
    unchanged(&f, &session);
    assert_focus(pointer_window, f.selected, cx);
    assert_eq!(pointer_host.clicks.borrow().len(), 1);
    assert!(pointer_host.keys.borrow().is_empty());
}

#[gpui::test]
fn atom_keyboard_two_instances_in_one_window_keep_node_identity_local(cx: &mut TestAppContext) {
    let f = fixture();
    let first = keyboard_host();
    let second = keyboard_host();
    let (handle, _, _) = mount(&f, first.clone(), cx);
    let other = EditorInstance::new(
        f.document.clone(),
        f.selection,
        EditorHooks {
            atom_capability: Some(second.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    handle
        .update(cx, |frame, _, cx| {
            frame.second_editor = Some(cx.new(|_| other.build_view()));
            cx.notify();
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "tab enter tab");
    assert_chip(handle, f.atoms[1], cx);
    cx.simulate_keystrokes(handle.into(), "tab");
    handle
        .update(cx, |frame, window, cx| {
            let second_view = frame.second_editor.as_ref().unwrap().read(cx);
            assert!(
                second_view.atom_keyboard.borrow().entries[&f.atoms[0]]
                    .handle
                    .is_focused(window)
            );
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "space shift-tab");
    assert_chip(handle, f.atoms[1], cx);
    cx.simulate_keystrokes(handle.into(), "shift-tab");
    assert_chip(handle, f.atoms[0], cx);
    assert_eq!(first.keys.borrow().len(), 1);
    assert_eq!(second.keys.borrow().len(), 1);
    assert_eq!(
        first.keys.borrow()[0].0.node,
        second.keys.borrow()[0].0.node
    );
}

struct MutatingEligibility {
    inner: Host,
    source: RefCell<Option<SharedSession>>,
    armed: std::cell::Cell<bool>,
}

impl InlineAtomHostCapability for MutatingEligibility {
    fn atom_action(&self, action: AtomAction) {
        self.inner.atom_action(action);
    }
    fn keyboard_focusable(&self, action: &AtomAction) -> bool {
        if self.armed.replace(false) {
            self.source
                .borrow()
                .as_ref()
                .unwrap()
                .borrow_mut()
                .apply(&Transaction::new(TransactionOrigin::System).with_step(
                    TransactionStep::SetNodeAttrs {
                        node: action.node,
                        attrs: NodeAttrs::empty(),
                    },
                ))
                .unwrap();
        }
        true
    }
    fn atom_keyboard_activate(&self, action: AtomAction, modifiers: Modifiers) {
        self.inner.atom_keyboard_activate(action, modifiers);
    }
}

#[gpui::test]
fn atom_keyboard_eligibility_callback_cannot_activate_a_changed_snapshot(cx: &mut TestAppContext) {
    let f = fixture();
    let host = Rc::new(MutatingEligibility {
        inner: Host::default(),
        source: RefCell::new(None),
        armed: std::cell::Cell::new(false),
    });
    let (handle, _, session) = mount(&f, host.clone(), cx);
    *host.source.borrow_mut() = Some(session);
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[0], cx);
    host.armed.set(true);
    cx.simulate_keystrokes(handle.into(), "enter");
    assert!(host.inner.keys.borrow().is_empty());
}

#[gpui::test]
fn atom_keyboard_nested_table_admission_revoked_before_repaint_refuses_activation(
    cx: &mut TestAppContext,
) {
    for metadata in [false, true] {
        let (f, _) = super::geometry_tests::geometry_fixture(false);
        let host = keyboard_host();
        let (handle, view, _) = mount(&f, host.clone(), cx);
        handle
            .update(cx, |_, _, cx| {
                view.update(cx, |view, cx| {
                    view.set_measured_table_layout(true);
                    cx.notify();
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        cx.simulate_keystrokes(handle.into(), "tab");
        assert_chip(handle, f.atoms[0], cx);
        handle
            .update(cx, |_, _, cx| {
                view.update(cx, |view, _| {
                    if metadata {
                        view.set_measured_table_row_metadata(NodeAttrs::empty());
                    } else {
                        view.set_measured_table_layout(true);
                    }
                })
            })
            .unwrap();
        cx.simulate_keystrokes(handle.into(), "enter");
        assert!(host.keys.borrow().is_empty());
    }
}

#[gpui::test]
fn atom_keyboard_table_caret_tab_matches_default_behavior(cx: &mut TestAppContext) {
    let (f, _) = super::geometry_tests::geometry_fixture(false);
    let mut outcomes = Vec::new();
    for keyboard in [false, true] {
        let host = Rc::new(Host {
            keyboard,
            ..Default::default()
        });
        let (handle, view, session) = mount(&f, host.clone(), cx);
        handle
            .update(cx, |_, _, cx| {
                view.update(cx, |view, cx| {
                    view.set_measured_table_layout(true);
                    cx.notify();
                })
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                view.update(cx, |view, cx| {
                    assert!(view.place(InlinePoint::at_start_of(f.paragraph), window, cx));
                })
            })
            .unwrap();
        cx.simulate_keystrokes(handle.into(), "tab");
        assert!(host.keys.borrow().is_empty());
        let live = session.borrow();
        outcomes.push((
            live.document().clone(),
            live.selection(),
            live.history_depths(),
        ));
    }
    assert_eq!(outcomes[0].0.store(), outcomes[1].0.store());
    assert_eq!(outcomes[0].1, outcomes[1].1);
    assert_eq!(outcomes[0].2, outcomes[1].2);
}

#[gpui::test]
fn atom_keyboard_existing_native_composition_finishes_like_external_focus(cx: &mut TestAppContext) {
    for completion in 0..3 {
        let mut outcomes = Vec::new();
        for chip in [false, true] {
            let f = fixture();
            let (handle, view, session) = mount(&f, keyboard_host(), cx);
            handle
                .update(cx, |frame, window, cx| {
                    view.update(cx, |view, cx| {
                        view.focus_selection(window, cx);
                        let input = view
                            .children
                            .iter()
                            .find(|(id, _)| *id == f.selected)
                            .unwrap()
                            .1
                            .clone();
                        input.update(cx, |input, cx| {
                            input.replace_and_mark_text_in_range(
                                None,
                                "预编辑🙂",
                                Some(1..2),
                                window,
                                cx,
                            );
                            assert!(input.is_composing());
                        });
                        let target = if chip {
                            view.atom_keyboard.borrow().entries[&f.atoms[0]]
                                .handle
                                .clone()
                        } else {
                            frame.before.clone()
                        };
                        window.focus(&target);
                        // Native finalization can arrive before the deferred focus-out
                        // observer. Both focus destinations must keep the old order.
                        input.update(cx, |input, cx| {
                            match completion {
                                0 => input.unmark_text(window, cx),
                                1 => input.replace_text_in_range(None, "结果", window, cx),
                                _ => input.replace_text_in_range(None, "", window, cx),
                            }
                            assert!(!input.is_composing());
                            input.unmark_text(window, cx);
                        });
                    })
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            let live = session.borrow();
            outcomes.push((
                live.document().clone(),
                live.selection(),
                live.history_depths(),
            ));
        }
        assert_eq!(outcomes[0].0.store(), outcomes[1].0.store());
        assert_eq!(outcomes[0].1, outcomes[1].1);
        assert_eq!(outcomes[0].2, outcomes[1].2);
    }
}

#[gpui::test]
fn atom_keyboard_nonmonotonic_atom_order_and_host_shortcuts(cx: &mut TestAppContext) {
    let f = fixture_with_atom_order(true);
    assert!(f.atoms[0] > f.atoms[1]);
    let host = keyboard_host();
    let (handle, _, session) = mount(&f, host.clone(), cx);
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[0], cx);
    cx.simulate_keystrokes(handle.into(), "f8 enter tab");
    assert_chip(handle, f.atoms[1], cx);
    cx.simulate_keystrokes(handle.into(), "space tab");
    handle
        .update(cx, |frame, window, _| {
            assert_eq!(frame.shortcut_hits.get(), 1);
            assert!(frame.after.is_focused(window));
        })
        .unwrap();
    assert_eq!(host.keys.borrow()[0].0.node, f.atoms[0]);
    assert_eq!(host.keys.borrow()[1].0.node, f.atoms[1]);
    unchanged(&f, &session);
}

#[gpui::test]
fn atom_keyboard_fully_clipped_chips_leave_tab_order_and_can_return(cx: &mut TestAppContext) {
    let f = fixture();
    let host = keyboard_host();
    let (handle, view, session) = mount(&f, host.clone(), cx);
    session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::System).with_step(
            TransactionStep::ReplaceText {
                node: f.selected,
                range:
                    xiaomu_core::text::TextRange::new(TextOffset::ZERO, TextOffset::ZERO).unwrap(),
                replacement: "padding line\n".repeat(200),
            },
        ))
        .unwrap();
    handle
        .update(cx, |_, _, cx| view.update(cx, |_, cx| cx.notify()))
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "tab");
    assert_chip(handle, f.atoms[0], cx);
    let before = session.borrow().document().clone();
    handle
        .update(cx, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.scroll_handle.set_offset(point(px(0.), px(-2000.)));
                cx.notify();
            })
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    cx.simulate_keystrokes(handle.into(), "enter space");
    handle
        .update(cx, |frame, window, cx| {
            assert!(view.read(cx).atom_keyboard.borrow().entries.is_empty());
            window.focus(&frame.before);
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "tab");
    handle
        .update(cx, |frame, window, _| {
            assert!(frame.after.is_focused(window))
        })
        .unwrap();
    assert!(host.keys.borrow().is_empty());
    assert_eq!(session.borrow().document().store(), before.store());
    handle
        .update(cx, |frame, window, cx| {
            view.update(cx, |view, cx| {
                view.scroll_handle.set_offset(point(px(0.), px(0.)));
                cx.notify();
            });
            window.focus(&frame.before);
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "tab enter");
    assert_chip(handle, f.atoms[0], cx);
    assert_eq!(host.keys.borrow().len(), 1);
}
