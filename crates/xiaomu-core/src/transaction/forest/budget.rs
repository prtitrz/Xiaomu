//! Read-only bounded accounting before any template payload clone.

use std::mem::size_of;

use crate::document::{
    AttrValue, Mark, MarkSet, Node, NodeContent, NodeKind, StringAttribute, TextRun,
};
use crate::{Error, Result};

use super::TemplateNode;

const MAX_NODES: usize = 1_000_000;
const MAX_VALUES: usize = 1_000_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 128;
const MAX_ATTR_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::transaction) struct CaptureBudget {
    nodes: usize,
    values: usize,
    bytes: usize,
    limit: Error,
}

impl Default for CaptureBudget {
    fn default() -> Self {
        Self {
            nodes: 0,
            values: 0,
            bytes: 0,
            limit: Error::TableResourceLimit,
        }
    }
}

impl CaptureBudget {
    pub(in crate::transaction) fn document() -> Self {
        Self {
            limit: Error::SnapshotResourceLimit,
            ..Self::default()
        }
    }

    pub(in crate::transaction) fn limit(&self) -> Error {
        self.limit.clone()
    }

    pub(in crate::transaction) fn absorb(&mut self, other: &Self) -> Result<()> {
        self.nodes = self
            .nodes
            .checked_add(other.nodes)
            .ok_or_else(|| self.limit())?;
        if self.nodes > MAX_NODES {
            return Err(self.limit());
        }
        self.charge(other.bytes, other.values)
    }

    pub(in crate::transaction) fn maps(&mut self, roots: usize, nodes: usize) -> Result<()> {
        self.array(roots, size_of::<crate::mapping::StepMap>())?;
        self.array(nodes, size_of::<crate::document::NodeId>())
    }

    pub(in crate::transaction) fn operation(&mut self, bytes: usize) -> Result<()> {
        self.charge(bytes, 1)
    }

    pub(in crate::transaction) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(in crate::transaction) fn pending(
        &self,
        done: usize,
        pending: usize,
        added: usize,
    ) -> Result<()> {
        let count = done
            .checked_add(pending)
            .and_then(|n| n.checked_add(added))
            .ok_or_else(|| self.limit())?;
        if count > MAX_NODES {
            return Err(self.limit());
        }
        Ok(())
    }

    pub(in crate::transaction) fn node(&mut self, node: &Node, depth: usize) -> Result<()> {
        if depth >= MAX_TREE_DEPTH || self.nodes >= MAX_NODES {
            return Err(self.limit());
        }
        self.nodes += 1;
        self.charge(size_of::<TemplateNode>().max(size_of::<Node>()), 1)?;
        match node.kind() {
            NodeKind::Custom(key) => self.charge(key.len(), 0)?,
            NodeKind::InlineAtom(kind) => self.charge(kind.as_str().len(), 0)?,
            NodeKind::Document
            | NodeKind::Paragraph
            | NodeKind::Heading(_)
            | NodeKind::Quote
            | NodeKind::BulletList
            | NodeKind::OrderedList
            | NodeKind::ListItem
            | NodeKind::TaskList
            | NodeKind::TaskItem
            | NodeKind::CodeBlock
            | NodeKind::HorizontalRule
            | NodeKind::Image
            | NodeKind::Table
            | NodeKind::TableRow
            | NodeKind::TableCell
            | NodeKind::TableHeader => {}
        }
        for (key, value) in node.attrs().iter() {
            self.key(key)?;
            self.attr(value, 0)?;
        }
        match node.content() {
            NodeContent::Children(children) => self.array(children.len(), size_of::<usize>())?,
            NodeContent::Inline(inline) => {
                self.array(inline.runs().len(), size_of::<TextRun>())?;
                self.array(
                    inline.atoms().len(),
                    size_of::<(usize, crate::text::TextOffset)>(),
                )?;
                for run in inline.runs() {
                    self.charge(run.len_bytes(), 0)?;
                    self.marks(run.marks())?;
                }
            }
            NodeContent::InlineAtom(atom) => {
                self.charge(atom.fallback_text().len(), 0)?;
                self.marks(atom.marks())?;
            }
            NodeContent::Atomic => {}
        }
        Ok(())
    }

    fn charge(&mut self, bytes: usize, values: usize) -> Result<()> {
        self.bytes = self.bytes.checked_add(bytes).ok_or_else(|| self.limit())?;
        self.values = self
            .values
            .checked_add(values)
            .ok_or_else(|| self.limit())?;
        if self.bytes > MAX_BYTES || self.values > MAX_VALUES {
            return Err(self.limit());
        }
        Ok(())
    }

    fn array(&mut self, count: usize, size: usize) -> Result<()> {
        self.charge(count.checked_mul(size).ok_or_else(|| self.limit())?, count)
    }

    fn key(&mut self, key: &str) -> Result<()> {
        self.charge(
            size_of::<String>()
                .checked_add(key.len())
                .ok_or_else(|| self.limit())?,
            0,
        )
    }

    fn attr(&mut self, value: &AttrValue, depth: usize) -> Result<()> {
        if depth >= MAX_ATTR_DEPTH {
            return Err(self.limit());
        }
        self.charge(size_of::<AttrValue>(), 1)?;
        match value {
            AttrValue::String(text) => self.charge(text.len(), 0)?,
            AttrValue::List(values) => {
                for value in values {
                    self.attr(value, depth + 1)?;
                }
            }
            AttrValue::Object(values) => {
                for (key, value) in values {
                    self.key(key)?;
                    self.attr(value, depth + 1)?;
                }
            }
            AttrValue::Null | AttrValue::Bool(_) | AttrValue::Integer(_) => {}
        }
        Ok(())
    }

    fn string_attr(&mut self, value: &StringAttribute) -> Result<()> {
        self.charge(value.as_str().map_or(0, str::len), 1)
    }

    fn marks(&mut self, marks: &MarkSet) -> Result<()> {
        self.array(marks.len(), size_of::<Mark>())?;
        for mark in marks.as_slice() {
            match mark {
                Mark::Link(link) => {
                    let attrs = link.attributes();
                    for value in [
                        attrs.href(),
                        attrs.target(),
                        attrs.rel(),
                        attrs.class(),
                        attrs.title(),
                    ] {
                        self.string_attr(value)?;
                    }
                }
                Mark::TextStyle(style) => {
                    let attrs = style.attributes();
                    for value in [attrs.color(), attrs.font_family(), attrs.font_size()] {
                        self.string_attr(value)?;
                    }
                }
                Mark::Bold | Mark::Italic | Mark::Code | Mark::Underline | Mark::Strike => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LinkMark, NodeAttrs, NodeId};
    use std::collections::BTreeMap;

    #[test]
    fn owned_attrs_text_marks_and_atom_fallback_are_charged_before_cloning() {
        let node = Node::new(
            NodeId::from_allocated(1),
            NodeKind::Paragraph,
            NodeAttrs::new(BTreeMap::from([(
                "opaque".into(),
                AttrValue::String("x".repeat(1024)),
            )]))
            .unwrap(),
            NodeContent::empty_inline(),
        )
        .unwrap();
        let mut budget = CaptureBudget {
            bytes: MAX_BYTES - 1024,
            ..Default::default()
        };
        assert_eq!(budget.node(&node, 0), Err(Error::TableResourceLimit));
        let marks = MarkSet::new([Mark::Link(LinkMark::new(
            "https://example.com",
            Some("title".into()),
        ))])
        .unwrap();
        let mut budget = CaptureBudget::default();
        budget.marks(&marks).unwrap();
        assert!(budget.bytes >= size_of::<Mark>() + 19 + 5);
        let mut budget = CaptureBudget {
            bytes: usize::MAX,
            ..Default::default()
        };
        assert_eq!(budget.charge(1, 0), Err(Error::TableResourceLimit));
    }

    #[test]
    fn attribute_depth_value_count_and_pending_count_are_independently_bounded() {
        let mut nested = AttrValue::Null;
        for _ in 0..MAX_ATTR_DEPTH {
            nested = AttrValue::List(vec![nested]);
        }
        assert_eq!(
            CaptureBudget::default().attr(&nested, 0),
            Err(Error::TableResourceLimit)
        );
        let mut budget = CaptureBudget {
            values: MAX_VALUES,
            ..Default::default()
        };
        assert_eq!(
            budget.attr(&AttrValue::Null, 0),
            Err(Error::TableResourceLimit)
        );
        assert_eq!(
            CaptureBudget::default().pending(usize::MAX, 1, 1),
            Err(Error::TableResourceLimit)
        );
        assert_eq!(
            CaptureBudget::default().pending(0, 0, MAX_NODES + 1),
            Err(Error::TableResourceLimit)
        );
    }

    #[test]
    fn document_combined_payload_node_value_and_mapping_limits_are_checked() {
        let mut budget = CaptureBudget {
            bytes: MAX_BYTES - 1,
            ..CaptureBudget::document()
        };
        let extra = CaptureBudget {
            bytes: 2,
            ..CaptureBudget::document()
        };
        assert_eq!(budget.absorb(&extra), Err(Error::SnapshotResourceLimit));
        let mut budget = CaptureBudget {
            nodes: MAX_NODES,
            ..CaptureBudget::document()
        };
        let extra = CaptureBudget {
            nodes: 1,
            ..CaptureBudget::document()
        };
        assert_eq!(budget.absorb(&extra), Err(Error::SnapshotResourceLimit));
        let mut budget = CaptureBudget {
            values: MAX_VALUES,
            ..CaptureBudget::document()
        };
        assert_eq!(budget.maps(1, 0), Err(Error::SnapshotResourceLimit));
        let mut budget = CaptureBudget {
            bytes: MAX_BYTES - 1,
            ..CaptureBudget::document()
        };
        assert_eq!(budget.maps(0, 1), Err(Error::SnapshotResourceLimit));
        assert_eq!(
            CaptureBudget::document().maps(usize::MAX, 0),
            Err(Error::SnapshotResourceLimit)
        );
        let mut budget = CaptureBudget {
            nodes: usize::MAX,
            ..CaptureBudget::document()
        };
        assert_eq!(budget.absorb(&extra), Err(Error::SnapshotResourceLimit));
    }
}
