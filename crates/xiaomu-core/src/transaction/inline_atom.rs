//! Atom-aware text replacement over mixed inline content.
//!
//! The legacy text-only replacement fails closed at atom seams because a
//! bare `TextRange` cannot distinguish the caret gaps around same-boundary
//! atoms. This module implements the P4 contract that consumes an
//! [`InlinePoint`] start boundary, so the atom ordinal survives the edit and
//! the seam atoms are split or shifted explicitly instead of silently.

use crate::document::{InlineAtomPlacement, InlineContent};
use crate::selection::InlinePoint;
use crate::text::{TextOffset, TextRange};
use crate::{Error, Result};

use super::inline::{rebuild, splice_pieces};

/// Returns inline content with `[at.text_offset(), end)` replaced by
/// `replacement` at the mixed-inline caret gap addressed by `at`.
///
/// Placement rules (see [`crate::transaction::TransactionStep::ReplaceInlineText`]):
///
/// - atoms anchored before the seam, and seam atoms with ordinal
///   `< at.atom_index()`, keep their anchor;
/// - an empty range is a pure insertion at the seam: seam atoms with ordinal
///   `>= at.atom_index()` move after the replacement text;
/// - a non-empty range fails closed when an atom would fall inside the
///   replaced region — seam atoms with ordinal `>= at.atom_index()` or any
///   atom anchored strictly between the boundaries — because the step never
///   deletes atomic content as a side effect of a text edit;
/// - atoms anchored at or after `end` shift by the byte-length delta.
pub(super) fn replace_inline_text(
    inline: &InlineContent,
    at: InlinePoint,
    end: TextOffset,
    replacement: &str,
) -> Result<InlineContent> {
    let start = at.text_offset();
    if start > end {
        return Err(Error::InvalidTextRange {
            start: start.as_usize(),
            end: end.as_usize(),
        });
    }
    inline.validate_offset(start)?;
    inline.validate_offset(end)?;
    if at.atom_index() > inline.atom_count_at(start) {
        return Err(Error::InvalidSelection);
    }

    let atoms = mapped_atom_placements_after_inline_replace(inline, at, end, replacement.len())?;
    let range = TextRange::new(start, end)?;
    let pieces = splice_pieces(inline, range, replacement);
    rebuild(pieces, &atoms)
}

/// Remaps every atom placement across the replaced span, or fails closed.
///
/// Placements arrive sorted by text offset with same-boundary atoms in
/// canonical order, so the same-boundary ordinal of a placement is the number
/// of preceding placements sharing its offset.
fn mapped_atom_placements_after_inline_replace(
    inline: &InlineContent,
    at: InlinePoint,
    end: TextOffset,
    replacement_len: usize,
) -> Result<Vec<InlineAtomPlacement>> {
    let start = at.text_offset().as_usize();
    let end = end.as_usize();
    let seam = at.atom_index();
    let removed_len = end - start;

    let mut mapped = Vec::with_capacity(inline.atoms().len());
    let mut previous_offset = None;
    let mut ordinal = 0usize;
    for placement in inline.atoms() {
        let offset = placement.text_offset().as_usize();
        if previous_offset == Some(offset) {
            ordinal += 1;
        } else {
            ordinal = 0;
            previous_offset = Some(offset);
        }

        let anchor = if offset < start || (offset == start && ordinal < seam) {
            placement.text_offset()
        } else if offset == start {
            if end > start {
                // The replaced region begins at this caret gap, so every
                // seam atom after it would be deleted by a text edit.
                return Err(Error::InvalidTransaction);
            }
            // Pure insertion at the seam: later seam atoms move after the
            // inserted text.
            TextOffset::from_validated_byte_index(start + replacement_len)
        } else if offset < end {
            return Err(Error::InvalidTransaction);
        } else {
            TextOffset::from_validated_byte_index(offset - removed_len + replacement_len)
        };
        mapped.push(InlineAtomPlacement::new(placement.atom(), anchor));
    }

    Ok(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{MarkSet, NodeContent, NodeKind, NodeStoreBuilder, TextRun};

    fn inline(text: &str) -> InlineContent {
        InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
    }

    fn offset_at(raw: usize) -> TextOffset {
        const SCRATCH: &str = "00000000000000000000000000000000";
        crate::text::TextBuffer::from(SCRATCH)
            .offset_at(raw)
            .unwrap()
    }

    fn atom_ids(count: usize) -> Vec<crate::document::NodeId> {
        let mut builder = NodeStoreBuilder::new();
        (0..count)
            .map(|_| {
                builder
                    .insert(
                        NodeKind::Paragraph,
                        crate::document::NodeAttrs::empty(),
                        NodeContent::empty_inline(),
                    )
                    .unwrap()
            })
            .collect()
    }

    fn seam(node: crate::document::NodeId, offset: usize, atom_index: usize) -> InlinePoint {
        InlinePoint::new(
            node,
            offset_at(offset),
            atom_index,
            crate::selection::CursorAffinity::Before,
        )
    }

    fn text_of(content: &InlineContent) -> String {
        content
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect()
    }

    #[test]
    fn seam_insertion_splits_same_boundary_atoms_by_ordinal() {
        let ids = atom_ids(2);
        let content = InlineContent::with_atoms(
            [TextRun::new("AB", MarkSet::empty()).unwrap()],
            [
                InlineAtomPlacement::new(ids[0], offset_at(1)),
                InlineAtomPlacement::new(ids[1], offset_at(1)),
            ],
        )
        .unwrap();

        // Inserting between the two atoms pushes only later seam atoms past
        // the inserted text.
        let next = replace_inline_text(&content, seam(ids[0], 1, 1), offset_at(1), "X").unwrap();
        assert_eq!(text_of(&next), "AXB");
        assert_eq!(next.atoms()[0].atom(), ids[0]);
        assert_eq!(next.atoms()[0].text_offset(), offset_at(1));
        assert_eq!(next.atoms()[1].atom(), ids[1]);
        assert_eq!(next.atoms()[1].text_offset(), offset_at(2));

        // Inserting before both atoms moves both of them.
        let before = replace_inline_text(&content, seam(ids[0], 1, 0), offset_at(1), "X").unwrap();
        assert_eq!(text_of(&before), "AXB");
        assert_eq!(before.atoms()[0].text_offset(), offset_at(2));
        assert_eq!(before.atoms()[1].text_offset(), offset_at(2));

        // Inserting after both atoms leaves both anchors in place.
        let after = replace_inline_text(&content, seam(ids[0], 1, 2), offset_at(1), "X").unwrap();
        assert_eq!(text_of(&after), "AXB");
        assert_eq!(after.atoms()[0].text_offset(), offset_at(1));
        assert_eq!(after.atoms()[1].text_offset(), offset_at(1));
    }

    #[test]
    fn replacement_after_seam_atom_preserves_it_and_shifts_later_anchors() {
        let ids = atom_ids(2);
        let content = InlineContent::with_atoms(
            [TextRun::new("ABC", MarkSet::empty()).unwrap()],
            [
                InlineAtomPlacement::new(ids[0], offset_at(1)),
                InlineAtomPlacement::new(ids[1], offset_at(2)),
            ],
        )
        .unwrap();

        // Replace "B" with "XY" starting after the atom anchored at 1.
        let next = replace_inline_text(&content, seam(ids[0], 1, 1), offset_at(2), "XY").unwrap();
        assert_eq!(text_of(&next), "AXYC");
        assert_eq!(next.atoms()[0].text_offset(), offset_at(1));
        assert_eq!(next.atoms()[1].text_offset(), offset_at(3));
    }

    #[test]
    fn replacement_region_containing_atoms_fails_closed() {
        let ids = atom_ids(2);
        let content = InlineContent::with_atoms(
            [TextRun::new("AB", MarkSet::empty()).unwrap()],
            [
                InlineAtomPlacement::new(ids[0], offset_at(1)),
                InlineAtomPlacement::new(ids[1], offset_at(1)),
            ],
        )
        .unwrap();

        // Seam atoms after the caret are inside a non-empty region.
        assert_eq!(
            replace_inline_text(&content, seam(ids[0], 1, 1), offset_at(2), "X"),
            Err(Error::InvalidTransaction)
        );
        // A caret gap that does not exist at the seam is invalid.
        assert_eq!(
            replace_inline_text(&content, seam(ids[0], 1, 3), offset_at(1), "X"),
            Err(Error::InvalidSelection)
        );

        let inside = InlineContent::with_atoms(
            [TextRun::new("ABC", MarkSet::empty()).unwrap()],
            [InlineAtomPlacement::new(ids[0], offset_at(2))],
        )
        .unwrap();
        // An atom anchored strictly inside the region fails closed.
        assert_eq!(
            replace_inline_text(&inside, seam(ids[0], 1, 0), offset_at(3), "X"),
            Err(Error::InvalidTransaction)
        );
    }

    #[test]
    fn unordered_boundaries_and_invalid_offsets_fail_closed() {
        let ids = atom_ids(1);
        let content = inline("AB");

        assert!(matches!(
            replace_inline_text(&content, seam(ids[0], 2, 0), offset_at(1), "X"),
            Err(Error::InvalidTextRange { start: 2, end: 1 })
        ));
        assert!(replace_inline_text(&content, seam(ids[0], 1, 0), offset_at(9), "X").is_err());
    }
}
