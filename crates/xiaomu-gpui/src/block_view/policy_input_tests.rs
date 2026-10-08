//! Explicit native replacement ranges remain atomic when policy refuses input.

use super::*;
use crate::document_view::{EditorRejection, EditorRejectionReason, EditorRejectionStage};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    Mark, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument,
};
use xiaomu_core::selection::TextPoint;
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, IntentDisposition, PolicyError, SessionContext,
    SessionPolicy,
};

struct RefuseBang(bool);

impl SessionPolicy for RefuseBang {
    fn prepare_intent(
        &self,
        context: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if let EditIntent::InsertText { text } = intent
            && text == "!"
        {
            assert!(!context.selection().is_collapsed());
            assert_eq!(context.stored_marks(), None);
            if self.0 {
                return Err(PolicyError::new("preflight"));
            }
        }
        Ok(IntentDisposition::Continue)
    }
    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .any(|run| run.text().as_str().contains('!'))
        {
            return Err(PolicyError::new("candidate"));
        }
        Ok(())
    }
}

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (doc, selection) = self.0.get();
        self.0.set((doc + 1, selection));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (doc, selection) = self.0.get();
        self.0.set((doc, selection + 1));
    }
}

fn open(
    cx: &mut TestAppContext,
    preflight: bool,
) -> (WindowHandle<ParagraphView>, SharedSession, Counts) {
    let mut builder = NodeStoreBuilder::new();
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
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
    let mut session = DocumentSession::new_with_policy(
        document,
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
        Box::new(RefuseBang(preflight)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::ToggleMark { mark: Mark::Bold })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    let session = Rc::new(RefCell::new(session));
    let handle = cx.update(|cx| {
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
    (handle, session, counts)
}

#[gpui::test]
fn platform_range_rejection_preserves_marks_selection_group_and_all_listeners(
    cx: &mut TestAppContext,
) {
    for preflight in [true, false] {
        let (handle, session, counts) = open(cx, preflight);
        let document = session.borrow().document().clone();
        let selection = session.borrow().selection();
        let marks = session.borrow().stored_marks().cloned();
        let revision = document.revision();
        let entity = handle.update(cx, |_, _, cx| cx.entity()).unwrap();
        let events = Rc::new(Cell::new(0));
        let seen = events.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(&entity, move |emitter, event: &EditorRejection, cx| {
                assert_eq!(event.stage(), EditorRejectionStage::NativeInput);
                assert_eq!(event.reason(), EditorRejectionReason::Policy);
                assert_eq!(event.document_revision(), revision);
                assert_eq!(emitter.read(cx).session.borrow().history_depths(), (1, 0));
                seen.set(seen.get() + 1);
            })
        });
        handle
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(Some(0..1), "!", window, cx)
            })
            .unwrap();
        assert_eq!(session.borrow().document().store(), document.store());
        assert_eq!(session.borrow().document().revision(), document.revision());
        assert_eq!(session.borrow().selection(), selection);
        assert_eq!(session.borrow().stored_marks(), marks.as_ref());
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(counts.get(), (0, 0));
        assert_eq!(events.get(), 1);
        handle
            .update(cx, |view, window, cx| {
                view.replace_text_in_range(None, "b", window, cx)
            })
            .unwrap();
        assert_eq!(session.borrow().history_depths(), (1, 0));
        assert_eq!(events.get(), 1, "successful input stays silent");
        session.borrow_mut().undo().unwrap();
        assert!(
            session
                .borrow()
                .document()
                .store()
                .iter()
                .filter_map(|node| node.content().as_inline())
                .all(|inline| inline.len_bytes() == 0)
        );
    }
}

#[gpui::test]
fn successful_platform_range_notifies_once_and_undo_restores_original_caret(
    cx: &mut TestAppContext,
) {
    let (handle, session, counts) = open(cx, false);
    let document = session.borrow().document().clone();
    let before = session.borrow().selection();
    handle
        .update(cx, |view, window, cx| {
            view.replace_text_in_range(Some(0..1), "QZ", window, cx)
        })
        .unwrap();
    assert_eq!(counts.get(), (1, 0));
    assert_eq!(
        session
            .borrow()
            .selection()
            .as_single_node()
            .unwrap()
            .focus()
            .offset()
            .as_usize(),
        2
    );
    assert_eq!(session.borrow().history_depths(), (2, 0));
    assert_eq!(session.borrow().stored_marks(), None);
    session.borrow_mut().undo().unwrap();
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), before);
    assert!(session.borrow().selection().is_collapsed());
    assert_eq!(counts.get(), (2, 0));
    session.borrow_mut().redo().unwrap();
    assert_eq!(
        session
            .borrow()
            .selection()
            .as_single_node()
            .unwrap()
            .focus()
            .offset()
            .as_usize(),
        2
    );
}
