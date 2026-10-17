//! Independent, default-off inline-chip focus. Never a document selection.
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use gpui::{App, Bounds, Context, FocusHandle, KeystrokeEvent, Pixels, Subscription, Window};
use xiaomu_core::document::{DocumentRevision, NodeId};
use xiaomu_runtime::session::DocumentSessionIdentity;

use super::DocumentView;
use crate::atom_capability::{AtomAction, SharedAtomCapability};

pub(crate) type SharedAtomKeyboard = Rc<RefCell<AtomKeyboard>>;

pub(crate) struct ChipFocus {
    pub handle: FocusHandle,
    pub action: AtomAction,
    pub placement: (NodeId, usize, usize),
    pub frame: u64,
}

#[derive(Default)]
pub(crate) struct AtomKeyboard {
    pub capability: Option<SharedAtomCapability>,
    pub entries: BTreeMap<NodeId, ChipFocus>,
    pub frame: u64,
    snapshot: Option<(DocumentSessionIdentity, DocumentRevision, u64)>,
    retired: Vec<FocusHandle>,
    pub targets: Vec<(FocusHandle, Vec<Bounds<Pixels>>)>,
}

impl AtomKeyboard {
    pub fn invalidate(&mut self) {
        self.retired.extend(
            std::mem::take(&mut self.entries)
                .into_values()
                .map(|entry| entry.handle),
        );
        self.snapshot = None;
        self.targets.clear();
    }

    pub fn replace(&mut self, capability: SharedAtomCapability) {
        self.invalidate();
        self.capability = Some(capability);
    }

    pub fn finish_frame(&mut self, window: &Window) {
        self.entries.retain(|_, entry| {
            if entry.frame == self.frame {
                return true;
            }
            self.retired.push(entry.handle.clone());
            false
        });
        self.retired.retain(|handle| handle.is_focused(window));
    }

    pub fn owns_focus(&self, window: &Window) -> bool {
        self.entries
            .values()
            .any(|entry| entry.handle.is_focused(window))
            || self.retired.iter().any(|handle| handle.is_focused(window))
    }
}

impl DocumentView {
    /// Installs the host adapter that receives atom activations.
    /// Replacing it invalidates existing chip focus and pointer gestures.
    pub fn set_atom_capability(&mut self, capability: SharedAtomCapability) {
        self.atom_keyboard_subscription = None;
        self.atom_keyboard.borrow_mut().replace(capability.clone());
        self.atom_pointer = None;
        self.atom_capability = Some(capability);
    }

    pub(super) fn keyboard_focus_owner(&self, window: &Window) -> Option<NodeId> {
        self.atom_keyboard
            .borrow()
            .entries
            .iter()
            .find(|(_, entry)| entry.handle.is_focused(window))
            .map(|(node, _)| *node)
    }

    pub(super) fn begin_atom_keyboard_frame(&mut self, cx: &mut Context<Self>) {
        let mut state = self.atom_keyboard.borrow_mut();
        let session = self.session.borrow();
        let snapshot = (
            session.identity(),
            session.document().revision(),
            self.epoch.get(),
        );
        if state.snapshot.as_ref() != Some(&snapshot) {
            state.invalidate();
            state.snapshot = Some(snapshot);
        }
        state.frame = state.frame.wrapping_add(1);
        state.targets.clear();
        drop(session);
        drop(state);
        if self.atom_capability.is_some() && self.atom_keyboard_subscription.is_none() {
            let weak = cx.entity().downgrade();
            let subscription: Subscription = cx.intercept_keystrokes(move |event, window, cx| {
                let _ = weak.update(cx, |view, cx| view.atom_key(event, window, cx));
            });
            self.atom_keyboard_subscription = Some(subscription);
        }
    }

    fn atom_key(&self, event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
        // GPUI visits every interceptor even after propagation stops. A prior
        // editor may already have moved focus here during this same key event.
        if window.default_prevented() {
            return;
        }
        if !event
            .context_stack
            .iter()
            .any(|context| context.contains("XiaomuAtom"))
        {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let traverse = event.keystroke.key == "tab"
            && !modifiers.control
            && !modifiers.platform
            && !modifiers.alt;
        let activate = matches!(event.keystroke.key.as_str(), "enter" | "space" | " ");
        if !traverse && !activate {
            return;
        }
        let state = self.atom_keyboard.borrow();
        let Some(entry) = state
            .entries
            .values()
            .find(|entry| entry.handle.is_focused(window))
        else {
            if state.owns_focus(window) {
                cx.stop_propagation();
                window.prevent_default();
                if traverse {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                }
            }
            return;
        };
        // Targets are siblings of the editing subtree: unrelated host actions
        // remain available and cannot bubble through the retained paragraph.
        cx.stop_propagation();
        window.prevent_default();
        if traverse {
            drop(state);
            if modifiers.shift {
                window.focus_prev();
            } else {
                window.focus_next();
            }
            return;
        }
        let session = self.session.borrow();
        if entry.frame != state.frame
            || state.snapshot.as_ref()
                != Some(&(
                    session.identity(),
                    session.document().revision(),
                    self.epoch.get(),
                ))
            || self
                .hidden_table_ancestor(session.document(), entry.placement.0)
                .is_some()
        {
            return;
        }
        let Some(node) = session.document().node(entry.action.node) else {
            return;
        };
        if node.attrs() != &entry.action.attrs {
            return;
        }
        let Some(capability) = state.capability.clone() else {
            return;
        };
        let action = entry.action.clone();
        let handle = entry.handle.clone();
        let frame = entry.frame;
        let placement = entry.placement;
        let snapshot = (
            session.identity(),
            session.document().revision(),
            self.epoch.get(),
        );
        let selection = session.selection();
        drop(session);
        drop(state);
        if !capability.keyboard_focusable(&action) {
            return;
        }
        let current = {
            let session = self.session.borrow();
            let state = self.atom_keyboard.borrow();
            handle.is_focused(window)
                && snapshot
                    == (
                        session.identity(),
                        session.document().revision(),
                        self.epoch.get(),
                    )
                && self
                    .hidden_table_ancestor(session.document(), placement.0)
                    .is_none()
                && selection == session.selection()
                && state.frame == frame
                && state
                    .capability
                    .as_ref()
                    .is_some_and(|now| Rc::ptr_eq(now, &capability))
                && state.entries.get(&action.node).is_some_and(|entry| {
                    entry.handle == handle
                        && entry.action == action
                        && entry.placement == placement
                        && entry.frame == frame
                })
        };
        if current {
            capability.atom_keyboard_activate(action, modifiers);
        }
    }
}

/// Mount focus proxies outside the paragraph/document input dispatch ancestry.
/// Zero-size proxies have no mouse hit area; their separate measured outlines
/// never intercept the existing pointer path. Order is child prepaint order,
/// deliberately independent of NodeId/map order.
pub(super) fn prepaint_targets(
    view: &gpui::Entity<DocumentView>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<gpui::AnyElement> {
    use gpui::{AvailableSpace, div, prelude::*, px, rgba, size};
    let targets = std::mem::take(&mut view.read(cx).atom_keyboard.borrow_mut().targets);
    let mut elements = Vec::new();
    for (handle, rectangles) in targets {
        let Some(first) = rectangles.first() else {
            continue;
        };
        let mut proxy = div()
            .w(px(0.))
            .h(px(0.))
            .track_focus(&handle)
            .key_context("XiaomuAtom")
            .into_any_element();
        proxy.prepaint_as_root(
            first.origin,
            size(
                AvailableSpace::Definite(px(0.)),
                AvailableSpace::Definite(px(0.)),
            ),
            window,
            cx,
        );
        elements.push(proxy);
        if handle.is_focused(window) {
            for rect in rectangles {
                let mut outline = div()
                    .w(rect.size.width)
                    .h(rect.size.height)
                    .border_1()
                    .border_color(rgba(0x3377ccff))
                    .into_any_element();
                outline.prepaint_as_root(
                    rect.origin,
                    size(
                        AvailableSpace::Definite(rect.size.width),
                        AvailableSpace::Definite(rect.size.height),
                    ),
                    window,
                    cx,
                );
                elements.push(outline);
            }
        }
    }
    elements
}
