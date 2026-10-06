// Real-session fixtures for the prepared Cut coordinator; no clipboard override.

use super::*;

#[derive(Clone, Copy)]
enum Failure {
    None,
    FinalPolicy,
    FinalSelection,
    Core,
}

struct CutPolicy {
    seen: Rc<RefCell<Seen>>,
    cells: Vec<NodeId>,
    failure: Failure,
}

impl SessionPolicy for CutPolicy {
    fn clipboard_export_spec(
        &self,
        _: SessionContext<'_>,
        purpose: ClipboardExportPurpose,
    ) -> Result<Option<ClipboardExportSpec>, PolicyError> {
        self.seen.borrow_mut().exports.push(purpose);
        Ok(Some(
            ClipboardExportSpec::new()
                .with_closed_cell_ranges()
                .with_text_projection(ClipboardTextProjection::TextBetweenLfV1),
        ))
    }

    fn prepare_cut(&self, context: SessionContext<'_>) -> Result<Option<EditPlan>, PolicyError> {
        self.seen.borrow_mut().cut_preparations += 1;
        let Some(range) = context.selection().active_cell_range() else {
            return Ok(None);
        };
        // This fixture deliberately supports only its complete captured range.
        // It is not a replacement for the product's bounded table Cut planner.
        if !((range.anchor() == self.cells[0] && range.focus() == *self.cells.last().unwrap())
            || (range.focus() == self.cells[0] && range.anchor() == *self.cells.last().unwrap()))
        {
            return Err(PolicyError::new("fixture requires its complete cell range"));
        }
        let mut cells = self.cells.clone();
        cells.sort_by_key(|cell| *cell == range.focus());
        let mut transaction = Transaction::new(TransactionOrigin::UserInput);
        for cell in cells {
            let old_children = context
                .document()
                .node(cell)
                .unwrap()
                .content()
                .as_children()
                .unwrap();
            transaction = transaction.with_step(TransactionStep::InsertNode {
                parent: cell,
                index: 0,
                kind: NodeKind::Paragraph,
                attrs: NodeAttrs::empty(),
                content: NodeContent::Inline(InlineContent::empty()),
            });
            for child in old_children {
                transaction = transaction.with_step(TransactionStep::RemoveNode { node: *child });
            }
        }
        if matches!(self.failure, Failure::Core) {
            transaction = transaction.with_step(TransactionStep::RemoveNode {
                node: context.document().root(),
            });
        }
        let selection = if matches!(self.failure, Failure::FinalSelection) {
            let removed = context
                .document()
                .node(self.cells[0])
                .unwrap()
                .content()
                .as_children()
                .unwrap()[0];
            SelectionUpdate::Exact {
                selection: DocumentSelection::collapsed(InlinePoint::at_start_of(removed)),
            }
        } else {
            SelectionUpdate::CaretAtLastInsertedOffset { offset: 0 }
        };
        Ok(Some(EditPlan::new(transaction, selection, None)))
    }

    fn prepare_intent(
        &self,
        _: SessionContext<'_>,
        intent: &EditIntent,
    ) -> Result<IntentDisposition, PolicyError> {
        if matches!(intent, EditIntent::Delete) {
            self.seen.borrow_mut().deletes += 1;
            return Err(PolicyError::new(
                "prepared Cut must not dispatch generic Delete",
            ));
        }
        Ok(IntentDisposition::Continue)
    }

    fn validate_document(&self, document: &XiaomuDocument) -> Result<(), PolicyError> {
        let cleared = self.cells.iter().all(|cell| {
            let children = document
                .node(*cell)
                .unwrap()
                .content()
                .as_children()
                .unwrap();
            children.len() == 1
                && document
                    .node(children[0])
                    .unwrap()
                    .content()
                    .as_inline()
                    .is_some_and(|inline| inline.runs().is_empty() && inline.atoms().is_empty())
        });
        if cleared {
            self.seen.borrow_mut().cut_candidates += 1;
            self.seen
                .borrow_mut()
                .cut_candidate_writes
                .push(write_observation::calls());
            if matches!(self.failure, Failure::FinalPolicy) {
                return Err(PolicyError::new(
                    "fixture rejects the fully cleared candidate",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Notifications {
    // The passive stock-writer count observed by each document listener call.
    document_writes: Vec<usize>,
    selections: usize,
}

struct Listener(Rc<RefCell<Notifications>>);

impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        selection.validate(document).unwrap();
        self.0
            .borrow_mut()
            .document_writes
            .push(write_observation::calls());
    }

    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.borrow_mut().selections += 1;
    }
}

fn mount_cut(
    cx: &mut TestAppContext,
    fixture: &Fixture,
    failure: Failure,
) -> (Mounted, Rc<RefCell<Notifications>>) {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let editor = EditorInstance::new_with_policy(
        fixture.document.clone(),
        DocumentSelection::collapsed(InlinePoint::at_start_of(fixture.intro)),
        EditorHooks::default(),
        Box::new(CutPolicy {
            seen: seen.clone(),
            cells: fixture.cells.clone(),
            failure,
        }),
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
            .set_cell_range_selection(fixture.cells[0], *fixture.cells.last().unwrap())
            .unwrap();
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

fn capture_rejections(
    m: &Mounted,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<Vec<EditorRejection>>>, gpui::Subscription) {
    let entity = m.window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    let subscription = cx.update(|cx| {
        cx.subscribe(&entity, move |emitter, event: &EditorRejection, cx| {
            // The event can safely inspect the final live session.
            assert!(emitter.read(cx).session().try_borrow_mut().is_ok());
            seen.borrow_mut().push(*event);
        })
    });
    (events, subscription)
}

fn seed_view_state(m: &Mounted, cx: &mut TestAppContext) -> u64 {
    m.window
        .update(cx, |view, _, _| {
            let point = InlinePoint::at_start_of(view.children.first().unwrap().0);
            view.desired_x = Some((point, gpui::px(37.0)));
            view.epoch.get()
        })
        .unwrap()
}

fn assert_view_unchanged(m: &Mounted, epoch: u64, cx: &mut TestAppContext) {
    m.window
        .update(cx, |view, _, _| {
            assert_eq!(view.epoch.get(), epoch);
            assert!(view.desired_x.is_some());
        })
        .unwrap();
}

fn assert_no_notifications(notifications: &Rc<RefCell<Notifications>>) {
    assert!(notifications.borrow().document_writes.is_empty());
    assert_eq!(notifications.borrow().selections, 0);
}
