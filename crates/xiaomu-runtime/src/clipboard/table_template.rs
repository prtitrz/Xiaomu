//! Bounded bridge from one detached table tree to Core's opaque template.

use std::mem::size_of;

use xiaomu_core::document::{
    AttrValue, InlineAtomPlacement, Mark, MarkSet, Node, NodeAttrs, NodeId, NodeKind,
    StringAttribute, TextRun,
};
use xiaomu_core::transaction::TableTreeTemplate;
use xiaomu_core::{Error, Result};

use super::fragment::{ClipboardNode, ClipboardNodeContent, fragment_document};

impl ClipboardNode {
    /// Validates and captures this complete Table tree for fresh-ID insertion.
    ///
    /// Reuses the fragment-to-Core validator, preserving raw attributes, row
    /// wrappers (including covered empty rows), cell kinds, nested subtrees,
    /// text marks and independently marked inline atoms. The immutable result
    /// carries no source IDs; Core allocates every destination identity when
    /// applying [`xiaomu_core::transaction::TransactionStep::InsertTableTree`].
    ///
    /// This is only a tree-cloning bridge. It does not grant open/partial slice
    /// fitting, infer closed-selection provenance, or change `PasteSlice` rules.
    /// The caller remains responsible for choosing an allowed insertion route.
    ///
    /// Before rebuilding or cloning any payload, a borrowed preflight bounds
    /// canonical nodes/values to one million, tree depth to 128 levels, attribute
    /// depth to 64 levels and accounted owned payload to 64 MiB. Those limits
    /// match Core capture's limits; reconstruction and template accounting are
    /// separate checks, not a bound on allocator overhead or all execution memory.
    /// Non-tables, invalid structure and resource excess return Core errors
    /// without modifying the source, a destination, history or the clipboard.
    pub fn table_tree_template(&self) -> Result<TableTreeTemplate> {
        if !matches!(self.kind(), NodeKind::Table) {
            return Err(Error::InvalidTableStructure);
        }
        SourceBudget::default().node(self, 0)?;
        let document = fragment_document(std::slice::from_ref(self))?;
        let table = document
            .node(document.root())
            .and_then(|root| root.content().as_children())
            .and_then(|children| children.first())
            .copied()
            .ok_or(Error::InvalidTableStructure)?;
        TableTreeTemplate::capture(&document, table)
    }
}

// Keep the limits aligned with Core TableTreeTemplate capture. This checks the
// temporary canonical reconstruction's payload; Core independently accounts
// its private template representation before copying that validated tree.
const MAX_NODES: usize = 1_000_000;
const MAX_VALUES: usize = 1_000_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_TREE_DEPTH: usize = 128;
const MAX_ATTR_DEPTH: usize = 64;

#[derive(Default)]
struct SourceBudget {
    nodes: usize,
    values: usize,
    bytes: usize,
}

impl SourceBudget {
    fn node(&mut self, node: &ClipboardNode, depth: usize) -> Result<()> {
        self.node_header(node.kind(), node.attrs(), depth)?;
        match node.content() {
            ClipboardNodeContent::Children(children) => {
                self.children(children, depth + 1)?;
            }
            ClipboardNodeContent::Table { rows, row_attrs } => {
                if !row_attrs.is_empty() && row_attrs.len() != rows.len() {
                    return Err(Error::InvalidTableStructure);
                }
                self.array(rows.len(), size_of::<NodeId>())?;
                for (index, cells) in rows.iter().enumerate() {
                    // The DTO omits row wrappers but reconstruction allocates
                    // them, so count both their attributes and canonical depth.
                    self.node_header(
                        &NodeKind::TableRow,
                        row_attrs.get(index).unwrap_or(&NodeAttrs::empty()),
                        depth + 1,
                    )?;
                    self.children(cells, depth + 2)?;
                }
            }
            ClipboardNodeContent::Inline(inline) => {
                self.array(inline.runs().len(), size_of::<TextRun>())?;
                self.array(inline.atoms().len(), size_of::<InlineAtomPlacement>())?;
                for run in inline.runs() {
                    self.charge(run.len_bytes(), 0)?;
                    self.marks(run.marks())?;
                }
                for atom in inline.atoms() {
                    // Do not manufacture a NodeKind: extension atom kinds own
                    // strings, and preflight must never clone that payload.
                    self.node_size(depth + 1)?;
                    self.charge(atom.kind().as_str().len(), 0)?;
                    self.attrs(atom.attrs())?;
                    self.charge(atom.content().fallback_text().len(), 0)?;
                    self.marks(atom.content().marks())?;
                }
            }
            ClipboardNodeContent::Atomic => {}
        }
        Ok(())
    }

    fn children(&mut self, children: &[ClipboardNode], depth: usize) -> Result<()> {
        // Check the whole width before iterating or creating builder vectors.
        self.array(children.len(), size_of::<NodeId>())?;
        for child in children {
            self.node(child, depth)?;
        }
        Ok(())
    }

    fn node_header(&mut self, kind: &NodeKind, attrs: &NodeAttrs, depth: usize) -> Result<()> {
        self.node_size(depth)?;
        match kind {
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
            _ => return Err(Error::InvalidNodeContent),
        }
        self.attrs(attrs)
    }

    fn node_size(&mut self, depth: usize) -> Result<()> {
        if depth >= MAX_TREE_DEPTH || self.nodes >= MAX_NODES {
            return Err(Error::TableResourceLimit);
        }
        self.nodes += 1;
        self.charge(size_of::<Node>(), 1)
    }

    fn attrs(&mut self, attrs: &NodeAttrs) -> Result<()> {
        for (key, value) in attrs.iter() {
            self.key(key)?;
            self.attr(value, 0)?;
        }
        Ok(())
    }

    fn charge(&mut self, bytes: usize, values: usize) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(Error::TableResourceLimit)?;
        self.values = self
            .values
            .checked_add(values)
            .ok_or(Error::TableResourceLimit)?;
        if self.bytes > MAX_BYTES || self.values > MAX_VALUES {
            return Err(Error::TableResourceLimit);
        }
        Ok(())
    }

    fn array(&mut self, count: usize, size: usize) -> Result<()> {
        self.charge(
            count.checked_mul(size).ok_or(Error::TableResourceLimit)?,
            count,
        )
    }

    fn key(&mut self, key: &str) -> Result<()> {
        self.charge(
            size_of::<String>()
                .checked_add(key.len())
                .ok_or(Error::TableResourceLimit)?,
            0,
        )
    }

    fn attr(&mut self, value: &AttrValue, depth: usize) -> Result<()> {
        if depth >= MAX_ATTR_DEPTH {
            return Err(Error::TableResourceLimit);
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
            _ => return Err(Error::InvalidNodeContent),
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
                _ => return Err(Error::InvalidMarkSet),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{ClipboardAtom, ClipboardInline};
    use xiaomu_core::document::{
        AtomKind, InlineAtomContent, LinkMark, TextStyleAttributes, TextStyleMark,
    };
    use xiaomu_core::text::TextOffset;

    fn paragraph() -> ClipboardNode {
        ClipboardNode::new(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            ClipboardNodeContent::Inline(ClipboardInline::default()),
        )
    }

    fn table(rows: Vec<Vec<ClipboardNode>>, row_attrs: Vec<NodeAttrs>) -> ClipboardNode {
        ClipboardNode::new(
            NodeKind::Table,
            NodeAttrs::empty(),
            ClipboardNodeContent::Table { rows, row_attrs },
        )
    }

    fn cell() -> ClipboardNode {
        ClipboardNode::new(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            ClipboardNodeContent::Children(vec![paragraph()]),
        )
    }

    #[test]
    fn invalid_native_sources_fail_without_changing_their_values() {
        let no_children = ClipboardNode::new(
            NodeKind::TableCell,
            NodeAttrs::empty(),
            ClipboardNodeContent::Children(vec![]),
        );
        for invalid in [
            paragraph(),
            table(vec![], vec![]),
            table(vec![vec![cell()]], vec![NodeAttrs::empty(); 2]),
            table(vec![vec![paragraph()]], vec![]),
            table(vec![vec![no_children]], vec![]),
            table(vec![vec![cell()], vec![]], vec![]),
            ClipboardNode::new(
                NodeKind::Table,
                NodeAttrs::empty(),
                ClipboardNodeContent::Atomic,
            ),
        ] {
            let before = invalid.clone();
            assert!(invalid.table_tree_template().is_err());
            assert_eq!(invalid, before);
        }
    }

    #[test]
    fn canonical_children_and_row_dto_capture_the_same_complete_table() {
        let dto = table(vec![vec![cell()]], vec![]);
        let canonical = ClipboardNode::new(
            NodeKind::Table,
            NodeAttrs::empty(),
            ClipboardNodeContent::Children(vec![ClipboardNode::new(
                NodeKind::TableRow,
                NodeAttrs::empty(),
                ClipboardNodeContent::Children(vec![cell()]),
            )]),
        );
        assert_eq!(
            dto.table_tree_template().unwrap(),
            canonical.table_tree_template().unwrap()
        );
        assert_eq!(dto.table_tree_template().unwrap().node_count(), 4);
    }

    #[test]
    fn native_tree_and_attribute_depth_are_bounded_before_reconstruction() {
        let mut deep = paragraph();
        for _ in 0..MAX_TREE_DEPTH {
            deep = ClipboardNode::new(
                NodeKind::Quote,
                NodeAttrs::empty(),
                ClipboardNodeContent::Children(vec![deep]),
            );
        }
        let deep = table(
            vec![vec![ClipboardNode::new(
                NodeKind::TableCell,
                NodeAttrs::empty(),
                ClipboardNodeContent::Children(vec![deep]),
            )]],
            vec![],
        );
        assert_eq!(
            deep.table_tree_template().unwrap_err(),
            Error::TableResourceLimit
        );

        let mut nested = AttrValue::Null;
        for _ in 0..MAX_ATTR_DEPTH {
            nested = AttrValue::List(vec![nested]);
        }
        let attrs = NodeAttrs::new([("opaque".into(), nested)].into()).unwrap();
        let deep_attrs = table(vec![vec![cell()]], vec![attrs]);
        assert_eq!(
            deep_attrs.table_tree_template().unwrap_err(),
            Error::TableResourceLimit
        );
    }

    #[test]
    fn implicit_rows_and_detached_atoms_count_toward_canonical_depth_and_nodes() {
        let table = table(vec![vec![cell()]], vec![]);
        let mut budget = SourceBudget::default();
        budget.node(&table, 0).unwrap();
        assert_eq!(budget.nodes, 4);
        assert_eq!(
            SourceBudget::default().node(&table, MAX_TREE_DEPTH - 3),
            Err(Error::TableResourceLimit)
        );
        let inline = ClipboardInline::new(
            [],
            [ClipboardAtom::new(
                TextOffset::ZERO,
                AtomKind::new("extension").unwrap(),
                NodeAttrs::empty(),
                InlineAtomContent::new("fallback").unwrap(),
            )],
        )
        .unwrap();
        let node = ClipboardNode::new(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            ClipboardNodeContent::Inline(inline),
        );
        let mut budget = SourceBudget::default();
        budget.node(&node, 0).unwrap();
        assert_eq!(budget.nodes, 2);
        assert_eq!(
            SourceBudget::default().node(&node, MAX_TREE_DEPTH - 1),
            Err(Error::TableResourceLimit)
        );
    }

    #[test]
    fn counts_and_arithmetic_fail_before_builder_allocation() {
        let mut budget = SourceBudget {
            nodes: MAX_NODES,
            ..Default::default()
        };
        assert_eq!(budget.node(&cell(), 0), Err(Error::TableResourceLimit));
        let mut budget = SourceBudget {
            values: MAX_VALUES,
            ..Default::default()
        };
        assert_eq!(
            budget.attr(&AttrValue::Null, 0),
            Err(Error::TableResourceLimit)
        );
        assert_eq!(
            SourceBudget::default().array(MAX_VALUES + 1, 0),
            Err(Error::TableResourceLimit)
        );
        assert_eq!(
            SourceBudget::default().array(usize::MAX, 2),
            Err(Error::TableResourceLimit)
        );
        let mut budget = SourceBudget {
            bytes: usize::MAX,
            ..Default::default()
        };
        assert_eq!(budget.charge(1, 0), Err(Error::TableResourceLimit));
    }

    #[test]
    fn owned_text_attrs_keys_and_all_link_fields_are_accounted_without_cloning() {
        let payload =
            AttrValue::Object([("owned-key".into(), AttrValue::String("payload".into()))].into());
        let mut budget = SourceBudget::default();
        budget.attr(&payload, 0).unwrap();
        assert_eq!(budget.values, 2);
        assert_eq!(
            budget.bytes,
            2 * size_of::<AttrValue>() + size_of::<String>() + 9 + 7
        );
        let marks = MarkSet::new([Mark::Link(LinkMark::new(
            "https://example.test",
            Some("title".into()),
        ))])
        .unwrap();
        let mut budget = SourceBudget::default();
        budget.marks(&marks).unwrap();
        assert_eq!(budget.values, 6); // One mark and all five string fields.
        assert_eq!(budget.bytes, size_of::<Mark>() + 20 + 5);
        let style = MarkSet::new([Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(StringAttribute::Value("red".into()))
                .with_font_family(StringAttribute::Null)
                .with_font_size(StringAttribute::Value("12px".into())),
        ))])
        .unwrap();
        let mut budget = SourceBudget::default();
        budget.marks(&style).unwrap();
        assert_eq!(budget.values, 4); // One mark and all three string fields.
        assert_eq!(budget.bytes, size_of::<Mark>() + 3 + 4);
        let node = ClipboardNode::new(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            ClipboardNodeContent::Inline(
                ClipboardInline::text_only([TextRun::new("payload", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        );
        let mut budget = SourceBudget {
            bytes: MAX_BYTES - size_of::<Node>() - size_of::<TextRun>() - 6,
            ..Default::default()
        };
        assert_eq!(budget.node(&node, 0), Err(Error::TableResourceLimit));
    }
}
