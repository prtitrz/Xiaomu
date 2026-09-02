//! Host-neutral inline-atom rendering seam.
//!
//! The registry maps a stable [`AtomKind`] to a renderer. Renderers only ever
//! see canonical atom data — kind key, `fallback_text`, and extension
//! attributes — so a host can style or label atoms without its business types
//! entering Core, Runtime, or the rendering pipeline itself. A kind without a
//! registered renderer fails soft to the deterministic fallback: the atom
//! displays and reads exactly its `fallback_text`.

use std::collections::BTreeMap;
use std::rc::Rc;

use xiaomu_core::document::{AtomKind, NodeAttrs, NodeId};

/// Canonical projection of one inline atom handed to renderers.
///
/// The view is a detached snapshot: renderers cannot mutate the document and
/// never see host-side business objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineAtomView {
    node: NodeId,
    kind: AtomKind,
    fallback_text: String,
    attrs: NodeAttrs,
}

impl InlineAtomView {
    /// Builds the canonical projection of one inline-atom node.
    #[must_use]
    pub fn new(
        node: NodeId,
        kind: AtomKind,
        fallback_text: impl Into<String>,
        attrs: NodeAttrs,
    ) -> Self {
        Self {
            node,
            kind,
            fallback_text: fallback_text.into(),
            attrs,
        }
    }

    /// Returns the stable canonical identity of the atom node.
    #[must_use]
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the stable semantic kind key.
    #[must_use]
    pub const fn kind(&self) -> &AtomKind {
        &self.kind
    }

    /// Returns the host-neutral fallback text (plain text and accessibility).
    #[must_use]
    pub fn fallback_text(&self) -> &str {
        &self.fallback_text
    }

    /// Returns the extension payload attributes.
    #[must_use]
    pub const fn attrs(&self) -> &NodeAttrs {
        &self.attrs
    }
}

/// Renders one inline atom into the block's visual projection.
///
/// P4.4 starts with the display text contract: the returned string is
/// spliced into the paragraph's visual text at the atom's anchored boundary.
/// Visual styling (chip background, border, label) extends this trait in the
/// paint slice without changing the canonical coordinate contract.
pub trait InlineAtomRenderer {
    /// Returns the display text for the atom in the visual projection.
    fn display_text(&self, atom: &InlineAtomView) -> String;
}

/// The deterministic missing-renderer fallback: display exactly the atom's
/// `fallback_text`.
///
/// This is also the accessibility semantics, so an unknown or unregistered
/// atom kind renders and reads identically everywhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct FallbackAtomRenderer;

impl InlineAtomRenderer for FallbackAtomRenderer {
    fn display_text(&self, atom: &InlineAtomView) -> String {
        atom.fallback_text().to_owned()
    }
}

/// Registry of renderers keyed by stable atom kind.
///
/// Hosts register renderers before building a view; lookup is by the
/// [`AtomKind`] key and a missing entry resolves to
/// [`FallbackAtomRenderer`], never to a panic or a dropped atom.
#[derive(Default)]
pub struct InlineAtomRendererRegistry {
    renderers: BTreeMap<String, Rc<dyn InlineAtomRenderer>>,
}

impl InlineAtomRendererRegistry {
    /// Creates an empty registry: every atom falls back until a host
    /// registers renderers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `renderer` for `kind`, replacing any previous entry.
    pub fn register(&mut self, kind: &AtomKind, renderer: Rc<dyn InlineAtomRenderer>) {
        self.renderers.insert(kind.as_str().to_owned(), renderer);
    }

    /// Resolves the renderer for `kind`, or the deterministic fallback.
    #[must_use]
    pub fn renderer_for(&self, kind: &AtomKind) -> Rc<dyn InlineAtomRenderer> {
        self.renderers
            .get(kind.as_str())
            .cloned()
            .unwrap_or_else(|| Rc::new(FallbackAtomRenderer))
    }

    /// Returns whether a specific renderer is registered for `kind`.
    #[must_use]
    pub fn has_custom_renderer(&self, kind: &AtomKind) -> bool {
        self.renderers.contains_key(kind.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::NodeStoreBuilder;

    struct UppercaseRenderer;

    impl InlineAtomRenderer for UppercaseRenderer {
        fn display_text(&self, atom: &InlineAtomView) -> String {
            atom.fallback_text().to_uppercase()
        }
    }

    fn view(kind: &str, fallback: &str) -> InlineAtomView {
        // Identity is opaque bookkeeping for the renderer contract; the
        // builder's next id is a convenient valid handle for tests.
        let node = NodeStoreBuilder::new().peek_next_id();
        InlineAtomView::new(
            node,
            AtomKind::new(kind).unwrap(),
            fallback,
            NodeAttrs::empty(),
        )
    }

    #[test]
    fn missing_renderer_falls_back_to_fallback_text() {
        let registry = InlineAtomRendererRegistry::new();
        let atom = view("mention", "@Ann");
        assert_eq!(
            registry.renderer_for(atom.kind()).display_text(&atom),
            "@Ann"
        );
        assert!(!registry.has_custom_renderer(atom.kind()));
    }

    #[test]
    fn registered_renderer_overrides_and_unknown_kinds_still_fall_back() {
        let mut registry = InlineAtomRendererRegistry::new();
        registry.register(
            &AtomKind::new("mention").unwrap(),
            Rc::new(UppercaseRenderer),
        );

        let mention = view("mention", "@Ann");
        let reference = view("reference", "ref");
        assert_eq!(
            registry.renderer_for(mention.kind()).display_text(&mention),
            "@ANN"
        );
        assert_eq!(
            registry
                .renderer_for(reference.kind())
                .display_text(&reference),
            "ref"
        );
        assert!(registry.has_custom_renderer(mention.kind()));
        assert!(!registry.has_custom_renderer(reference.kind()));
    }
}
