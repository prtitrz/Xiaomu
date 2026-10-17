//! Captured atom clicks use real down/move/up dispatch and painted geometry.

#[path = "atom_pointer_geometry_tests.rs"]
mod geometry_tests;
#[path = "atom_pointer_lifecycle_tests.rs"]
mod lifecycle_tests;
#[path = "atom_pointer_sibling_tests.rs"]
mod sibling_tests;

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext,
    WindowHandle, point, px,
};
use xiaomu_core::{
    document::{
        AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet,
        NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
    text::TextOffset,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession, EditIntent};

use super::DocumentView;
use crate::{
    atom_capability::{
        ATOM_ACTION_CLICK, AtomAction, InlineAtomHostCapability, SharedAtomCapability,
    },
    block_view::SharedSession,
    editor::{EditorHooks, EditorInstance, bind_default_editor_keys},
};

#[derive(Default)]
struct Host {
    capture: bool,
    probes: RefCell<Vec<(AtomAction, Modifiers)>>,
    clicks: RefCell<Vec<(AtomAction, Modifiers)>>,
    legacy: RefCell<Vec<AtomAction>>,
}

impl InlineAtomHostCapability for Host {
    fn atom_action(&self, action: AtomAction) {
        self.legacy.borrow_mut().push(action);
    }

    fn capture_pointer_click(&self, action: &AtomAction, modifiers: Modifiers) -> bool {
        self.probes.borrow_mut().push((action.clone(), modifiers));
        self.capture
    }

    fn atom_pointer_click(&self, action: AtomAction, modifiers: Modifiers) {
        self.clicks.borrow_mut().push((action, modifiers));
    }
}

fn capturing_host() -> Rc<Host> {
    Rc::new(Host {
        capture: true,
        ..Default::default()
    })
}

struct Fixture {
    document: XiaomuDocument,
    paragraph: NodeId,
    selected: NodeId,
    atoms: [NodeId; 2],
    selection: DocumentSelection,
}

fn fixture() -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let atoms = std::array::from_fn(|_| {
        builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("reference").unwrap()),
                NodeAttrs::new([("target".into(), AttrValue::String("same target".into()))].into())
                    .unwrap(),
                NodeContent::InlineAtom(InlineAtomContent::new("[same label]").unwrap()),
            )
            .unwrap()
    });
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(" trailing text", MarkSet::empty()).unwrap()],
                    atoms.map(|atom| InlineAtomPlacement::new(atom, TextOffset::ZERO)),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let inline =
        InlineContent::new([TextRun::new("keep selection", MarkSet::empty()).unwrap()]).unwrap();
    let start = inline.offset_at(2).unwrap();
    let end = inline.offset_at(8).unwrap();
    let selected = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, selected]),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
        selected,
        atoms,
        selection: DocumentSelection::new(
            InlinePoint::new(selected, end, 0, CursorAffinity::Before),
            InlinePoint::new(selected, start, 0, CursorAffinity::After),
        ),
    }
}

fn open(
    f: &Fixture,
    capability: Option<SharedAtomCapability>,
    cx: &mut TestAppContext,
) -> (WindowHandle<DocumentView>, SharedSession) {
    let editor = EditorInstance::new(
        f.document.clone(),
        f.selection,
        EditorHooks {
            atom_capability: capability,
            ..Default::default()
        },
    )
    .unwrap();
    let session = editor.session().clone();
    let handle = cx.update(|cx| {
        bind_default_editor_keys(cx);
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
    (handle, session)
}

fn chip_point(
    handle: WindowHandle<DocumentView>,
    paragraph: NodeId,
    atom: NodeId,
    cx: &mut TestAppContext,
) -> Point<Pixels> {
    handle
        .update(cx, |view, _, cx| {
            let child = view
                .children
                .iter()
                .find(|(node, _)| *node == paragraph)
                .unwrap()
                .1
                .read(cx);
            let projection = child.atom_display_projection().unwrap();
            let span = projection
                .atoms()
                .iter()
                .find(|span| span.atom() == atom)
                .unwrap();
            let layout = child.last_layout.as_ref().expect("mounted atom layout");
            let start = layout
                .position_for_caret(span.display_range().start, CursorAffinity::After)
                .unwrap();
            let end = layout
                .position_for_caret(span.display_range().end, CursorAffinity::Before)
                .unwrap();
            assert_eq!(start.y, end.y, "fixture chip stays on one painted row");
            child.last_bounds.unwrap().origin + point((start.x + end.x) / 2.0, start.y + px(8.0))
        })
        .unwrap()
}

fn down(
    handle: WindowHandle<DocumentView>,
    point: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut TestAppContext,
) {
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_down(
        point,
        MouseButton::Left,
        modifiers,
    );
}

fn up(
    handle: WindowHandle<DocumentView>,
    point: Point<Pixels>,
    modifiers: Modifiers,
    cx: &mut TestAppContext,
) {
    VisualTestContext::from_window(handle.into(), cx).simulate_mouse_up(
        point,
        MouseButton::Left,
        modifiers,
    );
}

fn unchanged(f: &Fixture, session: &SharedSession) {
    let session = session.borrow();
    assert_eq!(session.selection(), f.selection);
    assert_eq!(session.document().store(), f.document.store());
    assert_eq!(session.document().revision(), f.document.revision());
    assert_eq!(session.history_depths(), (0, 0));
}

fn assert_focus(handle: WindowHandle<DocumentView>, node: NodeId, cx: &mut TestAppContext) {
    handle
        .update(cx, |view, window, cx| {
            assert_eq!(
                view.accessibility_projection(window, cx)
                    .unwrap()
                    .focus_owner(),
                Some(node)
            );
        })
        .unwrap();
}

fn modifiers() -> [Modifiers; 4] {
    [
        Modifiers::default(),
        Modifiers {
            control: true,
            ..Default::default()
        },
        Modifiers {
            platform: true,
            ..Default::default()
        },
        Modifiers {
            shift: true,
            ..Default::default()
        },
    ]
}

#[gpui::test]
fn atom_pointer_capture_preserves_selection_and_focus_until_matching_release(
    cx: &mut TestAppContext,
) {
    for modifiers in modifiers() {
        let f = fixture();
        let host = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
        down(handle, point, modifiers, cx);
        unchanged(&f, &session);
        assert_focus(handle, f.selected, cx);
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        let expected = AtomAction {
            node: f.atoms[0],
            kind: AtomKind::new("reference").unwrap(),
            action: ATOM_ACTION_CLICK.into(),
            attrs: f.document.node(f.atoms[0]).unwrap().attrs().clone(),
        };
        assert_eq!(
            host.probes.borrow().as_slice(),
            &[(expected.clone(), modifiers)]
        );
        up(handle, point, modifiers, cx);
        assert_eq!(host.clicks.borrow().as_slice(), &[(expected, modifiers)]);
        unchanged(&f, &session);
        assert_focus(handle, f.selected, cx);
        up(handle, point, modifiers, cx);
        assert_eq!(
            host.clicks.borrow().len(),
            1,
            "release cannot consume a capture twice"
        );
    }
}

#[gpui::test]
fn atom_pointer_distinct_same_label_occurrences_activate_once_per_gesture(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    for (index, atom) in [f.atoms[0], f.atoms[1], f.atoms[0], f.atoms[1]]
        .into_iter()
        .enumerate()
    {
        let point = chip_point(handle, f.paragraph, atom, cx);
        down(handle, point, Modifiers::default(), cx);
        assert_eq!(host.clicks.borrow().len(), index);
        up(handle, point, Modifiers::default(), cx);
        assert_eq!(host.clicks.borrow().len(), index + 1);
        assert_eq!(host.clicks.borrow()[index].0.node, atom);
        unchanged(&f, &session);
    }
}

#[gpui::test]
fn atom_pointer_release_elsewhere_cancels(cx: &mut TestAppContext) {
    for cancellation in 0..3 {
        let f = fixture();
        let host = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
        down(handle, point, Modifiers::default(), cx);
        let release = match cancellation {
            0 => chip_point(handle, f.paragraph, f.atoms[1], cx),
            1 => point + gpui::point(px(0.0), px(40.0)),
            _ => gpui::point(px(-20.0), px(-20.0)),
        };
        up(handle, release, Modifiers::default(), cx);
        up(handle, point, Modifiers::default(), cx);
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        unchanged(&f, &session);
    }
}

#[gpui::test]
fn atom_pointer_callback_receives_release_modifiers_when_they_change(cx: &mut TestAppContext) {
    for pressed in modifiers() {
        for released in modifiers() {
            let f = fixture();
            let host = capturing_host();
            let (handle, session) = open(&f, Some(host.clone()), cx);
            let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
            down(handle, point, pressed, cx);
            assert_eq!(host.probes.borrow()[0].1, pressed);
            up(handle, point, released, cx);
            assert_eq!(host.clicks.borrow().len(), 1);
            assert_eq!(host.clicks.borrow()[0].1, released);
            unchanged(&f, &session);
        }
    }
}

#[gpui::test]
fn atom_pointer_drag_away_and_back_stays_cancelled_then_next_click_works(cx: &mut TestAppContext) {
    let f = fixture();
    let host = capturing_host();
    let (handle, session) = open(&f, Some(host.clone()), cx);
    let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
    down(handle, point, Modifiers::default(), cx);
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.simulate_mouse_move(
        point + gpui::point(px(0.0), px(40.0)),
        Some(MouseButton::Left),
        Modifiers::default(),
    );
    visual.simulate_mouse_move(point, Some(MouseButton::Left), Modifiers::default());
    up(handle, point, Modifiers::default(), cx);
    assert!(host.clicks.borrow().is_empty());
    assert!(host.legacy.borrow().is_empty());
    unchanged(&f, &session);
    down(handle, point, Modifiers::default(), cx);
    up(handle, point, Modifiers::default(), cx);
    assert_eq!(host.clicks.borrow().len(), 1);
}

#[gpui::test]
fn atom_pointer_stale_revision_selection_session_or_capability_cancels(cx: &mut TestAppContext) {
    for stale in 0..5 {
        let f = fixture();
        let host = capturing_host();
        let replacement = capturing_host();
        let (handle, session) = open(&f, Some(host.clone()), cx);
        let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
        down(handle, point, Modifiers::default(), cx);
        match stale {
            0 => {
                session
                    .borrow_mut()
                    .apply_intent(&EditIntent::InsertText {
                        text: "changed".into(),
                    })
                    .unwrap();
            }
            1 => {
                let start = InlinePoint::at_start_of(f.selected);
                session
                    .borrow_mut()
                    .set_inline_selection(start, start)
                    .unwrap();
            }
            2 => {
                *session.borrow_mut() =
                    DocumentSession::new(f.document.clone(), f.selection).unwrap();
            }
            3 => {
                handle
                    .update(cx, |view, _, _| {
                        view.set_atom_capability(replacement.clone())
                    })
                    .unwrap();
            }
            _ => {
                session
                    .borrow_mut()
                    .apply(&Transaction::new(TransactionOrigin::System).with_step(
                        TransactionStep::SetNodeAttrs {
                            node: f.atoms[0],
                            attrs: NodeAttrs::empty(),
                        },
                    ))
                    .unwrap();
            }
        }
        let before_up = {
            let live = session.borrow();
            (
                live.document().clone(),
                live.selection(),
                live.history_depths(),
            )
        };
        up(handle, point, Modifiers::default(), cx);
        assert!(host.clicks.borrow().is_empty());
        assert!(host.legacy.borrow().is_empty());
        assert!(replacement.clicks.borrow().is_empty());
        let live = session.borrow();
        assert_eq!(live.document().store(), before_up.0.store());
        assert_eq!(live.document().revision(), before_up.0.revision());
        assert_eq!(live.selection(), before_up.1);
        assert_eq!(live.history_depths(), before_up.2);
    }
}

#[derive(Default)]
struct LegacyHost(RefCell<Vec<AtomAction>>);

// Existing hosts implement only atom_action and retain their down-time callback.
impl InlineAtomHostCapability for LegacyHost {
    fn atom_action(&self, action: AtomAction) {
        self.0.borrow_mut().push(action);
    }
}

#[gpui::test]
fn atom_pointer_absent_default_and_declined_capture_keep_caret_and_legacy_down(
    cx: &mut TestAppContext,
) {
    for mode in 0..3 {
        for modifiers in modifiers() {
            let f = fixture();
            let legacy = Rc::new(LegacyHost::default());
            let declined = Rc::new(Host::default());
            let capability: Option<SharedAtomCapability> = match mode {
                0 => None,
                1 => Some(legacy.clone()),
                _ => Some(declined.clone()),
            };
            let (handle, session) = open(&f, capability, cx);
            let point = chip_point(handle, f.paragraph, f.atoms[0], cx);
            down(handle, point, modifiers, cx);
            let selection = session.borrow().selection();
            assert_ne!(selection, f.selection);
            assert_eq!(selection.is_collapsed(), !modifiers.shift);
            if modifiers.shift {
                assert_eq!(selection.anchor(), f.selection.anchor());
            }
            let callbacks = usize::from(!modifiers.shift);
            assert_eq!(
                legacy.0.borrow().len(),
                if mode == 1 { callbacks } else { 0 }
            );
            assert_eq!(
                declined.legacy.borrow().len(),
                if mode == 2 { callbacks } else { 0 }
            );
            assert!(declined.clicks.borrow().is_empty());
            up(handle, point, modifiers, cx);
            assert_eq!(session.borrow().selection(), selection);
            assert_eq!(
                legacy.0.borrow().len(),
                if mode == 1 { callbacks } else { 0 }
            );
            assert_eq!(
                declined.legacy.borrow().len(),
                if mode == 2 { callbacks } else { 0 }
            );
            assert_eq!(session.borrow().document().store(), f.document.store());
            assert_eq!(session.borrow().history_depths(), (0, 0));
        }
    }
}
