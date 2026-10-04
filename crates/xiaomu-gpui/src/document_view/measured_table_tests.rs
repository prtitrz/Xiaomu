//! Mounted native-input tests for opt-in layout; legacy guard tests stay intact.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler, Focusable as _, IntoElement,
    ParentElement, Render, Styled, TestAppContext, VisualTestContext, Window, WindowHandle, div,
    point, px,
};
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

use super::DocumentView;
use crate::block_view::SharedSession;
use crate::editor::bind_default_editor_keys;

#[path = "measured_table_focus_tests.rs"]
mod focus_tests;

#[path = "measured_table_handoff_tests.rs"]
mod handoff_tests;

#[path = "measured_table_guard_tests.rs"]
mod presentation_guard_tests;

struct Fixture {
    document: XiaomuDocument,
    table: NodeId,
    cells: Vec<NodeId>,
    blocks: Vec<NodeId>,
    before: NodeId,
}

fn paragraph(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap()
}

fn fixture(span: Option<&str>) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder, "before");
    let after = paragraph(&mut builder, "after");
    let mut blocks = Vec::new();
    let mut cells = Vec::new();
    let count = if span.is_some() { 3 } else { 4 };
    for index in 0..count {
        let block = paragraph(&mut builder, if index == 0 { "head" } else { "body" });
        let widths = match (span, index) {
            (Some("colspan"), 0) => vec![80, 120],
            (Some("colspan"), 1) | (_, 0) | (None, 2) => vec![80],
            _ => vec![120],
        };
        let mut attrs = vec![(
            "colwidth".into(),
            AttrValue::List(widths.into_iter().map(AttrValue::Integer).collect()),
        )];
        if index == 0 {
            attrs.push((
                "backgroundColor".into(),
                AttrValue::String("rgb(240, 241, 242)".into()),
            ));
            if let Some(span) = span {
                attrs.push((span.into(), AttrValue::Integer(2)));
            }
        }
        let cell = builder
            .insert(
                if index == 0 {
                    NodeKind::TableHeader
                } else {
                    NodeKind::TableCell
                },
                NodeAttrs::new(attrs.into_iter().collect()).unwrap(),
                NodeContent::children([block]),
            )
            .unwrap();
        blocks.push(block);
        cells.push(cell);
    }
    let split = if span == Some("colspan") { 1 } else { 2 };
    let rows: Vec<_> = [&cells[..split], &cells[split..]]
        .into_iter()
        .map(|cells| {
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells.iter().copied()),
                )
                .unwrap()
        })
        .collect();
    let table = builder
        .insert(
            NodeKind::Table,
            NodeAttrs::empty(),
            NodeContent::children(rows),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([before, table, after]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        table,
        cells,
        blocks,
        before,
    }
}

fn session(fixture: &Fixture, node: NodeId) -> SharedSession {
    Rc::new(RefCell::new(
        DocumentSession::new(
            fixture.document.clone(),
            DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        )
        .unwrap(),
    ))
}

fn open(session: SharedSession, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| {
                let mut view = DocumentView::new(session);
                view.set_measured_table_layout(true);
                view
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
}

fn text(session: &SharedSession, node: NodeId) -> String {
    session
        .borrow()
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn repaint(handle: WindowHandle<DocumentView>, cx: &mut TestAppContext) {
    handle.update(cx, |_, _, cx| cx.notify()).unwrap();
    cx.background_executor.run_until_parked();
}

#[gpui::test]
fn measured_headers_spans_native_bounds_and_same_frame_batch_typing(cx: &mut TestAppContext) {
    for span in ["colspan", "rowspan"] {
        let fixture = fixture(Some(span));
        let session = session(&fixture, fixture.blocks[0]);
        let handle = open(session.clone(), cx);
        handle
            .update(cx, |view, window, cx| {
                assert!(!view.selection_has_hidden_table_endpoint());
                assert_eq!(view.cell_registry.borrow().len(), 3);
                let cell = view
                    .cell_registry
                    .borrow()
                    .iter()
                    .find(|(id, _)| *id == fixture.cells[0])
                    .unwrap()
                    .1;
                let block = view.block_bounds(fixture.blocks[0]).unwrap();
                assert_eq!(block.left(), cell.left() + px(12.0));
                assert_eq!(block.top(), cell.top() + px(9.0));
                assert_eq!(block.size.width, cell.size.width - px(24.0));
                assert_eq!(
                    cell.size.width,
                    px(if span == "colspan" { 200.0 } else { 80.0 })
                );
                assert_eq!(view.cell_at_position(cell.center()), Some(fixture.cells[0]));
                let input = view
                    .focused_child(window, cx)
                    .expect("first successful measurement routes owned focus");
                input.update(cx, |input, cx| {
                    assert_eq!(
                        input
                            .bounds_for_range(0..0, block, window, cx)
                            .unwrap()
                            .origin,
                        block.origin
                    );
                    assert_eq!(
                        input.character_index_for_point(
                            block.origin + point(px(0.1), px(1.0)),
                            window,
                            cx
                        ),
                        Some(0)
                    );
                    input.replace_text_in_range(None, "a", window, cx);
                    // No redraw or document-revision equality check between these.
                    input.replace_text_in_range(None, "b", window, cx);
                    assert_eq!(
                        input.selected_text_range(false, window, cx).unwrap().range,
                        2..2
                    );
                });
            })
            .unwrap();
        assert_eq!(text(&session, fixture.blocks[0]), "abhead");
        assert_eq!(
            session
                .borrow()
                .document()
                .node(fixture.cells[0])
                .unwrap()
                .attrs(),
            fixture.document.node(fixture.cells[0]).unwrap().attrs()
        );
        cx.background_executor.run_until_parked();
        cx.simulate_keystrokes(handle.into(), "right");
        cx.simulate_input(handle.into(), "X");
        assert_eq!(text(&session, fixture.blocks[0]), "abhXead");
    }
}

#[gpui::test]
fn measured_cell_range_uses_logical_rowspan_targets_and_real_proxy(cx: &mut TestAppContext) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.blocks[1]);
    let handle = open(session.clone(), cx);
    for key in ["ctrl-shift-space", "shift-down", "shift-left"] {
        cx.simulate_keystrokes(handle.into(), key);
        cx.background_executor.run_until_parked();
    }
    let range = session.borrow().selection().active_cell_range().unwrap();
    assert_eq!(range.anchor(), fixture.cells[1]);
    assert_eq!(range.focus(), fixture.cells[0]);
    assert_eq!(
        range.unique_origins(session.borrow().document()).unwrap(),
        fixture.cells
    );
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input_is_focused(window, cx));
            let proxy = view.focused_child(window, cx).unwrap();
            proxy.update(cx, |input, cx| {
                assert!(input.selected_text_range(false, window, cx).is_some());
                input.replace_text_in_range(None, "range", window, cx);
            });
        })
        .unwrap();
    assert!(session.borrow().history_depths().0 > 0);
    cx.background_executor.run_until_parked();
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
}

#[gpui::test]
fn invalid_width_revokes_retained_paragraph_and_range_handlers_before_repaint_then_undo_recovers(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.blocks[0]);
    let handle = open(session.clone(), cx);
    let (paragraph, proxy) = handle
        .update(cx, |view, window, cx| {
            let paragraph = view.focused_child(window, cx).unwrap();
            view.install_cell_range(fixture.cells[0], fixture.cells[2], window, cx);
            let proxy = view.focused_child(window, cx).unwrap();
            view.place(InlinePoint::at_start_of(fixture.before), window, cx);
            (paragraph, proxy)
        })
        .unwrap();
    let mut attrs = session
        .borrow()
        .document()
        .node(fixture.cells[2])
        .unwrap()
        .attrs()
        .clone();
    attrs = NodeAttrs::new(
        attrs
            .iter()
            .filter(|(key, _)| *key != "colwidth")
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .chain([(
                "colwidth".into(),
                AttrValue::List(vec![AttrValue::Integer(137)]),
            )])
            .collect(),
    )
    .unwrap();
    session
        .borrow_mut()
        .apply(&Transaction::new(TransactionOrigin::System).with_step(
            TransactionStep::SetNodeAttrs {
                node: fixture.cells[2],
                attrs,
            },
        ))
        .unwrap();
    let invalid = session.borrow().document().clone();
    let selection = session.borrow().selection();
    handle
        .update(cx, |_, window, cx| {
            for input in [&paragraph, &proxy] {
                input.update(cx, |input, cx| {
                    input.replace_text_in_range(None, "late", window, cx);
                    input.replace_and_mark_text_in_range(None, "late preedit", None, window, cx);
                    input.unmark_text(window, cx);
                    assert!(input.selected_text_range(false, window, cx).is_none());
                    assert!(input.text_for_range(0..0, &mut None, window, cx).is_none());
                });
            }
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), invalid.store());
    assert_eq!(session.borrow().selection(), selection);
    repaint(handle, cx);
    let selector =
        Box::leak(format!("unsupported-measured-table-{:?}", fixture.table).into_boxed_str());
    assert!(
        VisualTestContext::from_window(handle.into(), cx)
            .debug_bounds(selector)
            .is_some()
    );
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    cx.background_executor.run_until_parked();
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
    handle
        .update(cx, |view, window, cx| {
            view.place(InlinePoint::at_start_of(fixture.blocks[0]), window, cx);
            assert!(!view.selection_has_hidden_table_endpoint());
            assert!(view.focused_child(window, cx).is_some());
        })
        .unwrap();
    cx.simulate_input(handle.into(), "restored");
    assert_eq!(text(&session, fixture.blocks[0]), "restoredhead");
}

#[gpui::test]
fn unit_to_span_and_undo_remeasure_while_deleted_handlers_stay_revoked(cx: &mut TestAppContext) {
    let fixture = fixture(None);
    let session = session(&fixture, fixture.blocks[1]);
    let handle = open(session.clone(), cx);
    let removed = handle
        .update(cx, |view, window, cx| {
            view.focused_child(window, cx).unwrap()
        })
        .unwrap();
    let attrs = NodeAttrs::new(
        [
            ("colspan".into(), AttrValue::Integer(2)),
            (
                "colwidth".into(),
                AttrValue::List(vec![AttrValue::Integer(80), AttrValue::Integer(120)]),
            ),
        ]
        .into(),
    )
    .unwrap();
    let merge = Transaction::new(TransactionOrigin::System)
        .with_step(TransactionStep::SetNodeAttrs {
            node: fixture.cells[0],
            attrs,
        })
        .with_step(TransactionStep::RemoveNode {
            node: fixture.cells[1],
        });
    session
        .borrow_mut()
        .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
            fixture.before,
        )))
        .unwrap();
    session.borrow_mut().apply(&merge).unwrap();
    repaint(handle, cx);
    let merged = session.borrow().document().clone();
    handle
        .update(cx, |view, window, cx| {
            assert!(
                view.table_capability
                    .borrow()
                    .permits(session.borrow().document(), fixture.table)
            );
            removed.update(cx, |input, cx| {
                input.replace_text_in_range(None, "removed", window, cx);
                assert!(input.selected_text_range(false, window, cx).is_none());
            });
            let outside = &view
                .children
                .iter()
                .find(|(node, _)| *node == fixture.before)
                .unwrap()
                .1;
            assert_eq!(
                view.focused_child(window, cx).unwrap().entity_id(),
                outside.entity_id(),
                "dropping the old focused cell preserves this pane's keyboard route"
            );
        })
        .unwrap();
    assert_eq!(session.borrow().document().store(), merged.store());
    assert_eq!(session.borrow().history_depths(), (1, 0));
    cx.simulate_keystrokes(handle.into(), "ctrl-z");
    cx.background_executor.run_until_parked();
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
    handle
        .update(cx, |view, window, cx| {
            view.place(InlinePoint::at_start_of(fixture.blocks[1]), window, cx)
        })
        .unwrap();
    cx.simulate_input(handle.into(), "unit");
    assert_eq!(text(&session, fixture.blocks[1]), "unitbody");
}

struct SizedHost {
    editor: Entity<DocumentView>,
    width: f32,
}

impl Render for SizedHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(self.width))
            .h(px(240.0))
            .child(self.editor.clone())
    }
}

#[gpui::test]
fn actual_width_failure_revokes_unchanged_key_cancels_preedit_and_restores_after_resize(
    cx: &mut TestAppContext,
) {
    let fixture = fixture(Some("rowspan"));
    let session = session(&fixture, fixture.blocks[0]);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let editor = cx.new(|_| {
                let mut view = DocumentView::new(session.clone());
                view.set_measured_table_layout(true);
                view
            });
            cx.new(|_| SizedHost {
                editor,
                width: 400.0,
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |host, window, cx| {
            host.editor
                .update(cx, |view, cx| view.focus_selection(window, cx))
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let retained = handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                let input = view.focused_child(window, cx).unwrap();
                input.update(cx, |input, cx| {
                    input.replace_and_mark_text_in_range(None, "preedit", None, window, cx)
                });
                input
            })
        })
        .unwrap();
    handle
        .update(cx, |host, _, cx| {
            host.width = 1_000_100.0;
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                assert!(view.selection_has_hidden_table_endpoint());
                assert!(view.cell_registry.borrow().is_empty());
                assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
                retained.update(cx, |input, cx| {
                    assert!(!input.is_composing());
                    input.replace_text_in_range(None, "hidden", window, cx);
                    assert!(input.selected_text_range(false, window, cx).is_none());
                });
            });
        })
        .unwrap();
    assert_eq!(
        session.borrow().document().store(),
        fixture.document.store()
    );
    handle
        .update(cx, |host, _, cx| {
            host.width = 400.0;
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |host, window, cx| {
            host.editor.update(cx, |view, cx| {
                assert!(!view.selection_has_hidden_table_endpoint());
                assert!(view.focused_child(window, cx).is_some());
            });
        })
        .unwrap();
}
