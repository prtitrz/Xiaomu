//! Transaction application engine.
//!
//! Steps are applied in order against intermediate stores. Full-tree
//! validation runs once on the final state; intermediate states are kept
//! internally and never escape the engine. While applying, the engine also
//! records the inverse steps of every step; see [`super::inverse`].

use std::collections::{BTreeSet, VecDeque};

use crate::document::{
    InlineContent, Node, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStore, XiaomuDocument,
};
use crate::mapping::{ChangeMap, StepMap};
use crate::text::TextRange;
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
    /// Attribute and mark steps never move positions and return no mapping
    /// data. Every step kind produces inverse steps so whole transactions
    /// stay exactly invertible.
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
            TransactionStep::AddMark { node, range, mark } => {
                self.apply_mark_change(*node, *range, MarkChange::Add(mark.clone()))
            }
            TransactionStep::RemoveMark {
                node,
                range,
                mark_kind,
            } => self.apply_mark_change(*node, *range, MarkChange::Remove(*mark_kind)),
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
            if let Ok(children) = self.children(current) {
                for child in children {
                    if collected.insert(child) {
                        queue.push_back(child);
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
