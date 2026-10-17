//! Host capability seam for inline-atom activations.
//!
//! Hosts react to user activation on an atom chip without leaking business
//! types into Core or Runtime: the callback receives only stable keys and a
//! canonical snapshot (atom [`NodeId`], [`AtomKind`], action key, attrs).

use std::rc::Rc;

use gpui::{Modifiers, SharedString};
use xiaomu_core::document::{AtomKind, NodeAttrs, NodeId};

/// One host-visible atom activation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtomAction {
    /// Canonical atom identity within this editor's current snapshot.
    pub node: NodeId,
    /// Stable semantic kind key the host can dispatch on.
    pub kind: AtomKind,
    /// Stable action key, e.g. `click`.
    pub action: SharedString,
    /// Snapshot of the atom's canonical extension attributes.
    pub attrs: NodeAttrs,
}

/// Host adapter receiving atom activations from one editor instance.
///
/// Implementations live entirely on the host side; the editor only forwards
/// the canonical data above and never awaits or interprets the reaction.
pub trait InlineAtomHostCapability {
    /// Delivers one atom activation to the host.
    fn atom_action(&self, action: AtomAction);

    /// Opts this pointer press into selection-preserving click handling.
    ///
    /// Called before caret placement or default focus transfer. Returning
    /// `false` preserves legacy mouse-down activation and Shift selection.
    /// This eligibility query must not perform the activation itself.
    /// The resolved candidate may be clamped outside its visible bounds;
    /// returning `true` consumes that press without any activation or legacy
    /// callback. Only a visible chip can complete a captured click.
    fn capture_pointer_click(&self, _action: &AtomAction, _modifiers: Modifiers) -> bool {
        false
    }

    /// Delivers a captured click after release over the same unchanged chip.
    ///
    /// Modifiers are sampled at release, just as for a completed pointer click.
    /// Leaving the chip, replacing its capability or changing the document,
    /// session or selection cancels the gesture without moving the caret.
    fn atom_pointer_click(&self, action: AtomAction, _modifiers: Modifiers) {
        self.atom_action(action);
    }
}

/// Shared handle used by the embedding seams.
pub type SharedAtomCapability = Rc<dyn InlineAtomHostCapability>;

/// The stable action key emitted for a plain pointer click on a chip.
pub const ATOM_ACTION_CLICK: &str = "click";

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use xiaomu_core::document::{NodeContent, NodeKind, NodeStoreBuilder};

    #[derive(Default)]
    struct Recorder {
        actions: RefCell<Vec<(NodeId, String)>>,
    }

    impl InlineAtomHostCapability for Recorder {
        fn atom_action(&self, action: AtomAction) {
            self.actions
                .borrow_mut()
                .push((action.node, action.action.to_string()));
        }
    }

    #[test]
    fn capability_receives_stable_keys_and_canonical_snapshot() {
        let mut builder = NodeStoreBuilder::new();
        let atom = builder
            .insert(
                NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(
                    xiaomu_core::document::InlineAtomContent::new("@a").unwrap(),
                ),
            )
            .unwrap();

        let recorder = Rc::new(Recorder::default());
        recorder.atom_action(AtomAction {
            node: atom,
            kind: AtomKind::new("mention").unwrap(),
            action: ATOM_ACTION_CLICK.into(),
            attrs: NodeAttrs::empty(),
        });

        let actions = recorder.actions.borrow();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], (atom, "click".to_owned()));
    }
}
