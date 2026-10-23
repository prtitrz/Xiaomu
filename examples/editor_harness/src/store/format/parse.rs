//! Strict, version-aware reader for harness fixtures.
use super::super::marks_text::parse_marks;
use super::{parse_attrs, unescape_text};
use std::{iter::Peekable, str::Lines};
use xiaomu_core::document::{
    AtomKind, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::text::TextBuffer;

/// One parsed inline field: text segments in order plus atom placements as
/// `(token index, raw byte offset)` pairs awaiting atom node resolution.
struct PendingInline {
    segments: Vec<(String, MarkSet)>,
    placements: Vec<(usize, usize)>,
}

/// Parses an inline field. Text travels as text/marks pair items; `{a#N}`
/// items are atom placement tokens (v3). Plain v2 fields carry no tokens and
/// parse identically.
fn parse_inline_pending(rest: &str) -> Result<PendingInline, String> {
    let mut pending = PendingInline {
        segments: Vec::new(),
        placements: Vec::new(),
    };
    if rest.is_empty() {
        return Ok(pending);
    }
    let fields: Vec<&str> = rest.split('\t').collect();
    let mut index = 0usize;
    let mut offset = 0usize;
    while index < fields.len() {
        if let Some(token) = fields[index]
            .strip_prefix("{a#")
            .and_then(|rest| rest.strip_suffix('}'))
        {
            let atom_index: usize = token.parse().map_err(|_| "bad atom token".to_owned())?;
            pending.placements.push((atom_index, offset));
            index += 1;
            continue;
        }
        let text = unescape_text(fields[index]);
        let marks = fields
            .get(index + 1)
            .ok_or("inline runs must be text/marks pairs")?;
        let marks = parse_marks(marks)?;
        offset += text.len();
        if !(text.is_empty() && marks.is_empty()) {
            pending.segments.push((text, marks));
        }
        index += 2;
    }
    Ok(pending)
}

pub fn parse_document(text: &str) -> Result<XiaomuDocument, String> {
    let mut lines = text.lines();
    let tables_allowed = match lines.next() {
        Some("xiaomu-fixture-doc v5") => true,
        Some("xiaomu-fixture-doc v2" | "xiaomu-fixture-doc v3" | "xiaomu-fixture-doc v4") => false,
        _ => return Err("unknown fixture header".to_owned()),
    };

    enum Frame {
        Quote,
        BulletList,
        OrderedList,
        ListItem,
        Table { rows: usize, columns: usize },
        TableRow { columns: usize },
        TableCell,
    }

    struct Builder {
        store: NodeStoreBuilder,
        roots: Vec<NodeId>,
        stack: Vec<(Frame, Vec<NodeId>, NodeAttrs)>,
        pending_attrs: NodeAttrs,
    }

    impl Builder {
        fn take_attrs(&mut self) -> NodeAttrs {
            std::mem::replace(&mut self.pending_attrs, NodeAttrs::empty())
        }

        fn push(&mut self, id: NodeId) {
            match self.stack.last_mut() {
                Some((_, children, _)) => children.push(id),
                None => self.roots.push(id),
            }
        }

        fn leaf(
            &mut self,
            kind: NodeKind,
            rest: &str,
            lines: &mut Peekable<Lines<'_>>,
        ) -> Result<(), String> {
            let pending = parse_inline_pending(rest)?;
            let attrs = self.take_attrs();
            // Atom node lines follow the leaf line they are placed in; each
            // may carry its own attrs line. Token indices map by order.
            let mut atom_ids: Vec<NodeId> = Vec::new();
            while let Some(peeked) = lines.peek() {
                let line = (*peeked).to_owned();
                if let Some(spec) = line.strip_prefix("@\t") {
                    lines.next();
                    self.pending_attrs = parse_attrs(spec)?;
                    continue;
                }
                if line == "@" {
                    return Err("empty attrs line".to_owned());
                }
                let Some(atom_rest) = line.strip_prefix("atom\t") else {
                    break;
                };
                lines.next();
                let mut fields = atom_rest.splitn(2, '\t');
                let kind = fields
                    .next()
                    .ok_or_else(|| "atom line missing kind".to_owned())
                    .and_then(|key| AtomKind::new(key).map_err(|error| error.to_string()))?;
                let fallback = fields
                    .next()
                    .ok_or("atom line missing fallback text")?
                    .to_owned();
                let fallback = unescape_text(&fallback);
                let content =
                    InlineAtomContent::new(fallback).map_err(|error| error.to_string())?;
                let atom_attrs = self.take_attrs();
                let id = self
                    .store
                    .insert(
                        NodeKind::InlineAtom(kind),
                        atom_attrs,
                        NodeContent::InlineAtom(content),
                    )
                    .map_err(|error| error.to_string())?;
                atom_ids.push(id);
            }

            let text: String = pending
                .segments
                .iter()
                .map(|(segment, _)| segment.as_str())
                .collect();
            let buffer = TextBuffer::from_string(text);
            let placements = pending
                .placements
                .iter()
                .map(|(index, raw)| -> Result<InlineAtomPlacement, String> {
                    let id = atom_ids
                        .get(*index)
                        .copied()
                        .ok_or_else(|| format!("inline references undefined atom {index}"))?;
                    let offset = buffer.offset_at(*raw).map_err(|error| error.to_string())?;
                    Ok(InlineAtomPlacement::new(id, offset))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let runs = pending
                .segments
                .into_iter()
                .map(|(text, marks)| TextRun::new(text, marks))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let inline =
                InlineContent::with_atoms(runs, placements).map_err(|error| error.to_string())?;
            let id = self
                .store
                .insert(kind, attrs, NodeContent::Inline(inline))
                .map_err(|error| error.to_string())?;
            self.push(id);
            Ok(())
        }

        fn finish(mut self) -> Result<XiaomuDocument, String> {
            if !self.stack.is_empty() {
                return Err("unclosed container".to_owned());
            }
            if !self.pending_attrs.is_empty() {
                return Err("attrs without a following node".to_owned());
            }
            let root = self
                .store
                .insert(
                    NodeKind::Document,
                    NodeAttrs::empty(),
                    NodeContent::children(self.roots),
                )
                .map_err(|error| error.to_string())?;
            XiaomuDocument::new(root, self.store.finish()).map_err(|error| error.to_string())
        }
    }

    let mut builder = Builder {
        store: NodeStoreBuilder::new(),
        roots: Vec::new(),
        stack: Vec::new(),
        pending_attrs: NodeAttrs::empty(),
    };

    let mut lines = lines.peekable();
    while let Some(line) = lines.next() {
        if let Some(spec) = line.strip_prefix("@\t") {
            builder.pending_attrs = parse_attrs(spec)?;
            continue;
        }
        if line == "@" {
            return Err("empty attrs line".to_owned());
        }
        let (tag, rest) = match line.split_once('\t') {
            Some((tag, rest)) => (tag, rest),
            None => (line, ""),
        };
        if matches!(tag, "table" | "trow" | "tcell") && !tables_allowed {
            return Err("table frames require fixture v5".to_owned());
        }
        match tag {
            "p" => builder.leaf(NodeKind::Paragraph, rest, &mut lines)?,
            "hr" => {
                let attrs = builder.take_attrs();
                let id = builder
                    .store
                    .insert(NodeKind::HorizontalRule, attrs, NodeContent::Atomic)
                    .map_err(|error| error.to_string())?;
                builder.push(id);
            }
            "img" => {
                let attrs = builder.take_attrs();
                let id = builder
                    .store
                    .insert(NodeKind::Image, attrs, NodeContent::Atomic)
                    .map_err(|error| error.to_string())?;
                builder.push(id);
            }
            "code" => builder.leaf(NodeKind::CodeBlock, rest, &mut lines)?,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..]
                    .parse::<u8>()
                    .map_err(|_| format!("bad heading level: {tag}"))?;
                let kind =
                    NodeKind::Heading(HeadingLevel::new(level).map_err(|error| error.to_string())?);
                builder.leaf(kind, rest, &mut lines)?;
            }
            "quote" => {
                let attrs = builder.take_attrs();
                builder.stack.push((Frame::Quote, Vec::new(), attrs));
            }
            "ul" => {
                let attrs = builder.take_attrs();
                builder.stack.push((Frame::BulletList, Vec::new(), attrs));
            }
            "ol" => {
                let attrs = builder.take_attrs();
                builder.stack.push((Frame::OrderedList, Vec::new(), attrs));
            }
            "li" => {
                let attrs = builder.take_attrs();
                builder.stack.push((Frame::ListItem, Vec::new(), attrs));
            }
            "table" => {
                let attrs = builder.take_attrs();
                let mut fields = rest.split('\t');
                let rows = fields
                    .next()
                    .ok_or("table line missing row count")?
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?;
                let columns = fields
                    .next()
                    .ok_or("table line missing column count")?
                    .parse::<usize>()
                    .map_err(|error| error.to_string())?;
                if fields.next().is_some() {
                    return Err("extra table dimensions".to_owned());
                }
                if rows == 0 || columns == 0 {
                    return Err("table dimensions must be at least 1".to_owned());
                }
                builder
                    .stack
                    .push((Frame::Table { rows, columns }, Vec::new(), attrs));
            }
            "trow" => {
                let attrs = builder.take_attrs();
                // The enclosing table frame dictates the expected cell count.
                let columns = match builder.stack.last() {
                    Some((Frame::Table { columns, .. }, _, _)) => *columns,
                    _ => return Err("trow outside a table frame".to_owned()),
                };
                builder
                    .stack
                    .push((Frame::TableRow { columns }, Vec::new(), attrs));
            }
            "tcell" => {
                let attrs = builder.take_attrs();
                match builder.stack.last() {
                    Some((Frame::TableRow { .. }, _, _)) => {}
                    _ => return Err("tcell outside a table row frame".to_owned()),
                }
                builder.stack.push((Frame::TableCell, Vec::new(), attrs));
            }
            "end" => {
                let (frame, children, attrs) = builder
                    .stack
                    .pop()
                    .ok_or_else(|| "unbalanced end".to_owned())?;
                let kind = match frame {
                    Frame::Quote => NodeKind::Quote,
                    Frame::BulletList => NodeKind::BulletList,
                    Frame::OrderedList => NodeKind::OrderedList,
                    Frame::ListItem => NodeKind::ListItem,
                    Frame::Table { rows, .. } => {
                        if children.len() != rows {
                            return Err(format!(
                                "table declares {rows} rows but carries {}",
                                children.len()
                            ));
                        }
                        NodeKind::Table
                    }
                    Frame::TableRow { columns } => {
                        if children.len() != columns {
                            return Err(format!(
                                "table row declares {columns} cells but carries {}",
                                children.len()
                            ));
                        }
                        NodeKind::TableRow
                    }
                    Frame::TableCell => NodeKind::TableCell,
                };
                let id = builder
                    .store
                    .insert(kind, attrs, NodeContent::children(children))
                    .map_err(|error| error.to_string())?;
                builder.push(id);
            }
            other => return Err(format!("unknown line tag: {other}")),
        }
    }

    builder.finish()
}
