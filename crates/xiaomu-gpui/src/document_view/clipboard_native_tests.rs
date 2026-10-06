//! Native v14 rejection reaches the real paste action without fallback edits.

use crate::block_view::{ClipboardPaste, SharedSession};
use crate::document_view::DocumentView;
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use gpui::{AppContext as _, TestAppContext, WindowHandle};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::InlinePoint;
use xiaomu_runtime::clipboard::{
    ClipboardExportPurpose, ClipboardExportSpec, ClipboardTextProjection, encode_metadata,
};
use xiaomu_runtime::session::{
    DocumentSelection, DocumentSession, EditIntent, IntentDisposition, PolicyError, SessionContext,
    SessionPolicy,
};

struct Policy(Rc<Cell<usize>>);
impl SessionPolicy for Policy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        _: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        Ok(Some(ClipboardExportSpec::new().with_text_projection(
            ClipboardTextProjection::TextBetweenLfV1,
        )))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.0.set(self.0.get() + 1);
        Ok(IntentDisposition::Continue)
    }
}

fn document(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

fn valid_metadata() -> String {
    let (document, _) = document("native");
    let all = DocumentSelection::all(&document);
    let session =
        DocumentSession::new_with_policy(document, all, Box::new(Policy(Rc::new(Cell::new(0)))))
            .unwrap();
    let metadata = encode_metadata(&session.clipboard_slice().unwrap().unwrap()).unwrap();
    assert!(metadata.starts_with("xiaomu.clipboard.v14\n"));
    assert!(metadata.contains("\"version\":14"));
    metadata
}

fn mount(cx: &mut TestAppContext) -> (WindowHandle<DocumentView>, SharedSession, Rc<Cell<usize>>) {
    let (document, paragraph) = document("target");
    let calls = Rc::new(Cell::new(0));
    let editor = EditorInstance::new_with_policy(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(paragraph)),
        EditorHooks::default(),
        Box::new(Policy(calls.clone())),
    )
    .unwrap();
    let session = editor.session().clone();
    {
        let mut session = session.borrow_mut();
        session
            .apply_intent(&EditIntent::InsertText {
                text: "history".into(),
            })
            .unwrap();
        session.undo().unwrap();
        session
            .apply_intent(&EditIntent::ToggleMark { mark: Mark::Italic })
            .unwrap();
    }
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view: &mut DocumentView, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    (window, session, calls)
}

#[gpui::test]
fn malformed_native_paste_never_reaches_edit_policy_or_plain_text(cx: &mut TestAppContext) {
    let metadata = valid_metadata();
    // Still valid JSON after the frame, but beyond the metadata size budget.
    let oversized = format!("{metadata}{}", " ".repeat(16 * 1024 * 1024));
    let malformed = [
        ("native", "xiaomu.clipboard.v14\n{".into()),
        (
            "native",
            metadata.replacen("\"version\":14", "\"version\":14,\"version\":14", 1),
        ),
        (
            "native",
            metadata.replacen("\"version\":14", "\"version\":14,\"\\u0076ersion\":14", 1),
        ),
        (
            "native",
            metadata.replacen("\"version\":14", "\"version\":999", 1),
        ),
        (
            "native",
            metadata.replacen("\"version\":14", "\"\\u0076ersion\":999", 1),
        ),
        (
            "native",
            metadata.replacen("xiaomu.clipboard.v14\n", "xiaomu.clipboard.v999\n", 1),
        ),
        ("different", metadata),
        ("native", oversized),
    ];
    let (window, session, calls) = mount(cx);
    let before = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let marks = session.borrow().stored_marks().cloned();
    assert!(marks.is_some());
    let history = session.borrow().history_depths();
    let before_calls = calls.get();
    for (text, metadata) in malformed {
        let item = gpui::ClipboardItem::new_string_with_metadata(text.into(), metadata);
        cx.update(|cx| cx.write_to_clipboard(item.clone()));
        // Use native keyboard dispatch as well as the real action entry point.
        cx.simulate_keystrokes(window.into(), "ctrl-v");
        window
            .update(cx, |view, window, cx| {
                view.paste(&ClipboardPaste, window, cx)
            })
            .unwrap();
        let session = session.borrow();
        assert_eq!(session.document().store(), before.store());
        assert_eq!(session.document().revision(), before.revision());
        assert_eq!(session.selection(), selection);
        assert_eq!(session.stored_marks(), marks.as_ref());
        assert_eq!(session.history_depths(), history);
        assert_eq!(calls.get(), before_calls);
        assert_eq!(cx.update(|cx| cx.read_from_clipboard().unwrap()), item);
    }
}

#[gpui::test]
fn foreign_and_legacy_invalid_metadata_still_paste_plain_text(cx: &mut TestAppContext) {
    for metadata in [
        "foreign clipboard metadata",
        r#"{"format":"xiaomu.clipboard","version":13,"roots":[]}"#,
    ] {
        let (window, session, calls) = mount(cx);
        let history = session.borrow().history_depths();
        let before_calls = calls.get();
        cx.update(|cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
                "fallback".into(),
                metadata.into(),
            ))
        });
        cx.simulate_keystrokes(window.into(), "ctrl-v");
        assert_eq!(calls.get(), before_calls + 1);
        assert_eq!(session.borrow().history_depths(), (history.0 + 1, 0));
        let session = session.borrow();
        let point = session.selection().as_single_node().unwrap().focus();
        let inline = session
            .document()
            .node(point.node_id())
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        assert_eq!(
            inline
                .runs()
                .iter()
                .map(|run| run.text().as_str())
                .collect::<String>(),
            "fallbacktarget"
        );
    }
}
