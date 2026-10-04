//! Native composition remains ahead of optional host command callbacks.

use super::DocumentView;
use crate::{
    block_view::{ClipboardPaste, ShiftTabIndent, TabIndent},
    editor::{EditorHooks, EditorInstance},
    editor_commands::{CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter},
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
