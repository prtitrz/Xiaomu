//! Explicit whole-block selection identity, distinct from ordinary gap ranges.

use xiaomu_core::document::{NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::mapping::{ChangeMap, MapBias, MappedPosition};
use xiaomu_core::selection::{NodeGap, NodeSelection};

use super::{DocumentPosition, DocumentSelection};
use crate::session::{DocumentSession, SessionError, SessionOutcome};

impl DocumentSelection {
    /// Selects one complete built-in block subtree in `document`.
    ///
    /// Supports paragraphs, headings, quotes, ordinary/task lists, code
    /// blocks, rules, images and whole tables. Document roots, list/task
    /// items, table rows/cells, inline atoms and custom kinds reject
    /// with `SelectionInvalid`. The block must be a reachable structural child.
    ///
    /// Its endpoints are the gaps immediately before and after the block,
    /// but private identity provenance distinguishes this from an ordinary
    /// gap range, including a sole root child that spans the whole document.
    /// This is a non-collapsed selection; atomic compatibility selections
    /// created with `collapsed(DocumentPosition::Atomic(..))` stay distinct.
    /// The root tag rejects a different root ID; IDs are document-local, so
    /// this is not a globally unique document/session ownership token.
    pub fn node(document: &XiaomuDocument, node: NodeId) -> Result<Self, SessionError> {
        let target = document.node(node).ok_or(SessionError::SelectionInvalid)?;
        if !matches!(
            target.kind(),
            NodeKind::Paragraph
                | NodeKind::Heading(_)
                | NodeKind::Quote
                | NodeKind::BulletList
                | NodeKind::OrderedList
                | NodeKind::TaskList
                | NodeKind::CodeBlock
                | NodeKind::HorizontalRule
                | NodeKind::Image
                | NodeKind::Table
        ) {
            return Err(SessionError::SelectionInvalid);
        }
        let parent = document
            .parent_of(node)
            .ok_or(SessionError::SelectionInvalid)?;
        let children = document
            .node(parent)
            .and_then(|parent| parent.content().as_children())
            .ok_or(SessionError::SelectionInvalid)?;
        let index = children
            .iter()
            .position(|child| *child == node)
            .ok_or(SessionError::SelectionInvalid)?;
        let mut selection = Self::new(NodeGap::new(parent, index), NodeGap::new(parent, index + 1));
        selection.node_selection = Some((node, document.root()));
        Ok(selection)
    }

    /// Returns the explicitly selected whole block, never inferring identity
    /// from endpoint gaps. Legacy atomic selections return `None`.
    #[must_use]
    pub const fn as_node_selection(&self) -> Option<NodeId> {
        match self.node_selection {
            Some((node, _)) => Some(node),
            None => None,
        }
    }

    pub(super) fn validate_node_selection(
        &self,
        document: &XiaomuDocument,
    ) -> Result<(), SessionError> {
        let expected = self
            .preserved_node_selection(document)?
            .ok_or(SessionError::SelectionInvalid)?;
        if *self != expected {
            return Err(SessionError::SelectionInvalid);
        }
        Ok(())
    }

    /// Identity-preserving plans refresh structural coordinates in the final
    /// snapshot, including when a remove/restore temporarily removed the node.
    pub(in crate::session) fn preserved_node_selection(
        &self,
        document: &XiaomuDocument,
    ) -> Result<Option<Self>, SessionError> {
        let Some((node, root)) = self.node_selection else {
            return Ok(None);
        };
        if root != document.root() {
            return Err(SessionError::SelectionInvalid);
        }
        Self::node(document, node).map(Some)
    }

    pub(super) fn map_node_selection(
        &self,
        changes: &ChangeMap,
        document: &XiaomuDocument,
    ) -> Result<Self, SessionError> {
        self.validate_node_selection(document)?;
        let (node, root) = self.node_selection.ok_or(SessionError::SelectionInvalid)?;
        let MappedPosition::Mapped(mapped) = changes.map_node_selection(NodeSelection::new(node))
        else {
            return Err(SessionError::SelectionDeleted);
        };
        let (DocumentPosition::Gap(before), DocumentPosition::Gap(after)) =
            (self.anchor, self.focus)
        else {
            return Err(SessionError::SelectionInvalid);
        };
        // Bias inward: adjacent inserted siblings must not join this selection.
        // Selecting a split block follows its original identity, not its tail.
        let MappedPosition::Mapped(before) = changes.map_node_gap(before, MapBias::End) else {
            return Err(SessionError::SelectionDeleted);
        };
        let MappedPosition::Mapped(after) = changes.map_node_gap(after, MapBias::Start) else {
            return Err(SessionError::SelectionDeleted);
        };
        let mut selection = Self::new(before, after);
        selection.node_selection = Some((mapped.node_id(), root));
        Ok(selection)
    }
}

impl DocumentSession {
    /// Selects a whole built-in block without editing the document or history.
    ///
    /// Invalid targets leave selection, marks, grouping and listeners unchanged.
    /// Whole-block clipboard projection is supported. Default selection-driven
    /// editing/navigation is explicitly unsupported until a node-aware policy
    /// handles it; callers can install another selection to leave this mode.
    pub fn set_node_selection(&mut self, node: NodeId) -> Result<SessionOutcome, SessionError> {
        let selection = DocumentSelection::node(self.document(), node)?;
        self.set_document_selection(selection)
    }
}
