//! Canonical node content shapes and inline normalization.

use crate::Result;

use super::{NodeId, TextRun};

/// Normalized inline text content for paragraph-like nodes.
///
/// Adjacent runs with identical marks are merged so run segmentation does not
/// become accidental canonical state. Empty inline content is valid and is how
/// an empty paragraph or heading is represented.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineContent {
    runs: Vec<TextRun>,
}

impl InlineContent {
    /// Creates normalized inline content.
    pub fn new(runs: impl IntoIterator<Item = TextRun>) -> Result<Self> {
        let mut normalized: Vec<TextRun> = Vec::new();

        for run in runs {
            if let Some(previous) = normalized.last_mut()
                && previous.marks() == run.marks()
            {
                let mut text = String::with_capacity(previous.len_bytes() + run.len_bytes());
                text.push_str(previous.text().as_str());
                text.push_str(run.text().as_str());
                let marks = previous.marks().clone();
                *previous = TextRun::new(text, marks)?;
                continue;
            }
            normalized.push(run);
        }

        Ok(Self { runs: normalized })
    }

    /// Returns empty inline content.
    #[must_use]
    pub const fn empty() -> Self {
        Self { runs: Vec::new() }
    }

    /// Returns normalized text runs.
    #[must_use]
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// Returns whether no text runs are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Returns the total UTF-8 byte length of the represented text.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.runs.iter().map(TextRun::len_bytes).sum()
    }
}

/// Canonical content shape of a document node.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeContent {
    /// Normalized inline text content.
    Inline(InlineContent),
    /// Ordered child-node references.
    Children(Vec<NodeId>),
    /// Atomic block with no editable child content.
    Atomic,
}

impl NodeContent {
    /// Creates empty inline content.
    #[must_use]
    pub const fn empty_inline() -> Self {
        Self::Inline(InlineContent::empty())
    }

    /// Creates ordered child content.
    #[must_use]
    pub fn children(children: impl IntoIterator<Item = NodeId>) -> Self {
        Self::Children(children.into_iter().collect())
    }

    /// Returns inline content when this node has an inline shape.
    #[must_use]
    pub const fn as_inline(&self) -> Option<&InlineContent> {
        match self {
            Self::Inline(content) => Some(content),
            _ => None,
        }
    }

    /// Returns child IDs when this node has a structural child shape.
    #[must_use]
    pub fn as_children(&self) -> Option<&[NodeId]> {
        match self {
            Self::Children(children) => Some(children),
            _ => None,
        }
    }

    /// Returns whether this is atomic content.
    #[must_use]
    pub const fn is_atomic(&self) -> bool {
        matches!(self, Self::Atomic)
    }
}
