//! Identity-preserving mixed-inline split/join and their structural inverses.

use std::collections::BTreeSet;

use crate::document::{InlineAtomPlacement, InlineContent, Node, NodeContent, NodeId, TextRun};
use crate::mapping::StepMap;
use crate::selection::{CursorAffinity, InlinePoint};
use crate::text::TextOffset;
use crate::{Error, Result};

use super::ApplyContext;
use crate::transaction::TransactionStep;

impl ApplyContext {
    pub(super) fn apply_split_node(
        &mut self,
        node: NodeId,
        at: TextOffset,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let content = self.inline_content(node)?;
        content.validate_offset(at)?;
        // A byte offset cannot choose a gap among same-boundary atoms.
        // Preserve the existing text-only contract even away from a seam.
        if !content.atoms().is_empty() {
            return Err(Error::InvalidTransaction);
        }
        self.split_inline_node(
            InlinePoint::new(node, at, 0, CursorAffinity::Before),
            None,
            false,
        )
    }

    pub(super) fn apply_split_inline_node(
        &mut self,
        at: InlinePoint,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        self.split_inline_node(at, None, true)
    }

    pub(super) fn apply_restore_joined_node(
        &mut self,
        at: InlinePoint,
        node: &Node,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        if self.store.contains(node.id()) || node.content().as_inline().is_none() {
            return Err(Error::InvalidTransaction);
        }
        let mixed = !self.inline_content(at.node_id())?.atoms().is_empty();
        self.split_inline_node(at, Some(node), mixed)
    }

    fn split_inline_node(
        &mut self,
        at: InlinePoint,
        restored: Option<&Node>,
        mixed_map: bool,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let node = at.node_id();
        let (head, tail) = split_content(&self.inline_content(node)?, at)?;
        let parent = self.find_parent(node)?;
        let mut children = self.children(parent)?;
        let index = children
            .iter()
            .position(|child| *child == node)
            .ok_or(Error::InvalidTransaction)?
            + 1;
        let attrs = self.attrs_of(node)?;
        let tail_id = if let Some(restored) = restored {
            // The saved payload may restore metadata, but may not replace
            // the live suffix or invent additional atom references.
            if restored.content().as_inline() != Some(&tail) {
                return Err(Error::InvalidTransaction);
            }
            let ceiling = restored
                .id()
                .raw()
                .checked_add(1)
                .ok_or(Error::NodeIdExhausted)?;
            self.next_node_id = self.next_node_id.max(ceiling);
            self.store.insert_node_mut(restored.clone())?;
            restored.id()
        } else {
            let kind = self
                .store
                .get(node)
                .ok_or(Error::UnknownNode)?
                .kind()
                .clone();
            self.allocate_node(kind, attrs.clone(), NodeContent::Inline(tail))?
        };
        self.rewrite_node(node, attrs, NodeContent::Inline(head))?;
        children.insert(index, tail_id);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;

        let map = if mixed_map {
            StepMap::InlineNodeSplit {
                parent,
                index,
                at,
                inserted: tail_id,
            }
        } else {
            StepMap::NodeSplit {
                parent,
                index,
                node,
                at: at.text_offset(),
                inserted: tail_id,
            }
        };
        Ok((
            vec![map],
            vec![TransactionStep::JoinNodes {
                first: node,
                second: tail_id,
            }],
        ))
    }

    pub(super) fn apply_join_nodes(
        &mut self,
        first: NodeId,
        second: NodeId,
    ) -> Result<(Vec<StepMap>, Vec<TransactionStep>)> {
        let first_content = self.inline_content(first)?;
        let second_content = self.inline_content(second)?;
        if first == second {
            return Err(Error::InvalidTransaction);
        }
        let parent = self.find_parent(first)?;
        let mut children = self.children(parent)?;
        let first_index = children
            .iter()
            .position(|child| *child == first)
            .ok_or(Error::InvalidTransaction)?;
        if children.get(first_index + 1) != Some(&second) {
            return Err(Error::InvalidTransaction);
        }

        let first_len = first_content.len_bytes();
        let offset = TextOffset::from_validated_byte_index(first_len);
        let seam_atom_index = first_content.atom_count_at(offset);
        let mixed = !first_content.atoms().is_empty() || !second_content.atoms().is_empty();
        let merged_atoms =
            first_content
                .atoms()
                .iter()
                .copied()
                .chain(second_content.atoms().iter().map(|placement| {
                    InlineAtomPlacement::new(
                        placement.atom(),
                        TextOffset::from_validated_byte_index(
                            first_len + placement.text_offset().as_usize(),
                        ),
                    )
                }));
        let merged = InlineContent::with_atoms(
            first_content
                .runs()
                .iter()
                .cloned()
                .chain(second_content.runs().iter().cloned()),
            merged_atoms,
        )?;
        let absorbed = self.store.get(second).ok_or(Error::UnknownNode)?.clone();
        self.rewrite_node(first, self.attrs_of(first)?, NodeContent::Inline(merged))?;
        children.remove(first_index + 1);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;
        // Inline atoms are migrated, never deleted with their old parent.
        let removed = BTreeSet::from([second]);
        self.store.remove_nodes_mut(&removed);

        let map = if mixed {
            StepMap::InlineNodeJoined {
                parent,
                index: first_index + 1,
                first,
                second,
                first_len,
                seam_atom_index,
            }
        } else {
            StepMap::NodeJoined {
                parent,
                index: first_index + 1,
                first,
                second,
                first_len,
                removed,
            }
        };
        let inverse = TransactionStep::RestoreJoinedNode {
            at: InlinePoint::new(first, offset, seam_atom_index, CursorAffinity::Before),
            node: absorbed,
        };
        Ok((vec![map], vec![inverse]))
    }
}

/// Cuts runs and the ordered atom vector at one validated canonical gap.
/// No atom payload is rewritten, copied, deleted, or allocated here.
fn split_content(
    content: &InlineContent,
    at: InlinePoint,
) -> Result<(InlineContent, InlineContent)> {
    content.validate_offset(at.text_offset())?;
    if at.atom_index() > content.atom_count_at(at.text_offset()) {
        return Err(Error::InvalidSelection);
    }
    let split = at.text_offset().as_usize();
    let mut head_runs = Vec::new();
    let mut tail_runs = Vec::new();
    let mut cursor = 0;
    for run in content.runs() {
        let cut = split.saturating_sub(cursor).min(run.len_bytes());
        let text = run.text().as_str();
        if cut > 0 {
            head_runs.push(TextRun::new(&text[..cut], run.marks().clone())?);
        }
        if cut < text.len() {
            tail_runs.push(TextRun::new(&text[cut..], run.marks().clone())?);
        }
        cursor += run.len_bytes();
    }
    let first_at = content
        .atoms()
        .partition_point(|placement| placement.text_offset() < at.text_offset());
    let cut = first_at + at.atom_index();
    let head = InlineContent::with_atoms(head_runs, content.atoms()[..cut].iter().copied())?;
    let tail = InlineContent::with_atoms(
        tail_runs,
        content.atoms()[cut..].iter().map(|placement| {
            InlineAtomPlacement::new(
                placement.atom(),
                TextOffset::from_validated_byte_index(placement.text_offset().as_usize() - split),
            )
        }),
    )?;
    Ok((head, tail))
}
