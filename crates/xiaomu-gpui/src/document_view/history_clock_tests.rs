//! Deterministic virtual-platform coverage; no OS input, sleep or real GUI claim.
//! Direct EntityInputHandler calls below are explicitly synthetic callback probes.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gpui::{AppContext as _, Entity, EntityInputHandler, TestAppContext, WindowHandle};
use xiaomu_core::document::{
    AttrValue, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, HistoryOptions,
    HistoryTimestamp, IntentDisposition, PolicyError, SelectionOnlyGrouping, SessionContext,
    SessionPolicy,
};

use super::DocumentView;
use crate::block_view::{ParagraphView, SharedSession};
use crate::editor::{EditorHooks, EditorInstance, bind_default_editor_keys};
use crate::history_clock::HistoryClock;

#[path = "history_clock_failures.rs"]
mod failures;
#[path = "history_clock_surfaces.rs"]
mod surfaces;

#[derive(Default)]
struct ManualClock {
    milliseconds: Cell<u64>,
    samples: Cell<usize>,
    session: RefCell<Option<Weak<RefCell<DocumentSession>>>>,
}

impl HistoryClock for ManualClock {
    fn now(&self) -> HistoryTimestamp {
        if let Some(session) = self.session.borrow().as_ref().and_then(Weak::upgrade) {
            assert!(
                session.try_borrow_mut().is_ok(),
                "clock sampled under session borrow"
            );
        }
        self.samples.set(self.samples.get() + 1);
        HistoryTimestamp::from_millis(self.milliseconds.get())
    }
}

struct TimedPolicy;
impl SessionPolicy for TimedPolicy {
    fn history_options(&self) -> HistoryOptions {
        HistoryOptions::new()
            .with_typing_group_delay_ms(500)
            .with_selection_only_grouping(SelectionOnlyGrouping::Preserve)
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        match intent {
            EditIntent::InsertText { text } if text == "!" => Err(PolicyError::new("preflight")),
            EditIntent::InsertText { text } if text == "?" => Ok(IntentDisposition::NoChange),
            _ => Ok(IntentDisposition::Continue),
        }
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document
            .store()
            .iter()
            .filter_map(|node| node.content().as_inline())
            .flat_map(|inline| inline.runs())
            .any(|run| run.text().as_str().contains('#'))
        {
            Err(PolicyError::new("candidate"))
        } else {
            Ok(())
        }
    }
}

type Counts = Rc<Cell<(usize, usize)>>;
struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (edits, selections) = self.0.get();
        self.0.set((edits + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (edits, selections) = self.0.get();
        self.0.set((edits, selections + 1));
    }
}

struct Fixture {
    document: XiaomuDocument,
    before: NodeId,
    nested: NodeId,
    cells: Vec<NodeId>,
    cell_texts: Vec<NodeId>,
    atomic: NodeId,
}

fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: Vec<NodeId>) -> NodeId {
    builder
        .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
        .unwrap()
}

fn paragraph(builder: &mut NodeStoreBuilder) -> NodeId {
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::empty()),
        )
        .unwrap()
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let before = paragraph(&mut builder);
    let nested = paragraph(&mut builder);
    let quote = container(&mut builder, NodeKind::Quote, vec![nested]);
    let mut cells = Vec::new();
    let mut cell_texts = Vec::new();
    for index in 0..3 {
        let text = paragraph(&mut builder);
        cell_texts.push(text);
        let attrs = if index == 0 {
            NodeAttrs::new([("colspan".into(), AttrValue::Integer(2))].into()).unwrap()
        } else {
            NodeAttrs::empty()
        };
        cells.push(
            builder
                .insert(
                    if index == 0 {
                        NodeKind::TableHeader
                    } else {
                        NodeKind::TableCell
                    },
                    attrs,
                    NodeContent::children([text]),
                )
                .unwrap(),
        );
    }
    let first = container(&mut builder, NodeKind::TableRow, vec![cells[0]]);
    let second = container(&mut builder, NodeKind::TableRow, cells[1..].to_vec());
    let table = container(&mut builder, NodeKind::Table, vec![first, second]);
    let atomic = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let root = container(
        &mut builder,
        NodeKind::Document,
        vec![before, quote, table, atomic],
    );
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        before,
        nested,
        cells,
        cell_texts,
        atomic,
    }
}

struct Opened {
    handle: WindowHandle<DocumentView>,
    session: SharedSession,
    clock: Rc<ManualClock>,
    counts: Counts,
    fixture: Fixture,
}

fn open(cx: &mut TestAppContext) -> Opened {
    let fixture = fixture();
    let clock = Rc::new(ManualClock::default());
    let counts = Rc::new(Cell::new((0, 0)));
    let editor = EditorInstance::new_with_policy_and_history_clock(
        fixture.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(fixture.before)),
        EditorHooks {
            listener: Some(Box::new(Listener(counts.clone()))),
            ..Default::default()
        },
        Box::new(TimedPolicy),
        clock.clone(),
    )
    .unwrap();
    let session = editor.session().clone();
    clock.session.replace(Some(Rc::downgrade(&session)));
    cx.update(bind_default_editor_keys);
    let handle = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|_| {
                let mut view = editor.build_view();
                view.set_measured_table_layout(true);
                view
            })
        })
        .unwrap()
    });
    handle
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    Opened {
        handle,
        session,
        clock,
        counts,
        fixture,
    }
}

fn child(view: &DocumentView, node: NodeId) -> Entity<ParagraphView> {
    view.children
        .iter()
        .find(|(id, _)| *id == node)
        .unwrap()
        .1
        .clone()
}

fn point(session: &SharedSession, node: NodeId, byte: usize) -> InlinePoint {
    let session = session.borrow();
    let inline = session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    InlinePoint::new(
        node,
        inline.offset_at(byte).unwrap(),
        0,
        CursorAffinity::Before,
    )
}

fn select(opened: &Opened, node: NodeId, byte: usize, cx: &mut TestAppContext) {
    let caret = point(&opened.session, node, byte);
    opened
        .session
        .borrow_mut()
        .set_inline_selection(caret, caret)
        .unwrap();
    opened
        .handle
        .update(cx, |view, window, cx| view.focus_selection(window, cx))
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

struct Snapshot {
    document: XiaomuDocument,
    selection: DocumentSelection,
    marks: Option<MarkSet>,
    depths: (usize, usize),
    rule_available: bool,
    counts: (usize, usize),
}
impl Snapshot {
    fn capture(opened: &Opened) -> Self {
        let session = opened.session.borrow();
        Self {
            document: session.document().clone(),
            selection: session.selection(),
            marks: session.stored_marks().cloned(),
            depths: session.history_depths(),
            rule_available: session.input_rule_undo_available(),
            counts: opened.counts.get(),
        }
    }
    fn assert_unchanged(&self, opened: &Opened) {
        let session = opened.session.borrow();
        assert_eq!(session.document().store(), self.document.store());
        assert_eq!(session.document().revision(), self.document.revision());
        assert_eq!(session.selection(), self.selection);
        assert_eq!(session.stored_marks(), self.marks.as_ref());
        assert_eq!(session.history_depths(), self.depths);
        assert_eq!(session.input_rule_undo_available(), self.rule_available);
        assert_eq!(opened.counts.get(), self.counts);
    }
}
