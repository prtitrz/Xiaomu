//! Only explicit node identity opts into host-defined node navigation.

use super::*;
use crate::editor_commands::{CommandRoute, EditorCommand, EditorCommandRouter};

type Navigations = Rc<RefCell<Vec<(DocumentSelection, NodeNavigation)>>>;
struct Router {
    calls: Navigations,
    legacy_down: Rc<Cell<usize>>,
    target: Rc<RefCell<Option<DocumentSelection>>>,
    reject: Rc<Cell<bool>>,
}
impl EditorCommandRouter for Router {
    fn route(
        &self,
        _: EditorCommandContext<'_>,
        _: EditorCommand<'_>,
    ) -> Result<CommandRoute, PolicyError> {
        Ok(CommandRoute::Default)
    }
    fn route_arrow_down(
        &self,
        _: EditorCommandContext<'_>,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.legacy_down.set(self.legacy_down.get() + 1);
        Ok(None)
    }
    fn route_node_navigation(
        &self,
        context: EditorCommandContext<'_>,
        gesture: NodeNavigation,
    ) -> Result<Option<DocumentSelection>, PolicyError> {
        self.calls.borrow_mut().push((context.selection(), gesture));
        if self.reject.get() {
            return Err(PolicyError::new("navigation fixture rejection"));
        }
        Ok(*self.target.borrow())
    }
}

#[gpui::test]
fn node_navigation_defaults_consume_without_synthesizing_text_or_atomic_targets(
    cx: &mut TestAppContext,
) {
    let f = fixture();
    let (handle, session, counts) = open(cx, &f, None);
    for selected in [f.quote, f.intro, f.rule] {
        select(cx, handle, selected);
        let selection = session.borrow().selection();
        counts.set((0, 0));
        for gesture in [
            "left",
            "right",
            "up",
            "down",
            "home",
            "end",
            "shift-left",
            "shift-right",
            "shift-up",
            "shift-down",
            "shift-home",
            "shift-end",
        ] {
            key(cx, handle, gesture);
            assert_unchanged(&session, &f, selection, &counts);
        }
    }
}

#[gpui::test]
fn node_router_receives_exact_gestures_and_installs_only_valid_targets(cx: &mut TestAppContext) {
    let f = fixture();
    let (handle, session, counts) = open(cx, &f, None);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let legacy_down = Rc::new(Cell::new(0));
    let target = Rc::new(RefCell::new(None));
    let reject = Rc::new(Cell::new(false));
    handle
        .update(cx, |view, _, _| {
            view.set_command_router(Some(Rc::new(Router {
                calls: calls.clone(),
                legacy_down: legacy_down.clone(),
                target: target.clone(),
                reject: reject.clone(),
            })))
        })
        .unwrap();
    select(cx, handle, f.quote);
    let selection = session.borrow().selection();
    counts.set((0, 0));
    for (gesture, direction, extend) in [
        ("left", NodeNavigationDirection::Left, false),
        ("shift-right", NodeNavigationDirection::Right, true),
        ("up", NodeNavigationDirection::Up, false),
        ("down", NodeNavigationDirection::Down, false),
        ("shift-home", NodeNavigationDirection::Home, true),
        ("end", NodeNavigationDirection::End, false),
    ] {
        key(cx, handle, gesture);
        assert_eq!(
            calls.borrow().last(),
            Some(&(selection, NodeNavigation { direction, extend }))
        );
        assert_unchanged(&session, &f, selection, &counts);
    }
    assert_eq!(legacy_down.get(), 0);
    reject.set(true);
    *target.borrow_mut() = Some(DocumentSelection::collapsed(InlinePoint::at_start_of(
        f.intro,
    )));
    key(cx, handle, "down");
    assert_unchanged(&session, &f, selection, &counts);
    reject.set(false);
    // An inline position naming a container is rejected at installation.
    *target.borrow_mut() = Some(DocumentSelection::collapsed(InlinePoint::at_start_of(
        f.document.root(),
    )));
    key(cx, handle, "down");
    assert_unchanged(&session, &f, selection, &counts);
    let next = DocumentSelection::node(&f.document, f.rule).unwrap();
    *target.borrow_mut() = Some(next);
    key(cx, handle, "right");
    assert_eq!(session.borrow().selection(), next);
    assert_eq!(session.borrow().history_depths(), (0, 0));
    assert_eq!(counts.get(), (0, 1));
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(view.range_input.as_ref().unwrap().0, f.rule);
            assert!(view.range_input_is_focused(window, cx));
        })
        .unwrap();
    let caret = DocumentSelection::collapsed(InlinePoint::at_start_of(f.intro));
    *target.borrow_mut() = Some(caret);
    key(cx, handle, "left");
    assert_eq!(session.borrow().selection(), caret);
    assert_eq!(session.borrow().history_depths(), (0, 0));
    let node_calls = calls.borrow().len();
    key(cx, handle, "down");
    assert_eq!(calls.borrow().len(), node_calls);
    assert_eq!(legacy_down.get(), 1);
}

#[gpui::test]
fn all_plain_gaps_and_legacy_atomic_do_not_enter_node_navigation_hook(cx: &mut TestAppContext) {
    let f = fixture();
    let (handle, session, _) = open(cx, &f, None);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let legacy_down = Rc::new(Cell::new(0));
    handle
        .update(cx, |view, _, _| {
            view.set_command_router(Some(Rc::new(Router {
                calls: calls.clone(),
                legacy_down: legacy_down.clone(),
                target: Rc::new(RefCell::new(None)),
                reject: Rc::new(Cell::new(false)),
            })))
        })
        .unwrap();
    let node = DocumentSelection::node(&f.document, f.quote).unwrap();
    for selection in [
        DocumentSelection::all(&f.document),
        DocumentSelection::new(node.anchor(), node.focus()),
        DocumentSelection::collapsed(DocumentPosition::Atomic(f.rule)),
    ] {
        session
            .borrow_mut()
            .set_document_selection(selection)
            .unwrap();
        handle
            .update(cx, |view, window, cx| {
                view.focus_selection(window, cx);
                cx.notify();
            })
            .unwrap();
        key(cx, handle, "down");
        assert_eq!(session.borrow().selection(), selection);
        assert!(calls.borrow().is_empty());
    }
    assert_eq!(legacy_down.get(), 3);
    // Legacy Atomic still traverses to the next ordinary navigation unit.
    key(cx, handle, "right");
    assert!(session.borrow().selection().as_same_node_inline().is_some());
    assert!(calls.borrow().is_empty());
    assert_eq!(session.borrow().history_depths(), (0, 0));
}

#[gpui::test]
fn node_navigation_never_reaches_router_during_native_composition(cx: &mut TestAppContext) {
    let f = fixture();
    let (handle, session, _) = open(cx, &f, None);
    let calls = Rc::new(RefCell::new(Vec::new()));
    handle
        .update(cx, |view, _, _| {
            view.set_command_router(Some(Rc::new(Router {
                calls: calls.clone(),
                legacy_down: Rc::new(Cell::new(0)),
                target: Rc::new(RefCell::new(Some(DocumentSelection::collapsed(
                    InlinePoint::at_start_of(f.intro),
                )))),
                reject: Rc::new(Cell::new(false)),
            })))
        })
        .unwrap();
    select(cx, handle, f.quote);
    let selection = session.borrow().selection();
    let input = proxy(cx, handle);
    handle
        .update(cx, |_, window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "ni", None, window, cx);
            })
        })
        .unwrap();
    key(cx, handle, "down shift-right home");
    assert!(calls.borrow().is_empty());
    assert_eq!(session.borrow().selection(), selection);
}
