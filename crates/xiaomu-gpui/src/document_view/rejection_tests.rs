//! Composition guards stay silent and actual failures do not invalidate layout.
use super::*;
use crate::{
    block_view::ClipboardPaste,
    editor::{EditorHooks, EditorInstance},
};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::InlinePoint,
};
use xiaomu_runtime::session::{
    DocumentSelection, EditIntent, IntentDisposition, PolicyError, SessionContext, SessionPolicy,
};

struct Reject;
impl SessionPolicy for Reject {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        Err(PolicyError::new("rejected"))
    }
}

#[gpui::test]
fn composition_guard_is_silent_and_intent_rejection_keeps_epoch(cx: &mut TestAppContext) {
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
    let editor = EditorInstance::new_with_policy(
        document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        EditorHooks::default(),
        Box::new(Reject),
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    let entity = handle
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            cx.entity()
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let events = Rc::new(Cell::new(0));
    let seen = events.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&entity, move |emitter, event: &EditorRejection, cx| {
            assert_eq!(event.stage(), EditorRejectionStage::Intent);
            assert_eq!(event.reason(), EditorRejectionReason::Policy);
            assert_eq!(
                event.document_revision(),
                xiaomu_core::document::DocumentRevision::INITIAL
            );
            assert_eq!(emitter.read(cx).session().borrow().history_depths(), (0, 0));
            seen.set(seen.get() + 1);
        })
    });
    // Even malformed native metadata is not inspected during composition.
    cx.update(|cx| {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
            "raw".into(),
            "xiaomu.clipboard.v14\n{".into(),
        ))
    });
    handle
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx)
            });
            let epoch = view.epoch.get();
            view.paste(&ClipboardPaste, window, cx);
            view.apply_edit_intent(EditIntent::SplitBlock, window, cx);
            assert_eq!(view.epoch.get(), epoch);
            child.update(cx, |child, cx| {
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
            });
        })
        .unwrap();
    assert_eq!(events.get(), 0);
    handle
        .update(cx, |view, window, cx| {
            let epoch = view.epoch.get();
            view.apply_edit_intent(EditIntent::SplitBlock, window, cx);
            assert_eq!(view.epoch.get(), epoch);
        })
        .unwrap();
    assert_eq!(events.get(), 1);
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().document().revision(), document.revision());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn metadata_rejection_stamp_precedes_success_in_one_outer_update(cx: &mut TestAppContext) {
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
    let rejected_revision = document.revision();
    let editor = EditorInstance::new(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(node)),
        EditorHooks::default(),
    )
    .unwrap();
    let session = editor.session().clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    let entity = window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
            cx.entity()
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let events = Rc::new(Cell::new(0));
    let _subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&entity, move |emitter, event: &EditorRejection, cx| {
            assert_eq!(event.stage(), EditorRejectionStage::ClipboardPaste);
            assert_eq!(event.reason(), EditorRejectionReason::ClipboardMetadata);
            assert_eq!(event.document_revision(), rejected_revision);
            assert_ne!(
                event.document_revision(),
                emitter.read(cx).session().borrow().document().revision()
            );
            events.set(events.get() + 1);
        })
    });
    window
        .update(cx, |view, window, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
                "private payload".into(),
                "xiaomu.clipboard.v14\n{".into(),
            ));
            view.paste(&ClipboardPaste, window, cx);
            assert_eq!(session.borrow().document().revision(), rejected_revision);
            assert_eq!(events.get(), 0);
            view.apply_edit_intent(
                EditIntent::InsertText {
                    text: "successful later edit".into(),
                },
                window,
                cx,
            );
            assert_eq!(events.get(), 0);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    assert_eq!(events.get(), 1);
    assert_eq!(session.borrow().history_depths(), (1, 0));
}
