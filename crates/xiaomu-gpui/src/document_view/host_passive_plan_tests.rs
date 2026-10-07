//! Mounted passive publication tests; virtual dispatch is not native IME evidence.

use super::*;
use crate::{
    block_view::SharedSession,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
};
use gpui::{
    AppContext as _, Entity, EntityInputHandler, Focusable as _, Render, TestAppContext,
    WindowHandle, div, point, prelude::*, px,
};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{
        InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        TextRun, XiaomuDocument,
    },
    selection::{InlinePoint, NodeGap},
    transaction::{DocumentTemplate, Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentPosition, DocumentSelection, EditIntent, SelectionUpdate,
};

#[path = "host_passive_plan_refusal_tests.rs"]
mod refusal;
#[path = "host_passive_plan_table_tests.rs"]
mod tables;

struct Listener(Rc<Cell<usize>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

struct Panes {
    a: Entity<DocumentView>,
    b: Entity<DocumentView>,
}
impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .child(div().flex_1().min_w_0().h_full().child(self.a.clone()))
            .child(div().flex_1().min_w_0().h_full().child(self.b.clone()))
    }
}

fn document(count: usize) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let children: Vec<_> = (0..count)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(inline("old")),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn inline(text: &str) -> InlineContent {
    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
}

fn children(document: &XiaomuDocument) -> &[NodeId] {
    document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
}

fn replacement(_document: &XiaomuDocument, count: usize, selection: SelectionUpdate) -> EditPlan {
    let mut builder = NodeStoreBuilder::new();
    let children: Vec<_> = (0..count)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(inline("new")),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    let source = XiaomuDocument::new(root, builder.finish()).unwrap();
    let transaction =
        Transaction::new(TransactionOrigin::System).with_step(TransactionStep::ReplaceDocument {
            template: DocumentTemplate::capture(&source).unwrap(),
        });
    EditPlan::new(transaction, selection, None)
}

fn editor(document: XiaomuDocument, counts: Rc<Cell<usize>>) -> EditorInstance {
    let initial = DocumentSelection::collapsed(InlinePoint::at_start_of(children(&document)[0]));
    EditorInstance::new(
        document,
        initial,
        EditorHooks {
            listener: Some(Box::new(Listener(counts))),
            ..Default::default()
        },
    )
    .unwrap()
}

fn open_panes(
    cx: &mut TestAppContext,
    a: &EditorInstance,
    b: &EditorInstance,
) -> WindowHandle<Panes> {
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            let a = cx.new(|_| a.build_view());
            let b = cx.new(|_| b.build_view());
            cx.new(|_| Panes { a, b })
        })
        .unwrap()
    });
    handle
        .update(cx, |panes, window, cx| {
            window.activate_window();
            panes
                .b
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
}

fn focus_text(session: &SharedSession) -> String {
    let session = session.borrow();
    let DocumentPosition::Inline(at) = session.selection().focus() else {
        panic!("expected inline caret");
    };
    session
        .document()
        .node(at.node_id())
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

#[gpui::test]
fn passive_background_replacement_preserves_other_pane_focus_scroll_and_input(
    cx: &mut TestAppContext,
) {
    let original = document(70);
    let changes_a = Rc::new(Cell::new(0));
    let changes_b = Rc::new(Cell::new(0));
    let a = editor(original.clone(), changes_a.clone());
    let b = editor(original.clone(), changes_b.clone());
    let session_a = a.session().clone();
    let session_b = b.session().clone();
    let selection_b = session_b.borrow().selection();
    let handle = open_panes(cx, &a, &b);
    // Deliberately scroll both panes away from their canonical carets.
    handle
        .update(cx, |panes, _, cx| {
            for view in [&panes.a, &panes.b] {
                view.update(cx, |view, cx| {
                    view.scroll_handle.set_offset(point(px(0.0), px(-140.0)));
                    cx.notify();
                });
            }
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let plan = replacement(&original, 70, SelectionUpdate::CaretAtDocumentEnd);
    handle
        .update(cx, |panes, window, cx| {
            let b_owner = panes
                .b
                .read(cx)
                .focused_child(window, cx)
                .unwrap()
                .entity_id();
            panes.a.update(cx, |view, cx| {
                let epoch = view.epoch.get();
                let old_ids: Vec<_> = view
                    .children
                    .iter()
                    .map(|(_, child)| child.entity_id())
                    .collect();
                view.is_dragging = true;
                view.cell_drag_anchor = Some(children(&original)[0]);
                view.desired_x = Some((InlinePoint::at_start_of(children(&original)[0]), px(7.0)));
                assert_eq!(
                    view.apply_passive_edit_plan(&plan, window, cx).unwrap(),
                    Some(SessionOutcome::DocumentChanged)
                );
                assert_eq!(view.epoch.get(), epoch + 1);
                assert!(!view.is_dragging);
                assert_eq!(view.cell_drag_anchor, None);
                assert_eq!(view.desired_x, None);
                assert!(view.registry.borrow().is_empty());
                assert!(view.cell_registry.borrow().is_empty());
                assert!(view.table_clips.borrow().is_empty());
                assert!(view.column_resize.measurements.borrow().is_empty());
                assert!(
                    view.children
                        .iter()
                        .all(|(_, child)| !old_ids.contains(&child.entity_id()))
                );
                assert!(view.focused_child(window, cx).is_none());
            });
            assert_eq!(
                panes
                    .b
                    .read(cx)
                    .focused_child(window, cx)
                    .unwrap()
                    .entity_id(),
                b_owner
            );
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, window, cx| {
            for view in [&panes.a, &panes.b] {
                assert_eq!(
                    view.read(cx).scroll_handle.offset(),
                    point(px(0.0), px(-140.0))
                );
            }
            assert!(panes.a.read(cx).focused_child(window, cx).is_none());
            assert!(panes.b.read(cx).focused_child(window, cx).is_some());
        })
        .unwrap();
    assert_eq!(session_b.borrow().selection(), selection_b);
    assert_eq!(session_b.borrow().document().store(), original.store());
    assert_eq!(session_b.borrow().history_depths(), (0, 0));
    assert_eq!(session_a.borrow().history_depths(), (1, 0));
    assert_eq!(changes_a.get(), 1);
    assert_eq!(changes_b.get(), 0);
    cx.simulate_input(handle.into(), "B");
    cx.background_executor.run_until_parked();
    assert_eq!(focus_text(&session_b), "Bold");
    assert_eq!(focus_text(&session_a), "new");
    assert_eq!(changes_a.get(), 1);
    assert_eq!(changes_b.get(), 1);
}

#[gpui::test]
fn passive_focused_fresh_ids_keep_native_input_without_scrolling_to_new_end(
    cx: &mut TestAppContext,
) {
    let original = document(70);
    let counts = Rc::new(Cell::new(0));
    let a = editor(original.clone(), counts.clone());
    let session = a.session().clone();
    let b = editor(original.clone(), Default::default());
    let handle = open_panes(cx, &a, &b);
    handle
        .update(cx, |panes, window, cx| {
            panes
                .a
                .update(cx, |view, cx| view.focus_selection(window, cx));
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, _, cx| {
            panes.a.update(cx, |view, cx| {
                view.scroll_handle.set_offset(point(px(0.0), px(-140.0)));
                cx.notify();
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let plan = replacement(&original, 70, SelectionUpdate::CaretAtDocumentEnd);
    handle
        .update(cx, |panes, window, cx| {
            panes.a.update(cx, |view, cx| {
                let old = view.focused_child(window, cx).unwrap();
                let old_scroll_epoch = old.read(cx).passive_scroll_epoch();
                // A previous frame can have queued a scroll against geometry
                // that the import retires before that callback executes.
                old.read(cx).request_caret_scroll();
                old.read(cx).keep_caret_visible(
                    &gpui::Bounds::new(point(px(0.0), px(5000.0)), gpui::size(px(2.0), px(28.0))),
                    window,
                );
                view.apply_passive_edit_plan(&plan, window, cx).unwrap();
                assert_ne!(old.read(cx).passive_scroll_epoch(), old_scroll_epoch);
                let new = view.focused_child(window, cx).unwrap();
                assert_ne!(new.entity_id(), old.entity_id());
                assert_eq!(
                    new.read(cx).node(),
                    *children(session.borrow().document()).last().unwrap()
                );
                assert!(!new.read(cx).scroll_caret_pending.get());
                assert!(new.read(cx).last_bounds.is_none());
                assert!(new.read(cx).last_layout.is_none());
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |panes, _, cx| {
            assert_eq!(
                panes.a.read(cx).scroll_handle.offset(),
                point(px(0.0), px(-140.0))
            );
        })
        .unwrap();
    cx.simulate_input(handle.into(), "!");
    cx.background_executor.run_until_parked();
    assert_eq!(focus_text(&session), "new!");
    assert_eq!(counts.get(), 2);
    assert_eq!(session.borrow().history_depths(), (2, 0));
    cx.simulate_keystrokes(handle.into(), "ctrl-z ctrl-z");
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().document().store(), original.store());
    cx.simulate_keystrokes(handle.into(), "ctrl-y");
    cx.background_executor.run_until_parked();
    assert_eq!(focus_text(&session), "new");
}

#[gpui::test]
fn passive_focus_transitions_cover_child_range_and_root_ownership(cx: &mut TestAppContext) {
    for initial in 0..3 {
        for target in 0..3 {
            let original = document(2);
            let a = editor(original.clone(), Default::default());
            let b = editor(original.clone(), Default::default());
            let session = a.session().clone();
            let before = match initial {
                0 => session.borrow().selection(),
                1 => DocumentSelection::all(&original),
                _ => DocumentSelection::collapsed(NodeGap::new(original.root(), 1)),
            };
            session.borrow_mut().set_document_selection(before).unwrap();
            let handle = open_panes(cx, &a, &b);
            handle
                .update(cx, |panes, window, cx| {
                    panes
                        .a
                        .update(cx, |view, cx| view.focus_selection(window, cx));
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            let after = match target {
                0 => SelectionUpdate::CaretAtDocumentEnd,
                1 => SelectionUpdate::AllDocument,
                _ => SelectionUpdate::CaretAtGap {
                    gap: NodeGap::new(original.root(), 1),
                },
            };
            let plan = replacement(&original, 2, after);
            handle
                .update(cx, |panes, window, cx| {
                    panes.a.update(cx, |view, cx| {
                        if initial == 2 {
                            assert!(view.focus_handle.as_ref().unwrap().is_focused(window));
                        } else {
                            assert!(view.focused_child(window, cx).is_some());
                        }
                        view.apply_passive_edit_plan(&plan, window, cx).unwrap();
                    });
                })
                .unwrap();
            cx.background_executor.run_until_parked();
            handle
                .update(cx, |panes, window, cx| {
                    let view = panes.a.read(cx);
                    match target {
                        0 => assert!(
                            view.children
                                .last()
                                .unwrap()
                                .1
                                .read(cx)
                                .focus_handle(cx)
                                .is_focused(window)
                        ),
                        1 => assert!(view.range_input_is_focused(window, cx)),
                        _ => assert!(view.focus_handle.as_ref().unwrap().is_focused(window)),
                    }
                    assert!(panes.b.read(cx).focused_child(window, cx).is_none());
                })
                .unwrap();
            if target == 0 {
                cx.simulate_input(handle.into(), "!");
                cx.background_executor.run_until_parked();
                assert_eq!(focus_text(&session), "new!");
            }
        }
    }
}
