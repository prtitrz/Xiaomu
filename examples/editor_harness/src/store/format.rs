//! Harness-internal fixture text format (v4).
//!
//! Not a codec: this encodes current-stage canonical semantics for the
//! host-contract harness only.

use std::collections::BTreeMap;
use std::iter::Peekable;
use std::str::Lines;

use xiaomu_core::document::{
    AtomKind, AttrValue, HeadingLevel, InlineAtomContent, InlineAtomPlacement, InlineContent,
    MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::persistence::PersistenceError;

use super::marks_text::{encode_marks, parse_marks};

pub(crate) fn write_node(
    document: &XiaomuDocument,
    id: NodeId,
    out: &mut String,
) -> Result<(), PersistenceError> {
    let Some(node) = document.node(id) else {
        return Err(PersistenceError(
            "fixture document references a missing node".to_owned(),
        ));
    };
    if !node.attrs().is_empty() {
        out.push_str("@\t");
        out.push_str(&encode_attrs(node.attrs())?);
        out.push('\n');
    }
    match (node.kind(), node.content()) {
        (NodeKind::Paragraph, NodeContent::Inline(inline)) => {
            write_inline_leaf(document, "p", inline, out)?;
        }
        (NodeKind::Heading(level), NodeContent::Inline(inline)) => {
            write_inline_leaf(document, &format!("h{}", level.as_u8()), inline, out)?;
        }
        (NodeKind::CodeBlock, NodeContent::Inline(inline)) => {
            write_inline_leaf(document, "code", inline, out)?;
        }
        (NodeKind::HorizontalRule, NodeContent::Atomic) => {
            out.push_str("hr\n");
        }
        (NodeKind::Image, NodeContent::Atomic) => {
            out.push_str("img\n");
        }
        (_, NodeContent::Children(children)) => {
            match node.kind() {
                NodeKind::Quote => out.push_str("quote\n"),
                NodeKind::BulletList => out.push_str("ul\n"),
                NodeKind::OrderedList => out.push_str("ol\n"),
                NodeKind::ListItem => out.push_str("li\n"),
                NodeKind::Document => {}
                _ => return Err(unsupported_node_error(node.kind())),
            }
            for child in children {
                write_node(document, *child, out)?;
            }
            if !matches!(node.kind(), NodeKind::Document) {
                out.push_str("end\n");
            }
        }
        _ => return Err(unsupported_node_error(node.kind())),
    }
    Ok(())
}

fn unsupported_node_error(kind: &NodeKind) -> PersistenceError {
    PersistenceError(format!(
        "fixture format does not encode node kind {kind:?}; refusing to save a lossy snapshot"
    ))
}

/// Writes one inline-bearing leaf line plus the atom node lines its
/// placements reference (v3). Atom indices in the line are positions in the
/// order atoms are first referenced.
fn write_inline_leaf(
    document: &XiaomuDocument,
    tag: &str,
    inline: &InlineContent,
    out: &mut String,
) -> Result<(), PersistenceError> {
    let mut atom_ids = Vec::new();
    out.push_str(tag);
    out.push('\t');
    out.push_str(&encode_inline(inline, &mut atom_ids)?);
    out.push('\n');
    for atom in atom_ids {
        write_atom(document, atom, out)?;
    }
    Ok(())
}

fn write_atom(
    document: &XiaomuDocument,
    id: NodeId,
    out: &mut String,
) -> Result<(), PersistenceError> {
    let Some(node) = document.node(id) else {
        return Err(PersistenceError(
            "fixture document references a missing atom node".to_owned(),
        ));
    };
    let NodeKind::InlineAtom(kind) = node.kind() else {
        return Err(PersistenceError(
            "inline atom placement references a non-atom node".to_owned(),
        ));
    };
    let NodeContent::InlineAtom(content) = node.content() else {
        return Err(PersistenceError(
            "inline atom node has non-atom content".to_owned(),
        ));
    };
    if !node.attrs().is_empty() {
        out.push_str("@\t");
        out.push_str(&encode_attrs(node.attrs())?);
        out.push('\n');
    }
    out.push_str("atom\t");
    out.push_str(kind.as_str());
    out.push('\t');
    out.push_str(&escape_text(content.fallback_text()));
    out.push('\n');
    Ok(())
}

fn encode_inline(
    inline: &InlineContent,
    atom_ids: &mut Vec<NodeId>,
) -> Result<String, PersistenceError> {
    enum Item {
        Text(String, String),
        Atom(usize),
    }

    let mut items: Vec<Item> = Vec::new();
    let mut cursor = 0usize;
    let mut pending = inline.atoms().iter().peekable();

    for run in inline.runs() {
        let run_start = cursor;
        let run_end = run_start + run.len_bytes();
        cursor = run_end;
        let mut text_start = run_start;
        // Anchors are byte-sorted; one anchored exactly at `run_end` is
        // consumed here so the next run starts after it.
        while let Some(placement) = pending.peek() {
            let anchor = placement.text_offset().as_usize();
            if anchor > run_end {
                break;
            }
            if anchor > text_start {
                items.push(Item::Text(
                    escape_text(&run.text().as_str()[text_start - run_start..anchor - run_start]),
                    encode_marks(run.marks())?,
                ));
            }
            items.push(Item::Atom(atom_ids.len()));
            atom_ids.push(placement.atom());
            text_start = anchor;
            pending.next();
        }
        if run_end > text_start {
            items.push(Item::Text(
                escape_text(&run.text().as_str()[text_start - run_start..run_end - run_start]),
                encode_marks(run.marks())?,
            ));
        }
    }
    // Only reachable when the canonical text is empty and atoms anchor at 0.
    for placement in pending {
        if placement.text_offset().as_usize() != cursor {
            return Err(PersistenceError(
                "inline atom placement is not representable in the fixture format".to_owned(),
            ));
        }
        items.push(Item::Atom(atom_ids.len()));
        atom_ids.push(placement.atom());
    }

    let mut out = String::new();
    let mut first = true;
    for item in items {
        if !first {
            out.push('\t');
        }
        first = false;
        match item {
            Item::Text(text, marks) => {
                out.push_str(&text);
                out.push('\t');
                out.push_str(&marks);
            }
            Item::Atom(index) => {
                out.push_str("{a#");
                out.push_str(&index.to_string());
                out.push('}');
            }
        }
    }
    Ok(out)
}

fn encode_attrs(attrs: &NodeAttrs) -> Result<String, PersistenceError> {
    let mut parts = Vec::new();
    for (key, value) in attrs.iter() {
        parts.push(format!(
            "{}={}",
            escape_text(key),
            encode_attr_value(value)?
        ));
    }
    Ok(parts.join("\t"))
}

fn encode_attr_value(value: &AttrValue) -> Result<String, PersistenceError> {
    match value {
        AttrValue::Bool(flag) => Ok(format!("b:{}", flag)),
        AttrValue::Integer(number) => Ok(format!("i:{number}")),
        AttrValue::String(text) => Ok(format!("s:{}", escape_text(text))),
        AttrValue::List(_) | AttrValue::Object(_) => Err(PersistenceError(
            "fixture format does not encode list/object attrs".to_owned(),
        )),
        _ => Err(PersistenceError(
            "fixture format does not encode this attr value".to_owned(),
        )),
    }
}

fn parse_attrs(spec: &str) -> Result<NodeAttrs, String> {
    if spec.is_empty() {
        return Ok(NodeAttrs::empty());
    }
    let mut values = BTreeMap::new();
    for part in spec.split('\t') {
        let (key, encoded) = part
            .split_once('=')
            .ok_or_else(|| format!("bad attr field: {part}"))?;
        values.insert(unescape_text(key), parse_attr_value(encoded)?);
    }
    NodeAttrs::new(values).map_err(|error| error.to_string())
}

fn parse_attr_value(encoded: &str) -> Result<AttrValue, String> {
    let (tag, rest) = encoded
        .split_once(':')
        .ok_or_else(|| format!("bad attr value: {encoded}"))?;
    match tag {
        "b" => match rest {
            "true" => Ok(AttrValue::Bool(true)),
            "false" => Ok(AttrValue::Bool(false)),
            _ => Err(format!("bad bool attr: {rest}")),
        },
        "i" => rest
            .parse::<i64>()
            .map(AttrValue::Integer)
            .map_err(|_| format!("bad int attr: {rest}")),
        "s" => Ok(AttrValue::String(unescape_text(rest))),
        _ => Err(format!("unknown attr type: {tag}")),
    }
}

pub fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('{', "\\{")
}

pub fn unescape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('{') => out.push('{'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

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
    match lines.next() {
        Some("xiaomu-fixture-doc v2" | "xiaomu-fixture-doc v3" | "xiaomu-fixture-doc v4") => {}
        _ => return Err("unknown fixture header".to_owned()),
    }

    enum Frame {
        Quote,
        BulletList,
        OrderedList,
        ListItem,
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
