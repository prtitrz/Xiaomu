//! Borrowed preflight before opted-in projection, cloning or string expansion.

use xiaomu_core::document::{
    AttrValue, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, StringAttribute, TextRun,
    XiaomuDocument,
};

use super::{ClipboardNode, ClipboardNodeContent};

pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 10_000;
const MAX_VALUES: usize = 100_000;
const MAX_DEPTH: usize = 32;

#[derive(Default)]
struct Budget {
    nodes: usize,
    values: usize,
    bytes: usize,
}

impl Budget {
    fn add(&mut self, values: usize, bytes: usize) -> Result<(), ()> {
        self.values = self.values.checked_add(values).ok_or(())?;
        self.bytes = self.bytes.checked_add(bytes).ok_or(())?;
        if self.values > MAX_VALUES || self.bytes > MAX_BYTES {
            return Err(());
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), ()> {
        // JSON escaping expands a UTF-8 input byte by at most six bytes.
        self.add(
            1,
            text.len()
                .checked_mul(6)
                .and_then(|n| n.checked_add(8))
                .ok_or(())?,
        )
    }

    fn node(&mut self, depth: usize) -> Result<(), ()> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(());
        }
        self.nodes += 1;
        // Conservative envelope/DTO framing, and one possible text separator.
        self.add(16, 512)
    }

    fn attrs(&mut self, attrs: &NodeAttrs, depth: usize) -> Result<(), ()> {
        for (key, value) in attrs.iter() {
            self.text(key)?;
            self.attr(value, depth)?;
        }
        Ok(())
    }

    fn attr(&mut self, value: &AttrValue, depth: usize) -> Result<(), ()> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        self.add(4, 64)?;
        match value {
            AttrValue::String(text) => self.text(text),
            AttrValue::List(values) => {
                for value in values {
                    self.attr(value, depth + 1)?;
                }
                Ok(())
            }
            AttrValue::Object(values) => {
                for (key, value) in values {
                    self.text(key)?;
                    self.attr(value, depth + 1)?;
                }
                Ok(())
            }
            AttrValue::Null | AttrValue::Bool(_) | AttrValue::Integer(_) => Ok(()),
            _ => Err(()),
        }
    }

    fn string_attr(&mut self, attr: &StringAttribute) -> Result<(), ()> {
        self.add(3, 32)?;
        if let Some(text) = attr.as_str() {
            self.text(text)?;
        }
        Ok(())
    }

    fn marks(&mut self, marks: &MarkSet) -> Result<(), ()> {
        for mark in marks.as_slice() {
            self.add(3, 64)?;
            match mark {
                Mark::Link(link) => {
                    let attrs = link.attributes();
                    for attr in [
                        attrs.href(),
                        attrs.target(),
                        attrs.rel(),
                        attrs.class(),
                        attrs.title(),
                    ] {
                        self.string_attr(attr)?;
                    }
                }
                Mark::TextStyle(style) => {
                    let attrs = style.attributes();
                    for attr in [attrs.color(), attrs.font_family(), attrs.font_size()] {
                        self.string_attr(attr)?;
                    }
                }
                Mark::Bold | Mark::Italic | Mark::Code | Mark::Underline | Mark::Strike => {}
                _ => return Err(()),
            }
        }
        Ok(())
    }

    fn runs(&mut self, runs: &[TextRun]) -> Result<(), ()> {
        for run in runs {
            self.add(4, 64)?;
            self.text(run.text().as_str())?;
            self.marks(run.marks())?;
        }
        Ok(())
    }

    fn kind(&mut self, kind: &NodeKind) -> Result<(), ()> {
        match kind {
            NodeKind::Custom(key) => self.text(key),
            NodeKind::InlineAtom(kind) => self.text(kind.as_str()),
            _ => Ok(()),
        }
    }

    fn document_node(
        &mut self,
        document: &XiaomuDocument,
        id: NodeId,
        depth: usize,
    ) -> Result<(), ()> {
        self.node(depth)?;
        let node = document.node(id).ok_or(())?;
        self.kind(node.kind())?;
        self.attrs(node.attrs(), depth)?;
        match node.content() {
            NodeContent::Inline(inline) => {
                self.runs(inline.runs())?;
                for atom in inline.atoms() {
                    self.document_node(document, atom.atom(), depth + 1)?;
                }
            }
            NodeContent::Children(children) => {
                for child in children {
                    self.document_node(document, *child, depth + 1)?;
                }
            }
            NodeContent::InlineAtom(atom) => {
                self.text(atom.fallback_text())?;
                self.marks(atom.marks())?;
            }
            NodeContent::Atomic => {}
            _ => return Err(()),
        }
        Ok(())
    }

    fn fragment(&mut self, node: &ClipboardNode, depth: usize) -> Result<(), ()> {
        self.node(depth)?;
        self.kind(node.kind())?;
        self.attrs(node.attrs(), depth)?;
        match node.content() {
            ClipboardNodeContent::Inline(inline) => {
                self.runs(inline.runs())?;
                for atom in inline.atoms() {
                    self.node(depth + 1)?;
                    self.text(atom.kind().as_str())?;
                    self.attrs(atom.attrs(), depth + 1)?;
                    self.text(atom.content().fallback_text())?;
                    self.marks(atom.content().marks())?;
                }
            }
            ClipboardNodeContent::Children(children) => {
                for child in children {
                    self.fragment(child, depth + 1)?;
                }
            }
            ClipboardNodeContent::Table { rows, row_attrs } => {
                for (index, row) in rows.iter().enumerate() {
                    self.node(depth + 1)?;
                    if let Some(attrs) = row_attrs.get(index) {
                        self.attrs(attrs, depth + 1)?;
                    }
                    for cell in row {
                        self.fragment(cell, depth + 2)?;
                    }
                }
            }
            ClipboardNodeContent::Atomic => {}
        }
        Ok(())
    }
}

/// The bounded read-only walk also bounds the existing cross-block selector's
/// temporary index. Unselected content may make an opted-in export exceed this
/// conservative limit; no partial/truncated projection is produced.
pub(super) fn document(document: &XiaomuDocument) -> Result<(), ()> {
    Budget::default().document_node(document, document.root(), 0)
}

pub(super) fn roots(roots: &[ClipboardNode]) -> Result<(), ()> {
    let mut budget = Budget::default();
    budget.add(32, 1024)?;
    for node in roots {
        budget.fragment(node, 1)?;
    }
    Ok(())
}
