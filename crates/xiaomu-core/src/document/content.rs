//! Canonical node content shapes and inline normalization.

use std::collections::BTreeSet;

use crate::text::TextOffset;
use crate::{Error, Result};

use super::{InlineAtomContent, InlineAtomPlacement, NodeId, TextRun};

/// Normalized mixed inline content for paragraph-like nodes.
///
/// Text remains a sequence of normalized [`TextRun`] values. Inline atoms are
/// referenced separately by validated UTF-8 text boundaries so adding atomic
/// content does not change the meaning of [`TextOffset`]. Multiple atoms may
/// share one boundary; their stable vector order defines the atom ordinal used
/// by [`crate::selection::InlinePoint`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineContent {
    runs: Vec<TextRun>,
    atoms: Vec<InlineAtomPlacement>,
}

impl InlineContent {
    /// Creates normalized text-only inline content.
    pub fn new(runs: impl IntoIterator<Item = TextRun>) -> Result<Self> {
        Self::with_atoms(runs, [])
    }

    /// Creates normalized mixed inline content with ordered atom placements.
    ///
    /// Placements are stably normalized by `text_offset`; atoms sharing one
    /// text boundary keep caller order. A referenced atom identity may occur
    /// only once in one inline parent. Full document validation later verifies
    /// that each identity exists and has inline-atom node semantics.
    pub fn with_atoms(
        runs: impl IntoIterator<Item = TextRun>,
        atoms: impl IntoIterator<Item = InlineAtomPlacement>,
    ) -> Result<Self> {
        let runs = normalize_runs(runs)?;
        let mut content = Self {
            runs,
            atoms: atoms.into_iter().collect(),
        };

        let mut identities = BTreeSet::new();
        for placement in &content.atoms {
            content.validate_offset(placement.text_offset())?;
            if !identities.insert(placement.atom()) {
                return Err(Error::DuplicateInlineAtomReference);
            }
        }
        content
            .atoms
            .sort_by_key(|placement| placement.text_offset().as_usize());
        Ok(content)
    }

    /// Returns empty inline content.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            runs: Vec::new(),
            atoms: Vec::new(),
        }
    }

    /// Returns normalized text runs.
    #[must_use]
    pub fn runs(&self) -> &[TextRun] {
        &self.runs
    }

    /// Returns ordered inline-atom placements.
    #[must_use]
    pub fn atoms(&self) -> &[InlineAtomPlacement] {
        &self.atoms
    }

    /// Returns the number of atoms anchored at `offset`.
    #[must_use]
    pub fn atom_count_at(&self, offset: TextOffset) -> usize {
        self.atoms
            .iter()
            .filter(|placement| placement.text_offset() == offset)
            .count()
    }

    /// Returns whether this inline content has neither text nor atoms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.atoms.is_empty()
    }

    /// Returns the total UTF-8 byte length of represented text only.
    ///
    /// Inline atoms do not consume fake bytes in this coordinate space.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.runs.iter().map(TextRun::len_bytes).sum()
    }

    /// Returns a validated coordinate for `byte_index` in the concatenated
    /// text of all runs.
    ///
    /// Mirrors [`crate::text::TextBuffer::offset_at`]: the index must be in
    /// bounds and on a UTF-8 scalar boundary. This gives non-Core layers a
    /// direct way to construct offsets against inline content without
    /// reassembling the concatenated text themselves.
    pub fn offset_at(&self, byte_index: usize) -> Result<TextOffset> {
        self.validate_offset(TextOffset::from_validated_byte_index(byte_index))?;
        Ok(TextOffset::from_validated_byte_index(byte_index))
    }

    /// Validates that `offset` is a usable coordinate in the concatenated
    /// text of all runs.
    ///
    /// Run boundaries are always valid coordinates because runs are never
    /// empty; offsets inside a run must fall on UTF-8 scalar boundaries of
    /// that run's text.
    pub fn validate_offset(&self, offset: TextOffset) -> Result<()> {
        let raw = offset.as_usize();
        if raw > self.len_bytes() {
            return Err(Error::TextOutOfBounds {
                offset: raw,
                len: self.len_bytes(),
            });
        }

        let mut remaining = raw;
        for run in &self.runs {
            let len = run.len_bytes();
            if remaining <= len {
                return run
                    .text()
                    .validate_offset(TextOffset::from_validated_byte_index(remaining));
            }
            remaining -= len;
        }

        // `raw == total length` with no runs means empty inline content,
        // whose only valid text coordinate is zero.
        Ok(())
    }
}

fn normalize_runs(runs: impl IntoIterator<Item = TextRun>) -> Result<Vec<TextRun>> {
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

    Ok(normalized)
}

/// Canonical content shape of a document node.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeContent {
    /// Normalized mixed inline content.
    Inline(InlineContent),
    /// Ordered child-node references.
    Children(Vec<NodeId>),
    /// Canonical payload for one atomic inline extension node.
    InlineAtom(InlineAtomContent),
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

    /// Returns inline-atom content when this node has atom shape.
    #[must_use]
    pub const fn as_inline_atom(&self) -> Option<&InlineAtomContent> {
        match self {
            Self::InlineAtom(content) => Some(content),
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

    /// Returns whether this is atomic block content.
    #[must_use]
    pub const fn is_atomic(&self) -> bool {
        matches!(self, Self::Atomic)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{MarkSet, NodeAttrs, NodeKind, NodeStoreBuilder};

    fn inline(text: &str) -> InlineContent {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    }

    #[test]
    fn offset_at_validates_boundaries_and_bounds() {
        let content = inline("a中");

        assert_eq!(content.offset_at(0).unwrap().as_usize(), 0);
        assert_eq!(content.offset_at(1).unwrap().as_usize(), 1);
        assert_eq!(content.offset_at(4).unwrap().as_usize(), 4);
        assert!(matches!(
            content.offset_at(2),
            Err(Error::InvalidTextBoundary { offset: 2 })
        ));
        assert!(matches!(
            content.offset_at(5),
            Err(Error::TextOutOfBounds { offset: 5, len: 4 })
        ));
        assert_eq!(InlineContent::empty().offset_at(0).unwrap().as_usize(), 0);
    }

    #[test]
    fn placements_normalize_by_offset_and_keep_same_offset_order() {
        let mut builder = NodeStoreBuilder::new();
        let first = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let second = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let third = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let text = inline("ab");
        let at_zero = InlineAtomPlacement::new(first, text.offset_at(0).unwrap());
        let at_one_first = InlineAtomPlacement::new(second, text.offset_at(1).unwrap());
        let at_one_second = InlineAtomPlacement::new(third, text.offset_at(1).unwrap());

        let mixed = InlineContent::with_atoms(
            text.runs().iter().cloned(),
            [at_one_first, at_zero, at_one_second],
        )
        .unwrap();

        assert_eq!(mixed.atoms(), &[at_zero, at_one_first, at_one_second]);
        assert_eq!(mixed.atom_count_at(text.offset_at(1).unwrap()), 2);
        assert_eq!(mixed.len_bytes(), 2);
        assert!(!mixed.is_empty());
    }

    #[test]
    fn placement_rejects_invalid_boundaries_and_duplicate_identity() {
        let builder = NodeStoreBuilder::new();
        let atom = builder.peek_next_id();
        let run = TextRun::new("中", MarkSet::empty()).unwrap();
        let invalid = InlineAtomPlacement::new(atom, TextOffset::from_validated_byte_index(1));
        assert_eq!(
            InlineContent::with_atoms([run.clone()], [invalid]),
            Err(Error::InvalidTextBoundary { offset: 1 })
        );

        let at_start = InlineAtomPlacement::new(atom, TextOffset::ZERO);
        assert_eq!(
            InlineContent::with_atoms([run], [at_start, at_start]),
            Err(Error::DuplicateInlineAtomReference)
        );
    }
}
