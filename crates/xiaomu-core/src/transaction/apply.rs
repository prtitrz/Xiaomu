//! Transaction application engine.
//!
//! Steps are applied in order against intermediate stores. Full-tree
//! validation runs once on the final state; intermediate states are kept
//! internally and never escape the engine.

use std::collections::{BTreeSet, VecDeque};

use crate::document::{
    InlineContent, Node, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStore, XiaomuDocument,
};
use crate::mapping::{ChangeMap, StepMap};
use crate::text::TextRange;
use crate::{Error, Result};

use super::inline;
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

    /// Applies one step and returns its mapping data when the step can move
    /// positions. Attribute and mark steps never move positions and return
    /// `None`.
    fn apply_step(&mut self, step: &TransactionStep) -> Result<Option<StepMap>> {
        match step {
            TransactionStep::ReplaceText {
                node,
                range,
                replacement,
            } => self
                .apply_replace_text(*node, *range, replacement)
                .map(Some),
            TransactionStep::InsertNode {
                parent,
                index,
                kind,
                attrs,
                content,
            } => self
                .apply_insert_node(*parent, *index, kind, attrs.clone(), content.clone())
                .map(Some),
            TransactionStep::RemoveNode { node } => self.apply_remove_node(*node).map(Some),
            TransactionStep::SetNodeAttrs { node, attrs } => {
                if self.store.get(*node).is_none() {
                    return Err(Error::UnknownNode);
                }
                self.rewrite_node(*node, attrs.clone(), self.content_of(*node)?)?;
                Ok(None)
            }
            TransactionStep::AddMark { node, range, mark } => {
                self.apply_mark_change(*node, *range, MarkChange::Add(mark.clone()))?;
                Ok(None)
            }
            TransactionStep::RemoveMark {
                node,
                range,
                mark_kind,
            } => {
                self.apply_mark_change(*node, *range, MarkChange::Remove(*mark_kind))?;
                Ok(None)
            }
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
    ) -> Result<StepMap> {
        let content = self.inline_content(node)?;
        let next = inline::replace_text(&content, range, replacement)?;
        self.rewrite_node(node, self.attrs_of(node)?, NodeContent::Inline(next))?;

        Ok(StepMap::TextReplaced {
            node,
            range,
            replacement_len: replacement.len(),
        })
    }

    fn apply_mark_change(
        &mut self,
        node: NodeId,
        range: TextRange,
        change: MarkChange,
    ) -> Result<()> {
        let content = self.inline_content(node)?;
        let next = match change {
            MarkChange::Add(mark) => inline::add_mark(&content, range, mark)?,
            MarkChange::Remove(kind) => inline::remove_mark(&content, range, kind)?,
        };
        self.rewrite_node(node, self.attrs_of(node)?, NodeContent::Inline(next))
    }

    fn attrs_of(&self, id: NodeId) -> Result<NodeAttrs> {
        self.store
            .get(id)
            .ok_or(Error::UnknownNode)
            .map(|node| node.attrs().clone())
    }

    fn apply_insert_node(
        &mut self,
        parent: NodeId,
        index: usize,
        kind: &NodeKind,
        attrs: NodeAttrs,
        content: NodeContent,
    ) -> Result<StepMap> {
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

        Ok(StepMap::NodeInserted {
            parent,
            index,
            inserted: id,
        })
    }

    fn apply_remove_node(&mut self, node: NodeId) -> Result<StepMap> {
        if node == self.root {
            return Err(Error::InvalidTransaction);
        }

        let parent = self.find_parent(node)?;
        let subtree = self.collect_subtree(node);

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

        Ok(StepMap::NodeRemoved {
            parent,
            index,
            removed: subtree,
        })
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

/// Applies `steps` in order and returns a fully validated snapshot together
/// with the composed mapping data of the transaction.
pub(super) fn apply_steps(
    document: &XiaomuDocument,
    steps: &[TransactionStep],
) -> Result<(XiaomuDocument, ChangeMap)> {
    let mut context = ApplyContext {
        root: document.root(),
        store: document.store().clone(),
        next_node_id: document.next_node_id(),
    };

    let mut step_maps = Vec::new();
    for step in steps {
        if let Some(step_map) = context.apply_step(step)? {
            step_maps.push(step_map);
        }
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

    Ok((applied, ChangeMap::from_steps(step_maps)))
}
