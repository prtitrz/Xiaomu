//! Transaction application engine.
//!
//! Steps are applied in order against intermediate stores. Full-tree
//! validation runs once on the final state; intermediate states are kept
//! internally and never escape the engine. While applying, the engine also
//! records the inverse steps of every step; see [`super::inverse`].

mod atom;

use std::collections::{BTreeSet, VecDeque};

use crate::document::{
    InlineContent, Node, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStore, TextRun,
    XiaomuDocument, allows_child,
};
use crate::mapping::{ChangeMap, StepMap};
use crate::text::{TextOffset, TextRange};
use crate::{Error, Result};

use super::inline;
use super::inverse;
use super::step::TransactionStep;

/// Working state of one application pass.
struct ApplyContext {
    root: NodeId,
    store: NodeStore,
    next_node_id: u64,
}

impl ApplyContext {
    fn allocate_node(
        &mut self,
        kind: NodeKind,
        attrs: NodeAttrs,
        content: NodeContent,
    ) -> Result<NodeId> {
        let raw = self.next_node_id;
        let id = NodeId::from_allocated(raw);
        let node = Node::new(id, kind, attrs, content)?;

        self.store = self.store.inserted(node)?;
        self.next_node_id = raw.checked_add(1).ok_or(Error::NodeIdExhausted)?;
        Ok(id)
    }

    fn rewrite_node(&mut self, id: NodeId, attrs: NodeAttrs, content: NodeContent) -> Result<()> {
        let node = self.store.get(id).ok_or(Error::UnknownNode)?;
        let rewritten = Node::new(node.id(), node.kind().clone(), attrs, content)?;
        self.store = self.store.replace_node(rewritten)?;
        Ok(())
    }

    fn inline_content(&self, id: NodeId) -> Result<InlineContent> {
        let node = self.store.get(id).ok_or(Error::UnknownNode)?;
        node.content()
            .as_inline()
            .cloned()
            .ok_or(Error::InvalidTransaction)
    }

    fn children(&self, id: NodeId) -> Result<Vec<NodeId>> {
        let node = self.store.get(id).ok_or(Error::UnknownNode)?;
        node.content()
            .as_children()
            .map(<[NodeId]>::to_vec)
            .ok_or(Error::InvalidTransaction)
    }

    /// Applies one step and returns its mapping data together with the
    /// inverse steps that undo it.
    ///
    /// Attribute, kind, and mark steps never move positions and return no
    /// mapping data. Every step kind produces inverse steps so whole
    /// transactions stay exactly invertible.
    fn apply_step(
        &mut self,
        step: &TransactionStep,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        match step {
            TransactionStep::ReplaceText {
                node,
                range,
                replacement,
            } => self.apply_replace_text(*node, *range, replacement),
            TransactionStep::InsertInlineAtom {
                at,
                kind,
                attrs,
                content,
            } => self.apply_insert_inline_atom(*at, kind, attrs.clone(), content.clone()),
            TransactionStep::RemoveInlineAtom { atom } => self.apply_remove_inline_atom(*atom),
            TransactionStep::RestoreInlineAtom { at, node } => {
                self.apply_restore_inline_atom(*at, node)
            }
            TransactionStep::InsertNode {
                parent,
                index,
                kind,
                attrs,
                content,
            } => self.apply_insert_node(*parent, *index, kind, attrs.clone(), content.clone()),
            TransactionStep::RestoreSubtree {
                parent,
                index,
                root,
                nodes,
            } => self.apply_restore_subtree(*parent, *index, *root, nodes),
            TransactionStep::RemoveNode { node } => self.apply_remove_node(*node),
            TransactionStep::SetNodeAttrs { node, attrs } => {
                self.apply_set_node_attrs(*node, attrs)
            }
            TransactionStep::SetNodeKind { node, kind } => self.apply_set_node_kind(*node, kind),
            TransactionStep::AddMark { node, range, mark } => {
                self.apply_mark_change(*node, *range, MarkChange::Add(mark.clone()))
            }
            TransactionStep::RemoveMark {
                node,
                range,
                mark_kind,
            } => self.apply_mark_change(*node, *range, MarkChange::Remove(*mark_kind)),
            TransactionStep::SplitNode { node, at } => self.apply_split_node(*node, *at),
            TransactionStep::JoinNodes { first, second } => self.apply_join_nodes(*first, *second),
        }
    }

    fn content_of(&self, id: NodeId) -> Result<NodeContent> {
        self.store
            .get(id)
            .ok_or(Error::UnknownNode)
            .map(|node| node.content().clone())
    }

    fn apply_replace_text(
        &mut self,
        node: NodeId,
        range: TextRange,
        replacement: &str,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let content = self.inline_content(node)?;
        let spans = inverse::spans_within(&content, range)?;
        let next = inline::replace_text(&content, range, replacement)?;
        self.rewrite_node(node, self.attrs_of(node)?, NodeContent::Inline(next))?;

        let step_map = StepMap::TextReplaced {
            node,
            range,
            replacement_len: replacement.len(),
        };
        let inverse_steps =
            inverse::replace_text_inverse(node, range, replacement, &content, &spans);
        Ok((Some(step_map), inverse_steps))
    }

    fn apply_mark_change(
        &mut self,
        node: NodeId,
        range: TextRange,
        change: MarkChange,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let content = self.inline_content(node)?;
        let spans = inverse::spans_within(&content, range)?;
        let inverse_steps = match &change {
            MarkChange::Add(mark) => inverse::add_mark_inverse(node, range, mark.kind(), &spans),
            MarkChange::Remove(kind) => inverse::remove_mark_inverse(node, *kind, &spans),
        };

        let next = match change {
            MarkChange::Add(mark) => inline::add_mark(&content, range, mark)?,
            MarkChange::Remove(kind) => inline::remove_mark(&content, range, kind)?,
        };
        self.rewrite_node(node, self.attrs_of(node)?, NodeContent::Inline(next))?;

        Ok((None, inverse_steps))
    }

    fn attrs_of(&self, id: NodeId) -> Result<NodeAttrs> {
        self.store
            .get(id)
            .ok_or(Error::UnknownNode)
            .map(|node| node.attrs().clone())
    }

    fn apply_set_node_attrs(
        &mut self,
        node: NodeId,
        attrs: &NodeAttrs,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        if self.store.get(node).is_none() {
            return Err(Error::UnknownNode);
        }

        let previous = self.attrs_of(node)?;
        self.rewrite_node(node, attrs.clone(), self.content_of(node)?)?;

        let inverse = vec![TransactionStep::SetNodeAttrs {
            node,
            attrs: previous,
        }];
        Ok((None, inverse))
    }

    fn apply_set_node_kind(
        &mut self,
        node: NodeId,
        kind: &NodeKind,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let current = self.store.get(node).ok_or(Error::UnknownNode)?;
        if current.id() == self.root {
            return Err(Error::InvalidRootNode);
        }

        let parent = self.find_parent(node)?;
        let parent_kind = self.store.get(parent).ok_or(Error::UnknownNode)?.kind();
        if !allows_child(parent_kind, kind) {
            return Err(Error::InvalidChildKind);
        }

        let previous = current.kind().clone();
        let rewritten = Node::new(
            current.id(),
            kind.clone(),
            current.attrs().clone(),
            current.content().clone(),
        )?;
        self.store = self.store.replace_node(rewritten)?;

        let inverse = vec![TransactionStep::SetNodeKind {
            node,
            kind: previous,
        }];
        Ok((None, inverse))
    }

    fn apply_insert_node(
        &mut self,
        parent: NodeId,
        index: usize,
        kind: &NodeKind,
        attrs: NodeAttrs,
        content: NodeContent,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let mut children = self.children(parent)?;
        if index > children.len() {
            return Err(Error::InvalidTransaction);
        }

        let id = self.allocate_node(kind.clone(), attrs, content)?;
        children.insert(index, id);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;

        let step_map = StepMap::NodeInserted {
            parent,
            index,
            inserted: id,
        };
        let inverse = vec![TransactionStep::RemoveNode { node: id }];
        Ok((Some(step_map), inverse))
    }

    fn apply_restore_subtree(
        &mut self,
        parent: NodeId,
        index: usize,
        root: NodeId,
        nodes: &[Node],
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        if !nodes.iter().any(|node| node.id() == root) {
            return Err(Error::InvalidTransaction);
        }
        if nodes.iter().any(|node| self.store.contains(node.id())) {
            return Err(Error::InvalidTransaction);
        }

        let mut children = self.children(parent)?;
        if index > children.len() {
            return Err(Error::InvalidTransaction);
        }

        for node in nodes {
            let ceiling = node
                .id()
                .raw()
                .checked_add(1)
                .ok_or(Error::NodeIdExhausted)?;
            self.next_node_id = self.next_node_id.max(ceiling);
            self.store = self.store.inserted(node.clone())?;
        }
        children.insert(index, root);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;

        let step_map = StepMap::NodeInserted {
            parent,
            index,
            inserted: root,
        };
        let inverse = vec![TransactionStep::RemoveNode { node: root }];
        Ok((Some(step_map), inverse))
    }

    fn apply_remove_node(
        &mut self,
        node: NodeId,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        if node == self.root {
            return Err(Error::InvalidTransaction);
        }

        let parent = self.find_parent(node)?;
        let subtree = self.collect_subtree(node);
        let payloads: Vec<Node> = subtree
            .iter()
            .filter_map(|id| self.store.get(*id).cloned())
            .collect();

        let mut children = self.children(parent)?;
        let index = children
            .iter()
            .position(|child| *child == node)
            .ok_or(Error::InvalidTransaction)?;
        children.retain(|child| *child != node);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;
        self.store = self.store.without_nodes(&subtree);

        let step_map = StepMap::NodeRemoved {
            parent,
            index,
            removed: subtree,
        };
        let inverse = vec![TransactionStep::RestoreSubtree {
            parent,
            index,
            root: node,
            nodes: payloads,
        }];
        Ok((Some(step_map), inverse))
    }

    /// Splits an inline-bearing node at `at`; the text from `at` onward
    /// moves into a freshly allocated sibling inserted right after it.
    fn apply_split_node(
        &mut self,
        node: NodeId,
        at: TextOffset,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        let content = self.inline_content(node)?;
        content.validate_offset(at)?;
        if !content.atoms().is_empty() {
            return Err(Error::InvalidTransaction);
        }

        let split_at = at.as_usize();
        let mut head_runs = Vec::new();
        let mut tail_runs = Vec::new();
        let mut cursor = 0usize;
        for run in content.runs() {
            let run_start = cursor;
            let run_end = run_start + run.len_bytes();
            cursor = run_end;

            let marks = run.marks().clone();
            let text = run.text().as_str();
            if run_end <= split_at {
                head_runs.push(TextRun::new(text.to_owned(), marks)?);
            } else if run_start >= split_at {
                tail_runs.push(TextRun::new(text.to_owned(), marks)?);
            } else {
                // Splitting inside a run keeps both halves on that run's
                // marks; both slices are non-empty by construction.
                head_runs.push(TextRun::new(
                    text[..split_at - run_start].to_owned(),
                    marks.clone(),
                )?);
                tail_runs.push(TextRun::new(
                    text[split_at - run_start..].to_owned(),
                    marks,
                )?);
            }
        }

        let parent = self.find_parent(node)?;
        let mut children = self.children(parent)?;
        let index = children
            .iter()
            .position(|child| *child == node)
            .ok_or(Error::InvalidTransaction)?;
        let kind = self
            .store
            .get(node)
            .ok_or(Error::UnknownNode)?
            .kind()
            .clone();
        let attrs = self.attrs_of(node)?;

        let tail_id = self.allocate_node(
            kind,
            attrs.clone(),
            NodeContent::Inline(InlineContent::new(tail_runs)?),
        )?;
        self.rewrite_node(
            node,
            attrs,
            NodeContent::Inline(InlineContent::new(head_runs)?),
        )?;
        children.insert(index + 1, tail_id);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;

        let step_map = StepMap::NodeSplit {
            parent,
            index: index + 1,
            node,
            at,
            inserted: tail_id,
        };
        // Joining the two siblings back re-merges the runs normalization
        // had to cut apart, so the store becomes exactly equal again.
        let inverse = vec![TransactionStep::JoinNodes {
            first: node,
            second: tail_id,
        }];
        Ok((Some(step_map), inverse))
    }

    /// Merges `second` into its immediately preceding sibling `first`.
    fn apply_join_nodes(
        &mut self,
        first: NodeId,
        second: NodeId,
    ) -> Result<(Option<StepMap>, Vec<TransactionStep>)> {
        // Unknown identities surface as `UnknownNode`; structural problems
        // (same node, non-siblings) surface as `InvalidTransaction`.
        let first_content = self.inline_content(first)?;
        let second_content = self.inline_content(second)?;
        if first == second || self.store.get(first).is_none() {
            return Err(Error::InvalidTransaction);
        }
        if !first_content.atoms().is_empty() || !second_content.atoms().is_empty() {
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
        let merged_runs = first_content
            .runs()
            .iter()
            .cloned()
            .chain(second_content.runs().iter().cloned());
        let merged_len = InlineContent::new(merged_runs.clone())?.len_bytes();
        self.rewrite_node(
            first,
            self.attrs_of(first)?,
            NodeContent::Inline(InlineContent::new(merged_runs)?),
        )?;

        let subtree = self.collect_subtree(second);
        let payloads: Vec<Node> = subtree
            .iter()
            .filter_map(|id| self.store.get(*id).cloned())
            .collect();
        children.remove(first_index + 1);
        self.rewrite_node(
            parent,
            self.attrs_of(parent)?,
            NodeContent::children(children),
        )?;
        self.store = self.store.without_nodes(&subtree);

        let step_map = StepMap::NodeJoined {
            parent,
            index: first_index + 1,
            first,
            second,
            first_len,
            removed: subtree,
        };
        let inverse = vec![
            // Deleting the appended span leaves exactly the original runs
            // of `first`, which were normalized before the join.
            TransactionStep::ReplaceText {
                node: first,
                range: TextRange::new(
                    TextOffset::from_validated_byte_index(first_len),
                    TextOffset::from_validated_byte_index(merged_len),
                )?,
                replacement: String::new(),
            },
            TransactionStep::RestoreSubtree {
                parent,
                index: first_index + 1,
                root: second,
                nodes: payloads,
            },
        ];
        Ok((Some(step_map), inverse))
    }

    fn find_parent(&self, target: NodeId) -> Result<NodeId> {
        let mut queue = VecDeque::from([self.root]);

        while let Some(current) = queue.pop_front() {
            let Ok(children) = self.children(current) else {
                continue;
            };
            if children.contains(&target) {
                return Ok(current);
            }
            queue.extend(children);
        }

        Err(Error::UnknownNode)
    }

    fn collect_subtree(&self, target: NodeId) -> BTreeSet<NodeId> {
        let mut collected = BTreeSet::from([target]);
        let mut queue = VecDeque::from([target]);

        while let Some(current) = queue.pop_front() {
            let Some(node) = self.store.get(current) else {
                continue;
            };
            if let Some(children) = node.content().as_children() {
                for child in children {
                    if collected.insert(*child) {
                        queue.push_back(*child);
                    }
                }
            }
            if let Some(inline) = node.content().as_inline() {
                for placement in inline.atoms() {
                    if collected.insert(placement.atom()) {
                        queue.push_back(placement.atom());
                    }
                }
            }
        }

        collected
    }
}

enum MarkChange {
    Add(crate::document::Mark),
    Remove(crate::document::MarkKind),
}

/// Applies `steps` in order and returns a fully validated snapshot, the
/// composed mapping data, and the inverse steps of the whole transaction.
///
/// Inverse step groups are recorded per step and reversed here, so each
/// group's coordinates match the intermediate state its original step
/// produced.
pub(super) fn apply_steps(
    document: &XiaomuDocument,
    steps: &[TransactionStep],
) -> Result<(XiaomuDocument, ChangeMap, Vec<TransactionStep>)> {
    let mut context = ApplyContext {
        root: document.root(),
        store: document.store().clone(),
        next_node_id: document.next_node_id(),
    };

    let mut step_maps = Vec::new();
    let mut inverse_groups: Vec<Vec<TransactionStep>> = Vec::new();
    for step in steps {
        let (step_map, inverse_steps) = context.apply_step(step)?;
        if let Some(step_map) = step_map {
            step_maps.push(step_map);
        }
        inverse_groups.push(inverse_steps);
    }

    let revision = document
        .revision()
        .next()
        .ok_or(Error::InvalidTransaction)?;
    let applied = XiaomuDocument::from_applied_parts(
        document.version(),
        revision,
        document.root(),
        context.store,
        context.next_node_id,
    )?;

    let inverse_steps = inverse_groups.into_iter().rev().flatten().collect();
    Ok((applied, ChangeMap::from_steps(step_maps), inverse_steps))
}
