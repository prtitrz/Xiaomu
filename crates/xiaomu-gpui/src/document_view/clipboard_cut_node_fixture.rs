// Generic selected-block fixture; observes the stock writer without replacing it.

use super::*;
use xiaomu_core::selection::{CursorAffinity, NodeGap};
use xiaomu_runtime::session::DocumentPosition;

#[derive(Clone, Copy, Debug)]
enum NodeFailure {
    None,
    NoPlan,
    Prepare,
    Export,
    MissingExport,
    Empty,
    Core,
    StaleSelection,
    GapAfter,
    NodeAfter,
    AtomicAfter,
    RangeAfter,
    Admission,
}

struct NodeFixture {
    document: XiaomuDocument,
    selected: NodeId,
    caret: NodeId,
    survivor: NodeId,
}

fn node_fixture(table_after: bool, deep: bool) -> NodeFixture {
    let f = fixture(false, if deep { 160 } else { 1 });
    let root = f.document.root();
    let document = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeKind {
            node: f.cells[0],
            kind: NodeKind::TableHeader,
        })
        .with_step(TransactionStep::InsertNode {
            parent: root,
            index: 1,
            kind: NodeKind::Image,
            attrs: attrs([
                ("src", AttrValue::Null),
                (
                    "exact",
                    AttrValue::List(vec![AttrValue::String("image".into()), AttrValue::Null]),
                ),
            ]),
            content: NodeContent::Atomic,
        })
        .with_step(TransactionStep::InsertNode {
            parent: root,
            index: 3,
            kind: NodeKind::HorizontalRule,
            attrs: NodeAttrs::empty(),
            content: NodeContent::Atomic,
        })
        .apply(&f.document)
        .unwrap();
    let children = document
        .node(root)
        .unwrap()
        .content()
        .as_children()
        .unwrap();
    let caret = if table_after {
        document
            .node(f.cells[0])
            .unwrap()
            .content()
            .as_children()
            .unwrap()[0]
    } else {
        f.intro
    };
    NodeFixture {
        selected: children[1],
        survivor: children[3],
        document,
        caret,
    }
}

struct NodePolicy {
    seen: Rc<RefCell<Seen>>,
    selected: NodeId,
    caret: NodeId,
    survivor: NodeId,
    failure: NodeFailure,
}

impl SessionPolicy for NodePolicy {
    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        self.seen.borrow_mut().cut_preparations += 1;
        if matches!(self.failure, NodeFailure::NoPlan) {
            return Ok(None);
        }
        if matches!(self.failure, NodeFailure::Prepare) {
            return Err(PolicyError::new("node prepare refusal"));
        }
        if context
            .selection()
            .as_node_selection()
            .or(context.selection().as_atomic_node())
            != Some(self.selected)
        {
            return Ok(None);
        }
        let mut tx = Transaction::new(TransactionOrigin::UserInput);
        if !matches!(self.failure, NodeFailure::Empty) {
            tx.push_step(TransactionStep::RemoveNode {
                node: self.selected,
            });
        }
        if matches!(self.failure, NodeFailure::Core) {
            tx.push_step(TransactionStep::RemoveNode {
                node: context.document().root(),
            });
        }
        let selection = match self.failure {
            NodeFailure::StaleSelection => context.selection(),
            NodeFailure::GapAfter => {
                DocumentSelection::collapsed(NodeGap::new(context.document().root(), 1))
            }
            NodeFailure::NodeAfter => {
                DocumentSelection::node(&tx.apply(context.document()).unwrap(), self.caret).unwrap()
            }
            NodeFailure::AtomicAfter => {
                DocumentSelection::collapsed(DocumentPosition::Atomic(self.survivor))
            }
            NodeFailure::RangeAfter => {
                let offset = context
                    .document()
                    .node(self.caret)
                    .unwrap()
                    .content()
                    .as_inline()
                    .unwrap()
                    .offset_at(1)
                    .unwrap();
                DocumentSelection::new(
                    InlinePoint::at_start_of(self.caret),
                    InlinePoint::new(self.caret, offset, 0, CursorAffinity::After),
                )
            }
            _ => DocumentSelection::collapsed(InlinePoint::at_start_of(self.caret)),
        };
        Ok(Some(EditPlan::new(
            tx,
            SelectionUpdate::Exact { selection },
            None,
        )))
    }

    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.seen.borrow_mut().exports.push(purpose);
        match self.failure {
            NodeFailure::Export => Err(PolicyError::new("node export refusal")),
            NodeFailure::MissingExport => Ok(None),
            _ => Ok(Some(
                ClipboardExportSpec::new()
                    .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
            )),
        }
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Delete) {
            self.seen.borrow_mut().deletes += 1;
            return Err(PolicyError::new("generic Delete rejected by fixture"));
        }
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        if document.node(self.selected).is_none() {
            self.seen.borrow_mut().cut_candidates += 1;
            self.seen
                .borrow_mut()
                .cut_candidate_writes
                .push(write_observation::calls());
            if matches!(self.failure, NodeFailure::Admission) {
                return Err(PolicyError::new("node candidate refusal"));
            }
        }
        Ok(())
    }
}

fn mount_node_cut(
    cx: &mut TestAppContext,
    f: &NodeFixture,
    atomic: bool,
    failure: NodeFailure,
) -> (Mounted, Rc<RefCell<Notifications>>) {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let editor = EditorInstance::new_with_policy(
        f.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(f.caret)),
        EditorHooks::default(),
        Box::new(NodePolicy {
            seen: seen.clone(),
            selected: f.selected,
            caret: f.caret,
            survivor: f.survivor,
            failure,
        }),
    )
    .unwrap();
    let session = editor.session().clone();
    {
        let mut s = session.borrow_mut();
        s.apply_intent(&EditIntent::InsertText {
            text: "history".into(),
        })
        .unwrap();
        s.undo().unwrap();
        let selection = if atomic {
            DocumentSelection::collapsed(DocumentPosition::Atomic(f.selected))
        } else {
            DocumentSelection::node(s.document(), f.selected).unwrap()
        };
        s.set_document_selection(selection).unwrap();
    }
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| cx.new(|_| editor.build_view()))
            .unwrap()
    });
    window
        .update(cx, |view, window, cx| {
            window.activate_window();
            view.focus_selection(window, cx);
        })
        .unwrap();
    cx.background_executor.run_until_parked();
    let notifications = Rc::new(RefCell::new(Notifications::default()));
    session
        .borrow_mut()
        .add_listener(Box::new(Listener(notifications.clone())));
    (
        Mounted {
            window,
            session,
            seen,
        },
        notifications,
    )
}
