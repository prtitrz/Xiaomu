//! Linear ordered-boundary validation for copying normalized inline content.

use std::collections::BTreeSet;

use crate::document::InlineAtomPlacement;
use crate::text::TextOffset;
use crate::{Error, Result};

use super::InlineContent;

impl InlineContent {
    /// Keeps normalized runs and installs an ordered set of remapped atoms.
    /// This private materialization seam validates anchors in one run walk,
    /// not one full run scan per atom. No unchecked offsets or duplicate IDs
    /// enter canonical content. Equal-boundary atom order is unchanged.
    pub(crate) fn with_replaced_atom_placements(
        &self,
        atoms: Vec<InlineAtomPlacement>,
    ) -> Result<Self> {
        let mut identities = BTreeSet::new();
        let mut previous = TextOffset::ZERO;
        let mut run_index = 0usize;
        let mut run_start = 0usize;
        let length = self.len_bytes();
        for atom in &atoms {
            let offset = atom.text_offset().as_usize();
            if atom.text_offset() < previous {
                return Err(Error::InvalidTransaction);
            }
            previous = atom.text_offset();
            if offset > length {
                return Err(Error::TextOutOfBounds {
                    offset,
                    len: length,
                });
            }
            if !identities.insert(atom.atom()) {
                return Err(Error::DuplicateInlineAtomReference);
            }
            while run_index < self.runs.len()
                && offset > run_start + self.runs[run_index].len_bytes()
            {
                run_start += self.runs[run_index].len_bytes();
                run_index += 1;
            }
            if let Some(run) = self.runs.get(run_index) {
                run.text()
                    .validate_offset(TextOffset::from_validated_byte_index(offset - run_start))?;
            }
        }
        Ok(Self {
            runs: self.runs.clone(),
            atoms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Mark, MarkSet, NodeId, TextRun};
    use crate::text::TextBuffer;

    fn offset(index: usize) -> TextOffset {
        TextBuffer::from("0123456789").offset_at(index).unwrap()
    }
    fn atom(id: u64, index: usize) -> InlineAtomPlacement {
        InlineAtomPlacement::new(NodeId::from_allocated(id), offset(index))
    }

    #[test]
    fn remap_preserves_same_anchor_order_marks_and_multibyte_boundaries() {
        let inline = InlineContent::new([
            TextRun::new("A🙂", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
            TextRun::new("中B", MarkSet::new([Mark::Italic]).unwrap()).unwrap(),
        ])
        .unwrap();
        let atoms = vec![atom(3, 1), atom(1, 1), atom(2, 5), atom(4, 9)];
        let mapped = inline.with_replaced_atom_placements(atoms.clone()).unwrap();
        assert_eq!(mapped.runs(), inline.runs());
        assert_eq!(mapped.atoms(), atoms);
        assert!(inline.atoms().is_empty());
    }

    #[test]
    fn remap_rejects_bad_utf8_bounds_order_and_duplicate_identity() {
        let inline = InlineContent::new([TextRun::new("A🙂", MarkSet::empty()).unwrap()]).unwrap();
        assert_eq!(
            inline
                .with_replaced_atom_placements(vec![atom(1, 2)])
                .unwrap_err(),
            Error::InvalidTextBoundary { offset: 2 }
        );
        assert!(matches!(
            inline.with_replaced_atom_placements(vec![atom(1, 6)]),
            Err(Error::TextOutOfBounds { .. })
        ));
        assert_eq!(
            inline
                .with_replaced_atom_placements(vec![atom(1, 5), atom(2, 1)])
                .unwrap_err(),
            Error::InvalidTransaction
        );
        assert_eq!(
            inline
                .with_replaced_atom_placements(vec![atom(1, 1), atom(1, 5)])
                .unwrap_err(),
            Error::DuplicateInlineAtomReference
        );
        assert!(
            InlineContent::empty()
                .with_replaced_atom_placements(vec![atom(1, 0), atom(2, 0)])
                .is_ok()
        );
    }
}
