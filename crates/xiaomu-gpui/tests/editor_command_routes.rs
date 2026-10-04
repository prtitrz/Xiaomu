//! Public, mounted-editor command routing through actual native action bindings.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{AppContext as _, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_gpui::{
    block_view::SharedSession,
    document_view::DocumentView,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
    editor_commands::{CommandRoute, EditorCommand, EditorCommandContext, EditorCommandRouter},
};
use xiaomu_runtime::{
    clipboard::{ClipboardSlice, normalize_multiline_paste_text},
    session::{
        DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, PolicyError,
    },
};

#[path = "editor_commands/defaults.rs"]
mod defaults;
#[path = "editor_commands/paste.rs"]
mod paste;
#[path = "editor_commands/rejection.rs"]
mod rejection;

#[derive(Clone, Copy)]
enum Decision {
    Default,
    TwoSpaces,
    Multiline,
    NoChange,
    Reject,
    RejectedCandidate,
}

#[derive(Debug, PartialEq, Eq)]
enum Gesture {
    Tab(bool),
    Paste(String),
}

struct Observed {
    command: Gesture,
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
}

struct Router {
    decision: Decision,
    observed: RefCell<Vec<Observed>>,
}

impl Router {
    fn new(decision: Decision) -> Rc<Self> {
        Rc::new(Self {
            decision,
            observed: RefCell::new(Vec::new()),
        })
    }
}

impl EditorCommandRouter for Router {
    fn route(
        &self,
        context: EditorCommandContext<'_>,
        command: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        // Recording is test instrumentation; no editor/session is borrowed here.
        self.observed.borrow_mut().push(Observed {
            command: match command {
                EditorCommand::Tab { reverse } => Gesture::Tab(reverse),
                EditorCommand::PlainTextPaste(text) => Gesture::Paste(text.into()),
            },
            document: context.document().clone(),
            selection: context.selection(),
            marks: context.stored_marks().cloned(),
        });
        Ok(match (self.decision, command) {
            (Decision::Reject, _) => return Err(PolicyError::new("route rejected")),
            (Decision::NoChange, _)
            | (Decision::TwoSpaces, EditorCommand::Tab { reverse: true }) => CommandRoute::NoChange,
            (Decision::RejectedCandidate, _) => {
                CommandRoute::Intent(EditIntent::PasteText { text: "!".into() })
            }
            (Decision::TwoSpaces, EditorCommand::Tab { reverse: false }) => {
                CommandRoute::Intent(EditIntent::PasteText { text: "  ".into() })
            }
            (Decision::Multiline, EditorCommand::PlainTextPaste(text)) => {
                CommandRoute::Intent(EditIntent::PasteSlice {
                    slice: multiline_slice(text),
                })
            }
            _ => CommandRoute::Default,
        })
    }
}

fn inline(builder: &mut NodeStoreBuilder, kind: NodeKind, text: &str) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(if text.is_empty() {
                InlineContent::empty()
            } else {
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
            }),
        )
        .unwrap()
}

fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: &[NodeId]) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::children(children.iter().copied()),
        )
        .unwrap()
}

fn finish(mut builder: NodeStoreBuilder, children: &[NodeId]) -> XiaomuDocument {
    let root = container(&mut builder, NodeKind::Document, children);
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

fn single(kind: NodeKind, text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let node = inline(&mut builder, kind, text);
    (finish(builder, &[node]), node)
}

fn point(document: &XiaomuDocument, node: NodeId, byte: usize) -> InlinePoint {
    InlinePoint::new(
        node,
        document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(byte)
            .unwrap(),
        0,
        CursorAffinity::Before,
    )
}

fn caret(document: &XiaomuDocument, node: NodeId, byte: usize) -> DocumentSelection {
    DocumentSelection::collapsed(point(document, node, byte))
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    document
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

fn children(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

fn multiline_slice(raw: &str) -> ClipboardSlice {
    let normalized = normalize_multiline_paste_text(raw);
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = normalized
        .split('\n')
        .map(|line| inline(&mut builder, NodeKind::Paragraph, line))
        .collect();
    let document = finish(builder, &nodes);
    let last = *nodes.last().unwrap();
    let selection = DocumentSelection::new(
        point(&document, nodes[0], 0),
        point(&document, last, text(&document, last).len()),
    );
    DocumentSession::new(document, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

fn mount(
    editor: EditorInstance,
    cx: &mut TestAppContext,
) -> (WindowHandle<DocumentView>, SharedSession) {
    let session = editor.session().clone();
    cx.update(bind_default_editor_keys);
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    focus(window, cx);
    (window, session)
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

fn press(window: WindowHandle<DocumentView>, keys: &str, cx: &mut TestAppContext) {
    cx.simulate_keystrokes(window.into(), keys);
    cx.background_executor.run_until_parked();
}

fn paste_text(window: WindowHandle<DocumentView>, raw: &str, cx: &mut TestAppContext) {
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(raw.into())));
    press(window, "ctrl-v", cx);
}

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

fn listen(session: &SharedSession) -> Counts {
    let counts = Rc::new(Cell::new((0, 0)));
    session
        .borrow_mut()
        .add_listener(Box::new(Listener(counts.clone())));
    counts
}

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    history: (usize, usize),
    counts: (usize, usize),
}

impl Snapshot {
    fn capture(session: &SharedSession, counts: &Counts) -> Self {
        let session = session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            history: session.history_depths(),
            counts: counts.get(),
        }
    }
    fn assert_unchanged(&self, session: &SharedSession, counts: &Counts) {
        let session = session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.document().root(), self.document.root());
        assert_eq!(session.document().version(), self.document.version());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.history);
        assert_eq!(counts.get(), self.counts);
    }
}
