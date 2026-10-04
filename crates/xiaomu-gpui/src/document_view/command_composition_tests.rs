//! Native composition remains ahead of optional host command callbacks.

use super::DocumentView;
use crate::{
    block_view::{ClipboardPaste, ShiftTabIndent, TabIndent},
    editor::{
        EditorHooks, EditorInstance, bind_default_editor_keys, bind_primary_modifier_enter_keys,
    },
    editor_commands::{
        CodePasteSource, CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter,
        EnterSource,
    },
};
use gpui::{AppContext as _, EntityInputHandler, TestAppContext};
use std::{cell::Cell, rc::Rc};
use xiaomu_core::{
    document::{InlineContent, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument},
    selection::TextPoint,
};
use xiaomu_runtime::session::{DocumentSelection, PolicyError};

struct CountCalls(Rc<Cell<usize>>);
impl EditorCommandRouter for CountCalls {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        self.0.set(self.0.get() + 1);
        Ok(CommandRoute::NoChange)
    }

    fn route_enter(
        &self,
        _: EditorCommandContext<'_>,
        _: EnterSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.0.set(self.0.get() + 1);
        Ok(CommandRoute::NoChange)
    }

    fn route_arrow_down(
        &self,
        context: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.0.set(self.0.get() + 1);
        Ok(Some(context.selection()))
    }

    fn route_code_paste(
        &self,
        _: EditorCommandContext<'_>,
        _: &str,
        _: CodePasteSource,
    ) -> Result<CommandRoute, PolicyError> {
        self.0.set(self.0.get() + 1);
        Ok(CommandRoute::NoChange)
    }
}

#[gpui::test]
fn composing_tab_reverse_tab_and_plain_paste_never_reach_router(cx: &mut TestAppContext) {
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
    let selection = DocumentSelection::collapsed(TextPoint::at_start_of(node));
    let calls = Rc::new(Cell::new(0));
    let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
        .unwrap()
        .with_command_router(Rc::new(CountCalls(calls.clone())));
    let session = editor.session().clone();
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
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\r\nb".into())));
    handle
        .update(cx, |view, window, cx| {
            let child = view.children[0].1.clone();
            child.update(cx, |child, cx| {
                child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
            });
            let epoch = view.epoch.get();
            view.tab_indent(&TabIndent, window, cx);
            view.shift_tab_indent(&ShiftTabIndent, window, cx);
            view.paste(&ClipboardPaste, window, cx);
            assert_eq!(calls.get(), 0);
            assert_eq!(view.epoch.get(), epoch);
            assert_eq!(session.borrow().document().store(), document.store());
            assert_eq!(session.borrow().selection(), selection);
            assert_eq!(session.borrow().history_depths(), (0, 0));
            child.update(cx, |child, cx| {
                assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                child.replace_and_mark_text_in_range(None, "", None, window, cx);
            });
            view.tab_indent(&TabIndent, window, cx);
            view.shift_tab_indent(&ShiftTabIndent, window, cx);
            view.paste(&ClipboardPaste, window, cx);
            assert_eq!(calls.get(), 3);
        })
        .unwrap();
}

fn code_command_document(
    kind: NodeKind,
    text: &str,
) -> (XiaomuDocument, xiaomu_core::document::NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let inline = if text.is_empty() {
        InlineContent::empty()
    } else {
        InlineContent::new([xiaomu_core::document::TextRun::new(text, Default::default()).unwrap()])
            .unwrap()
    };
    let node = builder
        .insert(kind, NodeAttrs::empty(), NodeContent::Inline(inline))
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([node]),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), node)
}

#[gpui::test]
fn composing_real_enter_and_code_paste_keys_never_reach_hooks(cx: &mut TestAppContext) {
    use xiaomu_core::document::{Mark, MarkSet};
    use xiaomu_runtime::session::DocumentSession;

    for kind in [NodeKind::CodeBlock, NodeKind::Paragraph] {
        let (document, node) = code_command_document(kind, "");
        let selection = DocumentSelection::collapsed(TextPoint::at_start_of(node));
        let calls = Rc::new(Cell::new(0));
        let editor = EditorInstance::new(document.clone(), selection, EditorHooks::default())
            .unwrap()
            .with_command_router(Rc::new(CountCalls(calls.clone())));
        let session = editor.session().clone();
        let handle = cx.update(|cx| {
            bind_default_editor_keys(cx);
            bind_primary_modifier_enter_keys(cx);
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
        cx.simulate_keystrokes(handle.into(), "ctrl-b");
        let (child, epoch) = handle
            .update(cx, |view, window, cx| {
                let child = view.children[0].1.clone();
                child.update(cx, |child, cx| {
                    child.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                    assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                });
                (child, view.epoch.get())
            })
            .unwrap();
        cx.background_executor.run_until_parked();
        cx.simulate_keystrokes(handle.into(), "enter shift-enter ctrl-enter cmd-enter down");
        cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\r\nb\rc".into())));
        cx.simulate_keystrokes(handle.into(), "ctrl-v");
        for closed in [false, true] {
            let (source, source_node) = code_command_document(NodeKind::Paragraph, "a\r\nb\rc");
            let source_selection = if closed {
                DocumentSelection::all(&source)
            } else {
                let end = source
                    .node(source_node)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(6)
                    .unwrap();
                DocumentSelection::new(
                    TextPoint::at_start_of(source_node),
                    TextPoint::new(
                        source_node,
                        end,
                        xiaomu_core::selection::CursorAffinity::Before,
                    ),
                )
            };
            let slice = DocumentSession::new(source, source_selection)
                .unwrap()
                .clipboard_slice()
                .unwrap()
                .unwrap();
            assert_eq!(slice.is_closed(), closed);
            cx.update(|cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
                    slice.plain_text().into(),
                    xiaomu_runtime::clipboard::encode_metadata(&slice).unwrap(),
                ))
            });
            cx.simulate_keystrokes(handle.into(), "ctrl-v");
        }
        handle
            .update(cx, |view, window, cx| {
                assert_eq!(calls.get(), 0);
                assert_eq!(view.epoch.get(), epoch);
                assert_eq!(session.borrow().document().store(), document.store());
                assert_eq!(session.borrow().selection(), selection);
                assert_eq!(session.borrow().history_depths(), (0, 0));
                assert_eq!(
                    session.borrow().stored_marks(),
                    Some(&MarkSet::new([Mark::Bold]).unwrap())
                );
                child.update(cx, |child, cx| {
                    assert_eq!(child.marked_text_range(window, cx), Some(0..2));
                    child.replace_and_mark_text_in_range(None, "", None, window, cx);
                });
            })
            .unwrap();
        cx.simulate_keystrokes(handle.into(), "enter shift-enter ctrl-enter cmd-enter down");
        cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("a\r\nb\rc".into())));
        cx.simulate_keystrokes(handle.into(), "ctrl-v");
        assert_eq!(
            calls.get(),
            6,
            "the real keys route normally after composition clears"
        );
        handle
            .update(cx, |_, window, cx| {
                child.update(cx, |child, cx| {
                    child.replace_text_in_range(None, "你", window, cx)
                });
            })
            .unwrap();
        assert_eq!(calls.get(), 6, "IME commit never enters command hooks");
    }
}
