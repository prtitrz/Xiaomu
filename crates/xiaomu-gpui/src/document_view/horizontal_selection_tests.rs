//! Real keyboard actions collapse text selections before horizontal stepping.

use std::{cell::Cell, rc::Rc};

use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, MarkKind, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSelection};

use super::DocumentView;
use crate::block_view::SharedSession;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};

fn document(texts: &[&str]) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = texts
        .iter()
        .map(|text| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([TextRun::new(*text, MarkSet::empty()).unwrap()])
                            .unwrap(),
                    ),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

fn point(doc: &XiaomuDocument, node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    let at = doc
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(raw)
        .unwrap();
    InlinePoint::new(node, at, ordinal, CursorAffinity::Before)
}

fn open(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    selection: DocumentSelection,
) -> (WindowHandle<DocumentView>, SharedSession) {
    let editor = EditorInstance::new(document, selection, EditorHooks::default()).unwrap();
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (handle, session)
}

fn step(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, key: &str) {
    cx.simulate_keystrokes(handle.into(), key);
    cx.background_executor.run_until_parked();
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

struct Listener(Rc<Cell<(usize, usize)>>);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (docs, sels) = self.0.get();
        self.0.set((docs + 1, sels));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (docs, sels) = self.0.get();
        self.0.set((docs, sels + 1));
    }
}

#[gpui::test]
fn select_all_right_code_then_type_does_not_replace_the_selected_text(cx: &mut TestAppContext) {
    let (doc, nodes) = document(&["B"]);
    let node = nodes[0];
    let end = point(&doc, node, 1, 0);
    let (handle, session) = open(cx, doc, DocumentSelection::collapsed(end));
    step(cx, handle, "ctrl-a");
    assert!(!session.borrow().selection().is_collapsed());
    step(cx, handle, "right");
    assert_eq!(
        session.borrow().selection(),
        DocumentSelection::collapsed(end)
    );
    step(cx, handle, "ctrl-e");
    assert!(
        session
            .borrow()
            .stored_marks()
            .unwrap()
            .contains(MarkKind::Code)
    );
    cx.simulate_input(handle.into(), "C");
    assert_eq!(text(&session, node), "BC");
    let borrowed = session.borrow();
    let runs = borrowed
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs();
    assert!(!runs[0].marks().contains(MarkKind::Code));
    assert_eq!(runs[1].text().as_str(), "C");
    assert!(runs[1].marks().contains(MarkKind::Code));
}

#[gpui::test]
fn arrows_collapse_forward_and_reversed_ranges_at_edges_and_inside_text(cx: &mut TestAppContext) {
    for (start, end) in [(0, "A你🙂Z".len()), (1, 4)] {
        for reversed in [false, true] {
            for forward in [false, true] {
                let (doc, nodes) = document(&["A你🙂Z"]);
                let node = nodes[0];
                let head = point(&doc, node, start, 0);
                let tail = point(&doc, node, end, 0);
                let selection = if reversed {
                    DocumentSelection::new(tail, head)
                } else {
                    DocumentSelection::new(head, tail)
                };
                let revision = doc.revision();
                let (handle, session) = open(cx, doc, selection);
                let counts = Rc::new(Cell::new((0, 0)));
                session
                    .borrow_mut()
                    .add_listener(Box::new(Listener(counts.clone())));
                step(cx, handle, if forward { "right" } else { "left" });
                assert_eq!(
                    session.borrow().selection(),
                    DocumentSelection::collapsed(if forward { tail } else { head })
                );
                assert_eq!(session.borrow().document().revision(), revision);
                assert_eq!(session.borrow().history_depths(), (0, 0));
                assert_eq!(counts.get(), (0, 1));
            }
        }
    }
}

#[gpui::test]
fn cross_block_collapse_uses_document_order_and_routes_native_focus(cx: &mut TestAppContext) {
    for reversed in [false, true] {
        for forward in [false, true] {
            let (doc, nodes) = document(&["a你", "🙂z"]);
            let head = point(&doc, nodes[0], 1, 0);
            let tail = point(&doc, nodes[1], "🙂".len(), 0);
            let selection = if reversed {
                DocumentSelection::new(tail, head)
            } else {
                DocumentSelection::new(head, tail)
            };
            let (handle, session) = open(cx, doc, selection);
            step(cx, handle, if forward { "right" } else { "left" });
            let target = if forward { tail } else { head };
            assert_eq!(
                session.borrow().selection(),
                DocumentSelection::collapsed(target)
            );
            let focus = handle
                .update(cx, |view, window, cx| {
                    view.accessibility_projection(window, cx)
                        .unwrap()
                        .focus_owner()
                })
                .unwrap();
            assert_eq!(focus, Some(target.node_id()));
            cx.simulate_input(handle.into(), "X");
            assert_eq!(
                text(&session, nodes[0]),
                if forward { "a你" } else { "aX你" }
            );
            assert_eq!(
                text(&session, nodes[1]),
                if forward { "🙂Xz" } else { "🙂z" }
            );
        }
    }
}

#[gpui::test]
fn inline_atom_selection_edges_preserve_ordinals_at_start_middle_and_end(cx: &mut TestAppContext) {
    for raw in [0, 1, 2] {
        for reversed in [false, true] {
            for forward in [false, true] {
                let (mut doc, nodes) = document(&["AB"]);
                let node = nodes[0];
                for ordinal in 0..2 {
                    let at = point(&doc, node, raw, ordinal);
                    doc = Transaction::new(TransactionOrigin::System)
                        .with_step(TransactionStep::InsertInlineAtom {
                            at,
                            kind: AtomKind::new("mention").unwrap(),
                            attrs: NodeAttrs::empty(),
                            content: InlineAtomContent::new("@A").unwrap(),
                        })
                        .apply(&doc)
                        .unwrap();
                }
                let head = point(&doc, node, raw, 0);
                let tail = point(&doc, node, raw, 2);
                let selection = if reversed {
                    DocumentSelection::new(tail, head)
                } else {
                    DocumentSelection::new(head, tail)
                };
                let original = doc.clone();
                let (handle, session) = open(cx, doc, selection);
                step(cx, handle, if forward { "right" } else { "left" });
                assert_eq!(
                    session.borrow().selection(),
                    DocumentSelection::collapsed(if forward { tail } else { head })
                );
                assert_eq!(session.borrow().document().store(), original.store());
                assert_eq!(session.borrow().document().revision(), original.revision());
                cx.simulate_input(handle.into(), "X");
                let inline = session
                    .borrow()
                    .document()
                    .node(node)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .clone();
                assert_eq!(inline.atoms().len(), 2);
                assert_eq!(
                    text(&session, node),
                    match raw {
                        0 => "XAB",
                        1 => "AXB",
                        _ => "ABX",
                    }
                );
            }
        }
    }
}

#[gpui::test]
fn shift_arrows_keep_extending_and_collapsed_arrows_still_step(cx: &mut TestAppContext) {
    for (anchor, focus, key, expected) in [
        (1, 3, "shift-right", 4),
        (1, 3, "shift-left", 2),
        (3, 1, "shift-left", 0),
        (3, 1, "shift-right", 2),
        (0, 4, "shift-right", 4),
        (4, 0, "shift-left", 0),
    ] {
        let (doc, nodes) = document(&["abcd"]);
        let node = nodes[0];
        let anchor = point(&doc, node, anchor, 0);
        let focus = point(&doc, node, focus, 0);
        let expected = point(&doc, node, expected, 0);
        let (handle, session) = open(cx, doc, DocumentSelection::new(anchor, focus));
        step(cx, handle, key);
        assert_eq!(
            session.borrow().selection(),
            DocumentSelection::new(anchor, expected)
        );
    }
    for (key, expected) in [("left", 1), ("right", 3)] {
        let (doc, nodes) = document(&["abcd"]);
        let node = nodes[0];
        let at = point(&doc, node, 2, 0);
        let expected = point(&doc, node, expected, 0);
        let (handle, session) = open(cx, doc, DocumentSelection::collapsed(at));
        step(cx, handle, key);
        assert_eq!(
            session.borrow().selection(),
            DocumentSelection::collapsed(expected)
        );
    }
}

#[gpui::test]
fn composition_still_blocks_selection_collapse_until_cancelled(cx: &mut TestAppContext) {
    let (doc, nodes) = document(&["abc"]);
    let node = nodes[0];
    let start = point(&doc, node, 0, 0);
    let end = point(&doc, node, 3, 0);
    let selection = DocumentSelection::new(start, end);
    let (handle, session) = open(cx, doc, selection);
    handle
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", None, window, cx)
            });
        })
        .unwrap();
    step(cx, handle, "right");
    assert_eq!(session.borrow().selection(), selection);
    handle
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "", None, window, cx)
            });
        })
        .unwrap();
    step(cx, handle, "right");
    assert_eq!(
        session.borrow().selection(),
        DocumentSelection::collapsed(end)
    );
    assert_eq!(text(&session, node), "abc");
}
