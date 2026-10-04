//! Real keyboard routing distinguishes structural Select All from text coverage.
use super::DocumentView;
use crate::block_view::{SelectionProjection, SharedSession};
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::editor_commands::{
    CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
};
use crate::input::platform_clipboard::{PlatformClipboard, PlatformClipboardContent};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use std::{cell::RefCell, rc::Rc};
use xiaomu_core::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextOffset;
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentSelection, EditIntent, IntentDisposition, PolicyError, SessionContext, SessionPolicy,
};

struct AllRouter;
impl EditorCommandRouter for AllRouter {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
    fn select_all(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        Ok(Some(DocumentSelection::all(context.document())))
    }
}

type Seen = Rc<RefCell<Vec<(DocumentSelection, EditIntent)>>>;
struct Observe {
    seen: Seen,
    reject: bool,
}
impl SessionPolicy for Observe {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.seen
            .borrow_mut()
            .push((context.selection(), intent.clone()));
        if self.reject {
            Err(PolicyError::new("test rejection"))
        } else {
            Ok(IntentDisposition::NoChange)
        }
    }
}

fn fixture(atomic_edges: bool) -> (XiaomuDocument, NodeId, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let atom = b
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(
                InlineAtomContent::hard_break().with_marks(MarkSet::new([Mark::Bold]).unwrap()),
            ),
        )
        .unwrap();
    let first = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms([], [InlineAtomPlacement::new(atom, TextOffset::ZERO)])
                    .unwrap(),
            ),
        )
        .unwrap();
    let item = b
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([first]),
        )
        .unwrap();
    let list = b
        .insert(
            NodeKind::OrderedList,
            NodeAttrs::empty(),
            NodeContent::children([item]),
        )
        .unwrap();
    let tail = b
        .insert(
            NodeKind::Heading(HeadingLevel::new(2).unwrap()),
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("尾🙂", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let mut children = vec![list, tail];
    if atomic_edges {
        let leading = b
            .insert(NodeKind::Image, NodeAttrs::empty(), NodeContent::Atomic)
            .unwrap();
        let trailing = b
            .insert(
                NodeKind::HorizontalRule,
                NodeAttrs::empty(),
                NodeContent::Atomic,
            )
            .unwrap();
        children.insert(0, leading);
        children.push(trailing);
    }
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    (XiaomuDocument::new(root, b.finish()).unwrap(), first, tail)
}

fn open(
    cx: &mut TestAppContext,
    document: XiaomuDocument,
    first: NodeId,
    reject: bool,
) -> (WindowHandle<DocumentView>, SharedSession, Seen) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let editor = EditorInstance::new_with_policy(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(first)),
        EditorHooks::default(),
        Box::new(Observe {
            seen: seen.clone(),
            reject,
        }),
    )
    .unwrap()
    .with_command_router(Rc::new(AllRouter));
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
    (handle, session, seen)
}

fn key(cx: &mut TestAppContext, handle: WindowHandle<DocumentView>, key: &str) {
    cx.simulate_keystrokes(handle.into(), key);
    cx.background_executor.run_until_parked();
}
fn clipboard(cx: &mut TestAppContext) -> ClipboardSlice {
    cx.update(|cx| match PlatformClipboard::new(cx).read_content() {
        Some(PlatformClipboardContent::Structured(slice)) => slice,
        _ => panic!("complete structured clipboard required"),
    })
}

fn assert_all_routes(cx: &mut TestAppContext, reject: bool) {
    let (doc, first, tail) = fixture(true);
    let all = DocumentSelection::all(&doc);
    let (handle, session, seen) = open(cx, doc.clone(), first, reject);
    key(cx, handle, "ctrl-a");
    assert_eq!(session.borrow().selection(), all);
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input_is_focused(window, cx));
            let order = view
                .children
                .iter()
                .map(|(node, _)| *node)
                .collect::<Vec<_>>();
            for (_, child) in &view.children {
                assert!(matches!(
                    child.read(cx).projected_display_selection(&order),
                    SelectionProjection::Highlight { .. }
                ));
            }
            let input = view.range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                assert_eq!(
                    input.selected_text_range(false, window, cx).unwrap().range,
                    0..0
                )
            });
        })
        .unwrap();
    key(cx, handle, "ctrl-c");
    let closed = clipboard(cx);
    assert!(closed.is_closed());
    assert_eq!(closed.roots().len(), 4);
    assert_eq!(closed.roots()[0].kind(), &NodeKind::Image);
    assert_eq!(closed.roots()[3].kind(), &NodeKind::HorizontalRule);
    key(cx, handle, "ctrl-x");
    assert_eq!(clipboard(cx), closed);
    cx.simulate_input(handle.into(), "typed");
    cx.background_executor.run_until_parked();
    let typed: String = seen
        .borrow()
        .iter()
        .filter_map(|(_, intent)| match intent {
            EditIntent::InsertText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        typed, "typed",
        "native simulation emits one intent per character"
    );
    // Explicit platform replacement offsets belong to the empty proxy, not
    // to any canonical root child. They must retain the all-selection.
    handle
        .update(cx, |view, window, cx| {
            let input = view.range_input.as_ref().unwrap().1.clone();
            input.update(cx, |input, cx| {
                input.replace_text_in_range(Some(0..0), "range", window, cx)
            });
        })
        .unwrap();
    key(cx, handle, "ctrl-v");
    let events = seen.borrow();
    assert!(events.iter().all(|(selection, _)| *selection == all));
    assert!(
        events
            .iter()
            .any(|(_, intent)| matches!(intent, EditIntent::Delete))
    );
    assert!(
        events.iter().any(
            |(_, intent)| matches!(intent, EditIntent::InsertText { text } if text == "range")
        )
    );
    assert!(events.iter().any(
        |(_, intent)| matches!(intent, EditIntent::PasteSlice { slice } if slice.is_closed())
    ));
    drop(events);
    assert_eq!(session.borrow().selection(), all);
    assert_eq!(session.borrow().document().store(), doc.store());
    assert_eq!(session.borrow().document().revision(), doc.revision());
    assert_eq!(session.borrow().history_depths(), (0, 0));

    let tail_inline = doc.node(tail).unwrap().content().as_inline().unwrap();
    let text_range = DocumentSelection::new(
        InlinePoint::at_start_of(first),
        InlinePoint::new(
            tail,
            tail_inline.offset_at(tail_inline.len_bytes()).unwrap(),
            0,
            CursorAffinity::Before,
        ),
    );
    handle
        .update(cx, |view, window, cx| {
            session
                .borrow_mut()
                .set_document_selection(text_range)
                .unwrap();
            view.focus_selection(window, cx);
            cx.notify();
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    key(cx, handle, "ctrl-c");
    let open = clipboard(cx);
    assert!(!open.is_closed());
    assert_eq!(open.roots().len(), 2);
    assert_eq!(session.borrow().selection(), text_range);
}

#[gpui::test]
fn explicit_all_keyboard_copy_cut_type_paste_and_nochange_keep_root_semantics(
    cx: &mut TestAppContext,
) {
    assert_all_routes(cx, false);
}
#[gpui::test]
fn explicit_all_keyboard_policy_rejections_preserve_document_history_and_selection(
    cx: &mut TestAppContext,
) {
    assert_all_routes(cx, true);
}

#[gpui::test]
fn explicit_all_left_right_collapse_to_real_edge_and_do_not_replace_document(
    cx: &mut TestAppContext,
) {
    let (doc, first, tail) = fixture(false);
    let (handle, session, seen) = open(cx, doc.clone(), first, false);
    key(cx, handle, "ctrl-a");
    key(cx, handle, "right");
    let selection = session.borrow().selection();
    let (_, focus) = selection.as_same_node_inline().unwrap();
    assert!(selection.is_collapsed());
    assert_eq!(focus.node_id(), tail);
    assert_eq!(focus.text_offset().as_usize(), "尾🙂".len());
    cx.simulate_input(handle.into(), "after");
    let typed: String = seen
        .borrow()
        .iter()
        .filter_map(|(selection, intent)| match intent {
            EditIntent::InsertText { text } if !selection.is_all(&doc) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(typed, "after");
    key(cx, handle, "ctrl-a");
    key(cx, handle, "left");
    assert_eq!(
        session.borrow().selection(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(first))
    );
    assert_eq!(session.borrow().document().store(), doc.store());
}

#[gpui::test]
fn select_all_hook_does_not_change_an_active_native_composition(cx: &mut TestAppContext) {
    let (doc, first, tail) = fixture(false);
    let (handle, session, _) = open(cx, doc.clone(), first, false);
    handle
        .update(cx, |view, window, cx| {
            session
                .borrow_mut()
                .set_document_selection(DocumentSelection::collapsed(InlinePoint::at_start_of(
                    tail,
                )))
                .unwrap();
            view.focus_selection(window, cx);
            let child = view
                .children
                .iter()
                .find(|(id, _)| *id == tail)
                .unwrap()
                .1
                .clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "preedit", Some(7..7), window, cx)
            });
            let before = session.borrow().selection();
            view.select_all(&crate::block_view::SelectAll, window, cx);
            assert_eq!(session.borrow().selection(), before);
            assert!(child.read(cx).is_composing());
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "", None, window, cx)
            });
        })
        .unwrap();
}

/// A deliberately small host policy to exercise native successful publication
/// and focus restoration, independently of any product's replacement fitter.
struct ReplaceAllText;
impl SessionPolicy for ReplaceAllText {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
        use xiaomu_runtime::session::{EditPlan, SelectionUpdate};
        if !context.selection().is_all(context.document()) {
            return Ok(IntentDisposition::Continue);
        }
        let text = match intent {
            EditIntent::InsertText { text } => text,
            EditIntent::CommitComposition { range, text }
                if range.start() == TextOffset::ZERO && range.end() == TextOffset::ZERO =>
            {
                text
            }
            _ => return Ok(IntentDisposition::NoChange),
        };
        let root = context.document().root();
        let mut transaction = Transaction::new(TransactionOrigin::UserInput);
        for node in context
            .document()
            .node(root)
            .unwrap()
            .content()
            .as_children()
            .unwrap()
        {
            transaction.push_step(TransactionStep::RemoveNode { node: *node });
        }
        let inline = if text.is_empty() {
            InlineContent::empty()
        } else {
            InlineContent::new([TextRun::new(text.as_str(), MarkSet::empty()).unwrap()]).unwrap()
        };
        transaction.push_step(TransactionStep::InsertNode {
            parent: root,
            index: 0,
            kind: NodeKind::Paragraph,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Inline(inline),
        });
        Ok(IntentDisposition::Apply(EditPlan::new(
            transaction,
            SelectionUpdate::CaretAtLastInsertedOffset { offset: text.len() },
            None,
        )))
    }
}

#[gpui::test]
fn explicit_all_ime_cancel_commit_undo_and_native_focus_restore(cx: &mut TestAppContext) {
    let (doc, first, _) = fixture(true);
    let all = DocumentSelection::all(&doc);
    let editor = EditorInstance::new_with_policy(
        doc.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(first)),
        EditorHooks::default(),
        Box::new(ReplaceAllText),
    )
    .unwrap()
    .with_command_router(Rc::new(AllRouter));
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
    key(cx, handle, "ctrl-a");
    handle
        .update(cx, |view, window, cx| {
            let proxy = view.range_input.as_ref().unwrap().1.clone();
            proxy.update(cx, |proxy, cx| {
                proxy.replace_and_mark_text_in_range(Some(0..0), "ni", Some(2..2), window, cx);
                assert_eq!(proxy.marked_text_range(window, cx), Some(0..2));
                assert_eq!(session.borrow().selection(), all);
                assert_eq!(session.borrow().document().store(), doc.store());
                assert_eq!(proxy.layout_content().0, "ni");
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    handle
        .update(cx, |view, window, cx| {
            let proxy = view.range_input.as_ref().unwrap().1.clone();
            proxy.update(cx, |proxy, cx| {
                let bounds = proxy
                    .last_bounds
                    .expect("visible root-range preedit bounds");
                let candidate = proxy
                    .bounds_for_range(2..2, bounds, window, cx)
                    .expect("candidate anchor");
                assert!(candidate.left() >= bounds.left());
                assert!(candidate.left() <= bounds.right());
                assert!(candidate.top() >= bounds.top());
                assert!(candidate.bottom() <= bounds.bottom());

                proxy.replace_and_mark_text_in_range(None, "", None, window, cx);
                assert!(!proxy.is_composing());
                assert_eq!(session.borrow().selection(), all);
                assert_eq!(session.borrow().history_depths(), (0, 0));
                proxy.replace_and_mark_text_in_range(None, "zhong", Some(5..5), window, cx);
                proxy.replace_text_in_range(None, "中🙂", window, cx);
                assert!(!proxy.is_composing());
            });
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(session.borrow().history_depths(), (1, 0));
    assert_eq!(session.borrow().document().node_count(), 2);
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input.is_none());
            let (_, focus) = session.borrow().selection().as_same_node_inline().unwrap();
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(focus.node_id())
            );
            assert_eq!(focus.text_offset().as_usize(), "中🙂".len());
        })
        .unwrap();
    cx.simulate_input(handle.into(), "!");
    let focus = session
        .borrow()
        .selection()
        .as_same_node_inline()
        .unwrap()
        .1
        .node_id();
    assert_eq!(
        session
            .borrow()
            .document()
            .node(focus)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .runs()[0]
            .text()
            .as_str(),
        "中🙂!"
    );
    key(cx, handle, "ctrl-z");
    key(cx, handle, "ctrl-z");
    assert_eq!(session.borrow().document().store(), doc.store());
    assert_eq!(session.borrow().selection(), all);
    handle
        .update(cx, |view, window, cx| {
            assert!(view.range_input_is_focused(window, cx))
        })
        .unwrap();
}

#[cfg(target_os = "linux")]
#[gpui::test]
fn explicit_all_linux_unmark_commits_once_without_waiting_or_moving_the_selection(
    cx: &mut TestAppContext,
) {
    let (doc, first, _) = fixture(false);
    let all = DocumentSelection::all(&doc);
    let (handle, session, seen) = open(cx, doc.clone(), first, false);
    key(cx, handle, "ctrl-a");
    handle
        .update(cx, |view, window, cx| {
            let proxy = view.range_input.as_ref().unwrap().1.clone();
            proxy.update(cx, |proxy, cx| {
                proxy.replace_and_mark_text_in_range(None, "中文", Some(2..2), window, cx);
                proxy.unmark_text(window, cx);
                proxy.unmark_text(window, cx);
                assert!(!proxy.is_composing());
            });
        })
        .unwrap();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, all);
    assert!(
        matches!(&seen[0].1, EditIntent::CommitComposition { range, text }
        if range.start() == TextOffset::ZERO && range.end() == TextOffset::ZERO && text == "中文")
    );
    assert_eq!(session.borrow().selection(), all);
    assert_eq!(session.borrow().document().store(), doc.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn closed_clipboard_into_code_block_reaches_policy_without_flattening(cx: &mut TestAppContext) {
    use xiaomu_runtime::session::DocumentSession;
    let (source, _, _) = fixture(false);
    let slice = DocumentSession::new(source.clone(), DocumentSelection::all(&source))
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap();
    let mut b = NodeStoreBuilder::new();
    let code = b
        .insert(
            NodeKind::CodeBlock,
            NodeAttrs::empty(),
            NodeContent::empty_inline(),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([code]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let (handle, session, seen) = open(cx, doc.clone(), code, false);
    cx.update(|cx| PlatformClipboard::new(cx).write_slice(&slice));
    key(cx, handle, "ctrl-v");
    assert!(seen.borrow().iter().any(
        |(_, intent)| matches!(intent, EditIntent::PasteSlice { slice } if slice.is_closed())
    ));
    assert!(
        !seen
            .borrow()
            .iter()
            .any(|(_, intent)| matches!(intent, EditIntent::PasteText { .. }))
    );
    assert_eq!(session.borrow().document().store(), doc.store());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}
