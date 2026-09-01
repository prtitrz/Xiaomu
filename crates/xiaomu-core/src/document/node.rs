//! Canonical document node value.

use crate::{Error, Result};

use super::{NodeAttrs, NodeContent, NodeId, NodeKind};

/// Immutable canonical document node.
///
/// Fields are intentionally private. Creation and later mutation paths must
/// preserve kind/content invariants instead of exposing a mutable struct to
/// hosts or extensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    id: NodeId,
    kind: NodeKind,
    attrs: NodeAttrs,
    content: NodeContent,
}

impl Node {
    pub(crate) fn new(
        id: NodeId,
        kind: NodeKind,
        attrs: NodeAttrs,
        content: NodeContent,
    ) -> Result<Self> {
        validate_content_shape(&kind, &content)?;
        Ok(Self {
            id,
            kind,
            attrs,
            content,
        })
    }

    /// Returns this node's stable identity.
    #[must_use]
    pub const fn id(&self) -> NodeId {
        self.id
    }

    /// Returns this node's semantic kind.
    #[must_use]
    pub const fn kind(&self) -> &NodeKind {
        &self.kind
    }

    /// Returns canonical node attributes.
    #[must_use]
    pub const fn attrs(&self) -> &NodeAttrs {
        &self.attrs
    }

    /// Returns canonical node content.
    #[must_use]
    pub const fn content(&self) -> &NodeContent {
        &self.content
    }

    #[cfg(test)]
    pub(crate) fn with_content(&self, content: NodeContent) -> Result<Self> {
        Self::new(self.id, self.kind.clone(), self.attrs.clone(), content)
    }
}

fn validate_content_shape(kind: &NodeKind, content: &NodeContent) -> Result<()> {
    let valid = match kind {
        NodeKind::Document
        | NodeKind::Quote
        | NodeKind::BulletList
        | NodeKind::OrderedList
        | NodeKind::ListItem => matches!(content, NodeContent::Children(_)),
        NodeKind::Paragraph | NodeKind::Heading(_) | NodeKind::CodeBlock => {
            matches!(content, NodeContent::Inline(_))
        }
        NodeKind::InlineAtom(_) => matches!(content, NodeContent::InlineAtom(_)),
        NodeKind::HorizontalRule | NodeKind::Image => matches!(content, NodeContent::Atomic),
        NodeKind::Custom(_) => true,
    };

    if valid {
        Ok(())
    } else {
        Err(Error::InvalidNodeContent)
    }
}
