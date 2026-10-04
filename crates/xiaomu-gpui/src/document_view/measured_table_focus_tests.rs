//! Mounted pane ownership, resized IME bounds and measured vertical navigation.
//! These exercise GPUI's virtual platform; they do not claim native XIM parity.

use gpui::{Bounds, Pixels, size};
use xiaomu_runtime::session::DocumentPosition;

use super::*;

struct SharedSessionPanes {
    legacy: Entity<DocumentView>,
    measured: Entity<DocumentView>,
}

impl Render for SharedSessionPanes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .child(div().w(px(320.0)).h_full().child(self.legacy.clone()))
            .child(div().w(px(320.0)).h_full().child(self.measured.clone()))
    }
}

#[gpui::test]
fn background_measured_pane_cannot_steal_shared_session_legacy_focus(cx: &mut TestAppContext) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.before);
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let legacy = cx.new(|_| DocumentView::new(session.clone()));
            let measured = cx.new(|_| {
                let mut view = DocumentView::new(session.clone());
                view.set_measured_table_layout(true);
                view
            });
            cx.new(|_| SharedSessionPanes { legacy, measured })
        })
        .unwrap()
    });
    handle
        .update(cx, |host, window, cx| {
            window.activate_window();
            host.legacy
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let legacy_input = handle
        .update(cx, |host, window, cx| {
            let legacy = host.legacy.read(cx);
            let measured = host.measured.read(cx);
            assert!(!Rc::ptr_eq(
                &legacy.table_capability,
                &measured.table_capability
            ));
            assert!(!legacy.table_capability.borrow().enabled());
            assert!(measured.table_capability.borrow().enabled());
            assert_eq!(
                legacy.hidden_table_ancestor(&fixture.document, fixture.blocks[0]),
                Some(fixture.table)
            );
            assert_eq!(
                measured.hidden_table_ancestor(&fixture.document, fixture.blocks[0]),
                None
            );
            assert!(legacy.cell_registry.borrow().is_empty());
            assert_eq!(measured.cell_registry.borrow().len(), fixture.cells.len());
            assert!(measured.focused_child(window, cx).is_none());
            assert!(!measured.focus_handle.as_ref().unwrap().is_focused(window));
            legacy.focused_child(window, cx).unwrap()
        })
        .unwrap();
    // Trigger another successful measurement after legacy owns a real native
    // input surface. Sharing canonical selection must not share focus ownership.
    handle
        .update(cx, |host, _, cx| {
            host.measured.update(cx, |_, cx| cx.notify());
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            assert_eq!(
                host.legacy
                    .read(cx)
                    .focused_child(window, cx)
                    .unwrap()
                    .entity_id(),
                legacy_input.entity_id()
            );
            assert!(host.measured.read(cx).focused_child(window, cx).is_none());
        })
        .unwrap();
    cx.simulate_input(handle.into(), "L");
    cx.background_executor.run_until_parked();
    assert_eq!(text(&session, fixture.before), "Lbefore");
    assert_eq!(text(&session, fixture.blocks[0]), "head");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    handle
        .update(cx, |host, window, cx| {
            let document = session.borrow().document().clone();
            assert!(host.legacy.read(cx).focused_child(window, cx).is_some());
            assert!(host.measured.read(cx).focused_child(window, cx).is_none());
            assert!(
                !host
                    .legacy
                    .read(cx)
                    .table_capability
                    .borrow()
                    .permits(&document, fixture.table)
            );
            assert!(
                host.measured
                    .read(cx)
                    .table_capability
                    .borrow()
                    .permits(&document, fixture.table)
            );
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
}

fn automatic_rowspan_fixture() -> Fixture {
    let mut fixture = fixture(Some("rowspan"));
    let mut transaction = Transaction::new(TransactionOrigin::System);
    for cell in &fixture.cells {
        let attrs = NodeAttrs::new(
            fixture
                .document
                .node(*cell)
                .unwrap()
                .attrs()
                .iter()
                .filter(|(key, _)| *key != "colwidth")
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect(),
        )
        .unwrap();
        transaction = transaction.with_step(TransactionStep::SetNodeAttrs { node: *cell, attrs });
    }
    fixture.document = transaction.apply(&fixture.document).unwrap();
    fixture
}

fn cell_bounds(view: &DocumentView, cell: NodeId) -> Bounds<Pixels> {
    view.cell_registry
        .borrow()
        .iter()
        .find(|(id, _)| *id == cell)
        .unwrap()
        .1
}

#[gpui::test]
fn measured_span_preedit_survives_real_window_resize_with_fresh_candidate_bounds(
    cx: &mut TestAppContext,
) {
    let fixture = automatic_rowspan_fixture();
    let session = session(&fixture, fixture.blocks[1]);
    let initial_selection = session.borrow().selection();
    let handle = open(session.clone(), cx);
    cx.simulate_window_resize(handle.into(), size(px(560.0), px(480.0)));
    cx.background_executor.run_until_parked();
    let input = handle
        .update(cx, |view, window, cx| {
            let input = view.focused_child(window, cx).unwrap();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "你好🙂", Some(4..4), window, cx);
                assert_eq!(input.marked_text_range(window, cx), Some(0..4));
            });
            input
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let mut painted = Vec::new();
    for width in [560.0, 340.0, 680.0] {
        cx.simulate_window_resize(handle.into(), size(px(width), px(480.0)));
        cx.background_executor.run_until_parked();
        painted.push(
            handle
                .update(cx, |view, window, cx| {
                    assert!(!view.selection_has_hidden_table_endpoint());
                    assert_eq!(
                        view.focused_child(window, cx).unwrap().entity_id(),
                        input.entity_id()
                    );
                    let block = view.block_bounds(fixture.blocks[1]).unwrap();
                    let cell = cell_bounds(view, fixture.cells[1]);
                    assert_eq!(block.left(), cell.left() + px(12.0));
                    assert_eq!(block.top(), cell.top() + px(9.0));
                    assert_eq!(block.size.width, cell.size.width - px(24.0));
                    input.update(cx, |input, cx| {
                        assert!(input.focus_handle(cx).is_focused(window));
                        assert!(input.is_composing());
                        assert_eq!(input.marked_text_range(window, cx), Some(0..4));
                        assert_eq!(
                            input.selected_text_range(false, window, cx).unwrap().range,
                            4..4
                        );
                        let candidate = input.bounds_for_range(0..4, block, window, cx).unwrap();
                        assert_eq!(candidate.origin, block.origin);
                        assert!(candidate.right() <= block.right());
                        assert!(candidate.bottom() <= block.bottom());
                        assert!(candidate.size.width > px(1.0));
                        let caret = input.bounds_for_range(4..4, block, window, cx).unwrap();
                        assert_eq!(caret.top(), block.top());
                        assert!(caret.left() > block.left());
                        assert!(caret.right() <= block.right());
                        // This method reads the paragraph's retained last_bounds,
                        // independently of the registry bounds passed above.
                        assert_eq!(
                            input.character_index_for_point(
                                block.origin + point(px(0.1), px(1.0)),
                                window,
                                cx
                            ),
                            Some(0)
                        );
                    });
                    block
                })
                .unwrap(),
        );
        assert_eq!(
            session.borrow().document().store(),
            fixture.document.store()
        );
        assert_eq!(session.borrow().selection(), initial_selection);
        assert_eq!(session.borrow().history_depths(), (0, 0));
    }
    assert!(painted[1].size.width < painted[0].size.width);
    assert!(painted[2].size.width > painted[0].size.width);
    assert!(painted[1].left() < painted[0].left());
    assert!(painted[2].left() > painted[0].left());
    // One platform IME commit must carry the complete string. simulate_input
    // deliberately emits separate characters, which would commit just "你"
    // and then make a second, ordinary-typing history entry for "好🙂".
    // The documented key->key_char test syntax reaches the installed native
    // handler once with this entire payload; it never inserts the physical x.
    cx.dispatch_keystroke(handle.into(), gpui::Keystroke::parse("x->你好🙂").unwrap());
    cx.background_executor.run_until_parked();
    assert_eq!(text(&session, fixture.blocks[1]), "你好🙂body");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    handle
        .update(cx, |view, window, cx| {
            assert!(!view.has_active_composition(cx));
            assert_eq!(
                view.focused_child(window, cx).unwrap().entity_id(),
                input.entity_id()
            );
        })
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
    assert_eq!(session.borrow().selection(), initial_selection);
}

#[gpui::test]
fn measured_rowspan_arrows_follow_logical_column_and_skip_full_source_extent(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.blocks[1]);
    let handle = open(session.clone(), cx);
    let after = *fixture
        .document
        .node(fixture.document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .last()
        .unwrap();
    handle
        .update(cx, |view, _, _| {
            let right_cell = cell_bounds(view, fixture.cells[1]);
            let right_block = view.block_bounds(fixture.blocks[1]).unwrap();
            let x = right_cell.right() - px(2.0);
            assert!(
                x > right_block.right(),
                "probe the full-cell padding, not paragraph width"
            );
            assert_eq!(
                view.vertical_neighbor(fixture.blocks[1], x, true),
                Some(fixture.blocks[2])
            );
            assert_eq!(
                view.vertical_neighbor(fixture.blocks[2], x, false),
                Some(fixture.blocks[1])
            );
            let left_cell = cell_bounds(view, fixture.cells[0]);
            assert_eq!(
                view.vertical_neighbor(fixture.blocks[0], left_cell.center().x, true),
                Some(after)
            );
        })
        .unwrap();
    for (key, expected) in [("down", fixture.blocks[2]), ("up", fixture.blocks[1])] {
        cx.simulate_keystrokes(handle.into(), key);
        cx.background_executor.run_until_parked();
        let DocumentPosition::Inline(point) = session.borrow().selection().focus() else {
            panic!("vertical arrow must retain an inline caret");
        };
        assert_eq!(point.node_id(), expected);
    }
    handle
        .update(cx, |view, window, cx| {
            view.place(InlinePoint::at_start_of(fixture.blocks[0]), window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    cx.simulate_keystrokes(handle.into(), "down");
    cx.background_executor.run_until_parked();
    let DocumentPosition::Inline(point) = session.borrow().selection().focus() else {
        panic!("rowspan exit must retain an inline caret");
    };
    assert_eq!(point.node_id(), after);
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
