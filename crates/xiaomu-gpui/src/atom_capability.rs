//! Host capability seam for inline-atom activations.
//!
//! Hosts react to user activation on an atom chip without leaking business
//! types into Core or Runtime: the callback receives only stable keys and a
//! canonical snapshot (atom [`NodeId`], [`AtomKind`], action key, attrs).

use std::rc::Rc;

use gpui::SharedString;
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
