//! Inline-atom transaction application and exact inverse capture.

use std::collections::BTreeSet;

use crate::document::{
    InlineAtomContent, InlineAtomPlacement, InlineContent, Node, NodeAttrs, NodeId, NodeKind,
};
use crate::mapping::StepMap;
use crate::selection::{CursorAffinity, InlinePoint};
use crate::text::{TextOffset, TextRange};
use crate::{Error, Result};

use super::ApplyContext;
use crate::transaction::{inline_atom, inverse, step::TransactionStep};

impl ApplyContext {
    /// Applies the atom-aware mixed-inline text replacement.
    ///
    /// The seam ordinal is consumed here: application validates that the
    /// caret gap exists and lets [`inline_atom::replace_inline_text`] decide
    /// which atom placements survive, shift, or fail the step.
    pub(super) fn apply_replace_inline_text(
        &mut self,
        at: InlinePoint,
        end: TextOffset,
        replacement: &str,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let node = at.node_id();
        let content = self.inline_content(node)?;
        let range = TextRange::new(at.text_offset(), end)?;
        let spans = inverse::spans_within(&content, range)?;
        let next = inline_atom::replace_inline_text(&content, at, end, replacement)?;
        self.rewrite_node(
            node,
            self.attrs_of(node)?,
            crate::document::NodeContent::Inline(next),
        )?;

        let step_map = StepMap::InlineTextReplaced {
            node,
            range,
            replacement_len: replacement.len(),
            seam_atom_index: at.atom_index(),
        };
        let inverse_steps =
            inverse::replace_inline_text_inverse(at, range, replacement, &content, &spans);
        Ok((Some(step_map), inverse_steps))
    }

    pub(super) fn apply_insert_inline_atom(
        &mut self,
        at: InlinePoint,
        kind: &crate::document::AtomKind,
        attrs: NodeAttrs,
        content: InlineAtomContent,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        self.validate_inline_gap(at)?;
        let id = self.allocate_node(
            NodeKind::InlineAtom(kind.clone()),
            attrs,
            crate::document::NodeContent::InlineAtom(content),
        )?;
        self.attach_inline_atom(at, id)?;

        let step_map = StepMap::InlineAtomInserted {
            parent: at.node_id(),
            text_offset: at.text_offset(),
            atom_index: at.atom_index(),
            inserted: id,
        };
        let inverse = vec![TransactionStep::RemoveInlineAtom { atom: id }];
        Ok((Some(step_map), inverse))
    }

    pub(super) fn apply_remove_inline_atom(
        &mut self,
        atom: NodeId,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let payload = self.store.get(atom).ok_or(Error::UnknownNode)?.clone();
        if !matches!(payload.kind(), NodeKind::InlineAtom(_)) {
            return Err(Error::InvalidTransaction);
        }

        let (parent, placement_index, placement, atom_index) =
            self.find_inline_atom_parent(atom)?;
        let content = self.inline_content(parent)?;
        let mut placements = content.atoms().to_vec();
        placements.remove(placement_index);
        let next = InlineContent::with_atoms(content.runs().iter().cloned(), placements)?;
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            crate::document::NodeContent::Inline(next),
        )?;
        self.store = self.store.without_nodes(&BTreeSet::from([atom]));

        let step_map = StepMap::InlineAtomRemoved {
            parent,
            text_offset: placement.text_offset(),
            atom_index,
            removed: atom,
        };
        let inverse = vec![TransactionStep::RestoreInlineAtom {
            at: InlinePoint::new(
                parent,
                placement.text_offset(),
                atom_index,
                CursorAffinity::Before,
            ),
            node: payload,
        }];
        Ok((Some(step_map), inverse))
    }

    pub(super) fn apply_restore_inline_atom(
        &mut self,
        at: InlinePoint,
        node: &Node,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        if self.store.contains(node.id()) || !matches!(node.kind(), NodeKind::InlineAtom(_)) {
            return Err(Error::InvalidTransaction);
        }
        if node.content().as_inline_atom().is_none() {
            return Err(Error::InvalidTransaction);
        }
        self.validate_inline_gap(at)?;

        let ceiling = node
            .id()
            .raw()
            .checked_add(1)
            .ok_or(Error::NodeIdExhausted)?;
        self.next_node_id = self.next_node_id.max(ceiling);
        self.store = self.store.inserted(node.clone())?;
        self.attach_inline_atom(at, node.id())?;

        let step_map = StepMap::InlineAtomInserted {
            parent: at.node_id(),
            text_offset: at.text_offset(),
            atom_index: at.atom_index(),
            inserted: node.id(),
        };
        let inverse = vec![TransactionStep::RemoveInlineAtom { atom: node.id() }];
        Ok((Some(step_map), inverse))
    }

    fn validate_inline_gap(&self, at: InlinePoint) -> Result<()> {
        let content = self.inline_content(at.node_id())?;
        content.validate_offset(at.text_offset())?;
        if at.atom_index() > content.atom_count_at(at.text_offset()) {
            return Err(Error::InvalidSelection);
        }
        Ok(())
    }

    fn attach_inline_atom(&mut self, at: InlinePoint, atom: NodeId) -> Result<()> {
        let content = self.inline_content(at.node_id())?;
        let mut placements = content.atoms().to_vec();
        let first_at_or_after = placements
            .iter()
            .position(|placement| placement.text_offset() >= at.text_offset())
            .unwrap_or(placements.len());
        let insert_at = first_at_or_after + at.atom_index();
        placements.insert(insert_at, InlineAtomPlacement::new(atom, at.text_offset()));
        let next = InlineContent::with_atoms(content.runs().iter().cloned(), placements)?;
        self.rewrite_node(
            at.node_id(),
            self.attrs_of(at.node_id())?,
            crate::document::NodeContent::Inline(next),
        )
    }

    fn find_inline_atom_parent(
        &self,
        atom: NodeId,
    ) -> Result<(NodeId, usize, InlineAtomPlacement, usize)> {
        for parent in self.store.iter() {
            let Some(inline) = parent.content().as_inline() else {
                continue;
            };
            let Some((placement_index, placement)) = inline
                .atoms()
                .iter()
                .copied()
                .enumerate()
                .find(|(_, placement)| placement.atom() == atom)
            else {
                continue;
            };
            let atom_index = inline.atoms()[..placement_index]
                .iter()
                .filter(|candidate| candidate.text_offset() == placement.text_offset())
                .count();
            return Ok((parent.id(), placement_index, placement, atom_index));
        }
        Err(Error::UnknownNode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        AtomKind, MarkSet, NodeContent, NodeStoreBuilder, TextRun, XiaomuDocument,
    };
    use crate::mapping::{MapBias, MappedPosition};
    use crate::transaction::{Transaction, TransactionOrigin};

    fn fixture() -> (XiaomuDocument, NodeId) {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("ab", MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        (
            XiaomuDocument::new(root, builder.finish()).unwrap(),
            paragraph,
        )
    }

    fn at(document: &XiaomuDocument, paragraph: NodeId, atom_index: usize) -> InlinePoint {
        let offset = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .offset_at(1)
            .unwrap();
        InlinePoint::new(paragraph, offset, atom_index, CursorAffinity::Before)
    }

    fn insert_atom(
        document: &XiaomuDocument,
        paragraph: NodeId,
        atom_index: usize,
        kind: &str,
        fallback: &str,
    ) -> crate::transaction::AppliedTransaction {
        Transaction::new(TransactionOrigin::Extension("atom-test".into()))
            .with_step(TransactionStep::InsertInlineAtom {
                at: at(document, paragraph, atom_index),
                kind: AtomKind::new(kind).unwrap(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::new(fallback).unwrap(),
            })
            .apply_with_changes(document)
            .unwrap()
    }

    #[test]
    fn insert_allocates_atom_and_maps_exact_gap_by_bias() {
        let (document, paragraph) = fixture();
        let old_gap = at(&document, paragraph, 0);
        let applied = insert_atom(&document, paragraph, 0, "mention", "@A");
        let next = applied.document();
        let inline = next.node(paragraph).unwrap().content().as_inline().unwrap();
        assert_eq!(inline.atoms().len(), 1);
        let atom = inline.atoms()[0].atom();
        assert_eq!(next.parent_of(atom), Some(paragraph));
        assert!(matches!(
            next.node(atom).unwrap().kind(),
            NodeKind::InlineAtom(_)
        ));
        assert_eq!(
            next.node(atom)
                .unwrap()
                .content()
                .as_inline_atom()
                .unwrap()
                .fallback_text(),
            "@A"
        );

        assert_eq!(
            applied.changes().map_inline_point(old_gap, MapBias::Start),
            MappedPosition::Mapped(old_gap)
        );
        assert_eq!(
            applied.changes().map_inline_point(old_gap, MapBias::End),
            MappedPosition::Mapped(at(next, paragraph, 1))
        );

        let undone = applied.inverse().apply(next).unwrap();
        assert_eq!(undone.store(), document.store());
        assert_eq!(undone.root(), document.root());
    }

    #[test]
    fn adjacent_atoms_keep_canonical_order_and_remove_inverse_restores_identity() {
        let (document, paragraph) = fixture();
        let first_applied = insert_atom(&document, paragraph, 0, "mention", "@A");
        let after_first = first_applied.document().clone();
        let first = after_first
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()[0]
            .atom();

        let second_applied = insert_atom(&after_first, paragraph, 1, "reference", "ref");
        let after_second = second_applied.document().clone();
        let placements = after_second
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms();
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].atom(), first);
        let second = placements[1].atom();
        assert_ne!(first, second);

        let removed = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveInlineAtom { atom: first })
            .apply_with_changes(&after_second)
            .unwrap();
        let after_remove = removed.document();
        let remaining = after_remove
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].atom(), second);
        assert!(after_remove.node(first).is_none());

        let restored = removed.inverse().apply(after_remove).unwrap();
        let restored_atoms = restored
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms();
        assert_eq!(restored_atoms.len(), 2);
        assert_eq!(restored_atoms[0].atom(), first);
        assert_eq!(restored_atoms[1].atom(), second);
        assert_eq!(restored.store(), after_second.store());
    }

    #[test]
    fn invalid_atom_ordinal_rejects_transaction_atomically() {
        let (document, paragraph) = fixture();
        let transaction = Transaction::new(TransactionOrigin::UserInput).with_step(
            TransactionStep::InsertInlineAtom {
                at: at(&document, paragraph, 1),
                kind: AtomKind::new("mention").unwrap(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::new("@A").unwrap(),
            },
        );
        assert!(matches!(
            transaction.apply(&document),
            Err(Error::InvalidSelection)
        ));
        assert_eq!(document.node_count(), 2);
    }

    #[test]
    fn removing_structural_parent_removes_and_restores_inline_atom_subtree() {
        let (document, paragraph) = fixture();
        let inserted = insert_atom(&document, paragraph, 0, "mention", "@A");
        let with_atom = inserted.document().clone();
        assert_eq!(with_atom.node_count(), 3);

        let removed = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::RemoveNode { node: paragraph })
            .apply_with_changes(&with_atom)
            .unwrap();
        assert_eq!(removed.document().node_count(), 1);

        let restored = removed.inverse().apply(removed.document()).unwrap();
        assert_eq!(restored.store(), with_atom.store());
    }
}
