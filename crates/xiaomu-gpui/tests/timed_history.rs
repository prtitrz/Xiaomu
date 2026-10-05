//! Deterministic public GPUI clock wiring. No sleeps or native event loop.
//!
//! Mounted tests use TestAppContext::simulate_input, which emits one native
//! insertion per scalar. The callbacks module separately covers one actual
//! multi-scalar EntityInputHandler callback and composition's isolated path.

#[path = "timed_history/callbacks.rs"]
mod callbacks;
#[path = "timed_history/mounted.rs"]
mod mounted;
#[path = "timed_history/sharing.rs"]
mod sharing;

use std::{cell::Cell, rc::Rc};

use gpui::{AppContext as _, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    XiaomuDocument,
};
use xiaomu_core::selection::TextPoint;
use xiaomu_gpui::{
    block_view::SharedSession,
    document_view::DocumentView,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
    history_clock::HistoryClock,
};
use xiaomu_runtime::session::{
    DefaultTextInputMarks, DocumentSelection, EditIntent, HistoryOptions, HistoryTimestamp,
    SelectionOnlyGrouping, SessionPolicy,
};

#[derive(Default)]
struct ManualClock {
    millis: Cell<u64>,
    samples: Cell<usize>,
}

impl ManualClock {
    fn set(&self, millis: u64) {
        self.millis.set(millis);
    }

    fn samples(&self) -> usize {
        self.samples.get()
    }
}

impl HistoryClock for ManualClock {
    fn now(&self) -> HistoryTimestamp {
        self.samples.set(self.samples.get() + 1);
        HistoryTimestamp::from_millis(self.millis.get())
    }
}

struct Options {
    history: HistoryOptions,
    marks: DefaultTextInputMarks,
}

impl SessionPolicy for Options {
    fn history_options(&self) -> HistoryOptions {
        self.history
    }

    fn default_text_input_marks(&self) -> DefaultTextInputMarks {
        self.marks
    }
}

fn timed() -> HistoryOptions {
    HistoryOptions::new()
        .with_typing_group_delay_ms(500)
        .with_selection_only_grouping(SelectionOnlyGrouping::Preserve)
}

fn fixture() -> (XiaomuDocument, NodeId, DocumentSelection) {
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
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        node,
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
    )
}

fn editor_with(
    clock: Rc<ManualClock>,
    history: HistoryOptions,
    marks: DefaultTextInputMarks,
) -> (EditorInstance, NodeId) {
    let (document, node, selection) = fixture();
    let editor = EditorInstance::new_with_policy_and_history_clock(
        document,
        selection,
        EditorHooks::default(),
        Box::new(Options { history, marks }),
        clock,
    )
    .unwrap();
    (editor, node)
}

fn editor(clock: Rc<ManualClock>) -> (EditorInstance, NodeId) {
    editor_with(clock, timed(), DefaultTextInputMarks::PreservePending)
}

fn mount_view(view: DocumentView, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| view))
            .unwrap()
    });
    focus(window, cx);
    window
}

fn mount(editor: &EditorInstance, cx: &mut TestAppContext) -> WindowHandle<DocumentView> {
    mount_view(editor.build_view(), cx)
}

fn focus(window: WindowHandle<DocumentView>, cx: &mut TestAppContext) {
    window
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
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

fn round_trip(
    window: WindowHandle<DocumentView>,
    session: &SharedSession,
    node: NodeId,
    states: &[&str],
    cx: &mut TestAppContext,
) {
    let document = session.borrow().document().clone();
    let selection = session.borrow().selection();
    let total = states.len() - 1;
    assert_eq!(text(session, node), states[total]);
    assert_eq!(session.borrow().history_depths(), (total, 0));
    for index in (0..total).rev() {
        cx.simulate_keystrokes(window.into(), "ctrl-z");
        assert_eq!(text(session, node), states[index]);
        assert_eq!(session.borrow().history_depths(), (index, total - index));
    }
    for (index, expected) in states.iter().enumerate().skip(1) {
        cx.simulate_keystrokes(window.into(), "ctrl-shift-z");
        assert_eq!(text(session, node), *expected);
        assert_eq!(session.borrow().history_depths(), (index, total - index));
    }
    assert_eq!(session.borrow().document().store(), document.store());
    assert_eq!(session.borrow().selection(), selection);
}
