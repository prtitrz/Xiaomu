//! Real GPUI virtual dispatch and retained-handler evidence, not OS/IME acceptance.

use super::super::*;
use crate::{
    block_view::{ClipboardCut, ClipboardPaste, SharedSession},
    editor::{
        EditorHooks, EditorInstance, bind_default_editor_keys, bind_primary_modifier_enter_keys,
    },
    editor_commands::{
        CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter, EnterSource,
    },
};
use gpui::{AppContext as _, ClipboardItem, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::{
    document::{
        InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    },
    selection::CursorAffinity,
};
use xiaomu_runtime::{
    assets::{AssetError, AssetFormat, AssetRef, AssetService, AssetSink},
    clipboard::{ClipboardExportPurpose, ClipboardExportSpec},
    persistence::{DocumentPersistence, PersistenceError},
    session::{
        DocumentChangeListener, DocumentSelection, EditIntent, EditPlan, IntentDisposition,
        PolicyError, SessionContext, SessionPolicy,
    },
};

#[path = "read_only_input_reading_tests.rs"]
mod reading;
#[path = "read_only_input_surfaces_tests.rs"]
mod surfaces;

#[derive(Default)]
struct Calls {
    intent: Cell<usize>,
    cut: Cell<usize>,
    export_cut: Cell<usize>,
    changes: Cell<usize>,
    imports: Cell<usize>,
    saves: Cell<usize>,
    router: Cell<usize>,
    clock: Cell<usize>,
}
struct Probe(Rc<Calls>);
impl crate::history_clock::HistoryClock for Probe {
    fn now(&self) -> xiaomu_runtime::session::HistoryTimestamp {
        self.0.clock.set(self.0.clock.get() + 1);
        xiaomu_runtime::session::HistoryTimestamp::from_millis(self.0.clock.get() as u64)
    }
}
impl SessionPolicy for Probe {
    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        _: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        self.0.intent.set(self.0.intent.get() + 1);
        Ok(IntentDisposition::Continue)
    }
    fn prepare_cut(&self, _: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        self.0.cut.set(self.0.cut.get() + 1);
        Ok(None)
    }
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        if purpose == ClipboardExportPurpose::Cut {
            self.0.export_cut.set(self.0.export_cut.get() + 1);
        }
        Ok(None)
    }
}
impl DocumentChangeListener for Probe {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.changes.set(self.0.changes.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.changes.set(self.0.changes.get() + 1);
    }
}
impl AssetService for Probe {
    fn resolve(&self, _: AssetRef, sink: Rc<dyn AssetSink>) {
        sink.resolved(Err(AssetError::NotFound));
    }
    fn import_image(
        &self,
        _: AssetFormat,
        _: &[u8],
    ) -> Result<xiaomu_core::document::ImageAttrs, AssetError> {
        self.0.imports.set(self.0.imports.get() + 1);
        Err(AssetError::InvalidImage)
    }
}
impl DocumentPersistence for Probe {
    fn save(&mut self, _: &XiaomuDocument) -> Result<(), PersistenceError> {
        self.0.saves.set(self.0.saves.get() + 1);
        Ok(())
    }
    fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError> {
        Ok(None)
    }
}
impl EditorCommandRouter for Probe {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        self.0.router.set(self.0.router.get() + 1);
        Ok(CommandRoute::Default)
    }
    fn route_enter(
        &self,
        _: EditorCommandContext<'_>,
        _: EnterSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.0.router.set(self.0.router.get() + 1);
        Ok(CommandRoute::Default)
    }
}

fn document(count: usize) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let blocks: Vec<_> = (0..count)
        .map(|_| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(
                        InlineContent::new([
                            TextRun::new("hello 中🙂 world", MarkSet::empty()).unwrap()
                        ])
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
            NodeContent::children(blocks.clone()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), blocks)
}

fn instance(
    document: XiaomuDocument,
    selection: DocumentSelection,
    read_only: bool,
    calls: Rc<Calls>,
) -> EditorInstance {
    let editor = EditorInstance::new_with_policy_and_history_clock(
        document,
        selection,
        EditorHooks {
            listener: Some(Box::new(Probe(calls.clone()))),
            asset_service: Some(Rc::new(Probe(calls.clone()))),
            persistence: Some(Rc::new(RefCell::new(Probe(calls.clone())))),
            ..Default::default()
        },
        Box::new(Probe(calls.clone())),
        Rc::new(Probe(calls.clone())),
    )
    .unwrap()
    .with_command_router(Rc::new(Probe(calls)));
    if read_only {
        editor.with_read_only_input()
    } else {
        editor
    }
}

fn open(editor: &EditorInstance, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    let handle = cx.update(|cx| {
        bind_default_editor_keys(cx);
        bind_primary_modifier_enter_keys(cx);
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
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

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    changes: usize,
    intents: usize,
    clock: usize,
}
impl Snapshot {
    fn capture(session: &SharedSession, calls: &Calls) -> Self {
        let session = session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            changes: calls.changes.get(),
            intents: calls.intent.get(),
            clock: calls.clock.get(),
        }
    }
    fn assert_unchanged(&self, session: &SharedSession, calls: &Calls) {
        let session = session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(calls.changes.get(), self.changes);
        assert_eq!(calls.intent.get(), self.intents);
        assert_eq!(calls.clock.get(), self.clock);
    }
}

fn exercise_native(
    input: &Entity<ParagraphView>,
    window: &mut Window,
    cx: &mut Context<DocumentView>,
) {
    input.update(cx, |input, cx| {
        assert!(input.selected_text_range(false, window, cx).is_none());
        assert!(input.selected_text_range(true, window, cx).is_some());
        for range in [None, Some(0..3)] {
            input.replace_text_in_range(range.clone(), "typed", window, cx);
            input.replace_and_mark_text_in_range(range, "预编辑🙂", Some(1..2), window, cx);
            assert!(!input.is_composing());
            assert_eq!(input.marked_text_range(window, cx), None);
            input.unmark_text(window, cx);
            input.replace_text_in_range(None, "提交", window, cx);
            input.replace_and_mark_text_in_range(None, "", None, window, cx);
            input.replace_text_in_range(None, "", window, cx);
            assert!(!input.is_composing());
        }
    });
}

#[gpui::test]
fn read_only_native_and_edit_commands_are_rejected_before_state_or_host_callbacks(
    cx: &mut TestAppContext,
) {
    let (document, blocks) = document(2);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    session
        .borrow_mut()
        .apply_intent(&EditIntent::InsertText {
            text: "host".into(),
        })
        .unwrap();
    session
        .borrow_mut()
        .apply_intent(&EditIntent::SetMark { mark: Mark::Bold })
        .unwrap();
    let handle = open(&editor, cx);
    let before = Snapshot::capture(&session, &calls);
    handle
        .update(cx, |view, window, cx| {
            assert!(view.is_read_only_input());
            let epoch = view.epoch.get();
            exercise_native(&view.children[0].1.clone(), window, cx);
            view.apply_edit_intent(EditIntent::SplitBlock, window, cx);
            view.apply_edit_intent_with_selection(
                DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[1])),
                EditIntent::InsertText {
                    text: "toolbar".into(),
                },
                window,
                cx,
            );
            assert_eq!(view.epoch.get(), epoch);
        })
        .unwrap();
    cx.simulate_input(handle.into(), "platform typing");
    for key in [
        "backspace",
        "delete",
        "enter",
        "shift-enter",
        "ctrl-enter",
        "tab",
        "shift-tab",
        "ctrl-b",
        "ctrl-i",
        "ctrl-u",
        "ctrl-e",
        "ctrl-shift-x",
        "ctrl-z",
        "ctrl-shift-z",
        "ctrl-y",
        "ctrl-s",
    ] {
        cx.simulate_keystrokes(handle.into(), key);
        before.assert_unchanged(&session, &calls);
    }
    assert_eq!(calls.router.get(), 0);
    assert_eq!(calls.saves.get(), 0);
    // A populated redo stack must be protected too, not just an empty no-op.
    session.borrow_mut().undo().unwrap();
    let before_redo = Snapshot::capture(&session, &calls);
    cx.simulate_keystrokes(handle.into(), "ctrl-shift-z");
    before_redo.assert_unchanged(&session, &calls);
}

#[gpui::test]
fn read_only_cut_and_paste_preserve_clipboard_and_never_import(cx: &mut TestAppContext) {
    let (document, blocks) = document(1);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document.clone(),
        DocumentSelection::all(&document),
        true,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    let before = Snapshot::capture(&session, &calls);
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
            "keep me".into(),
            "keep metadata".into(),
        ))
    });
    cx.simulate_keystrokes(handle.into(), "ctrl-x");
    handle
        .update(cx, |view, window, cx| view.cut(&ClipboardCut, window, cx))
        .unwrap();
    cx.update(|cx| {
        let item = cx.read_from_clipboard().unwrap();
        assert_eq!(item.text().as_deref(), Some("keep me"));
        assert_eq!(item.metadata().map(String::as_str), Some("keep metadata"));
    });
    assert_eq!(calls.cut.get(), 0);
    assert_eq!(calls.export_cut.get(), 0);
    before.assert_unchanged(&session, &calls);
    // Plain, valid structured and malformed metadata all stop before routing.
    let slice = session.borrow().clipboard_slice().unwrap().unwrap();
    let metadata = xiaomu_runtime::clipboard::encode_metadata(&slice).unwrap();
    for item in [
        ClipboardItem::new_string("paste\ntext".into()),
        ClipboardItem::new_string_with_metadata(slice.plain_text().into(), metadata),
        ClipboardItem::new_string_with_metadata(
            "fallback".into(),
            "xiaomu.clipboard.v14\ninvalid".into(),
        ),
    ] {
        cx.update(|cx| cx.write_to_clipboard(item));
        cx.simulate_keystrokes(handle.into(), "ctrl-v");
        before.assert_unchanged(&session, &calls);
    }
    session
        .borrow_mut()
        .set_inline_selection(
            InlinePoint::at_start_of(blocks[0]),
            InlinePoint::at_start_of(blocks[0]),
        )
        .unwrap();
    handle
        .update(cx, |view, window, cx| view.focus_selection(window, cx))
        .unwrap();
    let before_image = Snapshot::capture(&session, &calls);
    for format in [gpui::ImageFormat::Png, gpui::ImageFormat::Jpeg] {
        cx.update(|cx| {
            cx.write_to_clipboard(ClipboardItem::new_image(&gpui::Image::from_bytes(
                format,
                vec![1, 2, 3],
            )))
        });
        cx.simulate_keystrokes(handle.into(), "ctrl-v");
        handle
            .update(cx, |view, window, cx| {
                view.paste(&ClipboardPaste, window, cx)
            })
            .unwrap();
        before_image.assert_unchanged(&session, &calls);
    }
    assert_eq!(calls.imports.get(), 0);
    assert_eq!(calls.router.get(), 0);
}

#[gpui::test]
fn default_editable_still_types_composes_cuts_imports_and_saves(cx: &mut TestAppContext) {
    let (document, blocks) = document(1);
    let calls = Rc::new(Calls::default());
    let editor = instance(
        document,
        DocumentSelection::collapsed(InlinePoint::at_start_of(blocks[0])),
        false,
        calls.clone(),
    );
    let session = editor.session().clone();
    let handle = open(&editor, cx);
    cx.simulate_input(handle.into(), "X");
    assert_eq!(session.borrow().history_depths(), (1, 0));
    handle
        .update(cx, |view, window, cx| {
            assert!(!view.is_read_only_input());
            view.children[0].1.clone().update(cx, |input, cx| {
                assert!(input.selected_text_range(false, window, cx).is_some());
                input.replace_and_mark_text_in_range(None, "预编辑", None, window, cx);
                assert!(input.is_composing());
                input.replace_text_in_range(None, "提交", window, cx);
                assert!(!input.is_composing());
            });
        })
        .unwrap();
    assert_eq!(session.borrow().history_depths(), (2, 0));
    cx.simulate_keystrokes(handle.into(), "ctrl-a");
    cx.simulate_keystrokes(handle.into(), "ctrl-x");
    cx.update(|cx| {
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .contains("提交")
        )
    });
    cx.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            vec![1, 2, 3],
        )))
    });
    cx.simulate_keystrokes(handle.into(), "ctrl-v");
    cx.simulate_keystrokes(handle.into(), "ctrl-s");
    assert_eq!(calls.imports.get(), 1);
    assert_eq!(calls.saves.get(), 1);
    assert!(calls.cut.get() > 0);
}
