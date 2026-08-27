//! Harness-internal fixture text format (v2).
//!
//! Not a codec: this encodes current-stage canonical semantics for the
//! host-contract harness only.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    AttrValue, HeadingLevel, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_runtime::persistence::PersistenceError;

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
            out.push_str("p\t");
            out.push_str(&encode_inline(inline)?);
            out.push('\n');
        }
        (NodeKind::Heading(level), NodeContent::Inline(inline)) => {
            out.push_str(&format!("h{}\t", level.as_u8()));
            out.push_str(&encode_inline(inline)?);
            out.push('\n');
        }
        (NodeKind::CodeBlock, NodeContent::Inline(inline)) => {
            out.push_str("code\t");
            out.push_str(&encode_inline(inline)?);
            out.push('\n');
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

fn encode_inline(inline: &InlineContent) -> Result<String, PersistenceError> {
    let mut out = String::new();
    for (index, run) in inline.runs().iter().enumerate() {
        if index > 0 {
            out.push('\t');
        }
        out.push_str(&escape_text(run.text().as_str()));
        out.push('\t');
        out.push_str(&encode_marks(run.marks())?);
    }
    Ok(out)
}

fn encode_marks(marks: &MarkSet) -> Result<String, PersistenceError> {
    let mut parts = Vec::new();
    for mark in marks.as_slice() {
        parts.push(match mark {
            Mark::Bold => "bold".to_owned(),
            Mark::Italic => "italic".to_owned(),
            Mark::Code => "code".to_owned(),
            Mark::Underline => "underline".to_owned(),
            Mark::Strike => "strike".to_owned(),
            Mark::Link(link) => match link.title() {
                Some(title) => format!(
                    "link:{}:{}",
                    escape_mark_field(link.href()),
                    escape_mark_field(title)
                ),
                None => format!("link:{}", escape_mark_field(link.href())),
            },
            _ => {
                return Err(PersistenceError(
                    "fixture format does not encode this mark".to_owned(),
                ));
            }
        });
    }
    Ok(parts.join(","))
}

fn escape_mark_field(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
}

fn unescape_mark_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some(':') => out.push(':'),
                Some(',') => out.push(','),
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

fn parse_marks(spec: &str) -> Result<MarkSet, String> {
    if spec.is_empty() {
        return Ok(MarkSet::empty());
    }
    let mut marks = Vec::new();
    for token in split_mark_tokens(spec) {
        marks.push(parse_one_mark(&token)?);
    }
    MarkSet::new(marks).map_err(|error| error.to_string())
}

fn split_mark_tokens(spec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = spec.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => current.push('\\'),
            }
        } else if character == ',' {
            tokens.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    tokens.push(current);
    tokens
}

fn parse_one_mark(token: &str) -> Result<Mark, String> {
    if token == "bold" {
        return Ok(Mark::Bold);
    }
    if token == "italic" {
        return Ok(Mark::Italic);
    }
    if token == "code" {
        return Ok(Mark::Code);
    }
    if token == "underline" {
        return Ok(Mark::Underline);
    }
    if token == "strike" {
        return Ok(Mark::Strike);
    }
    if let Some(rest) = token.strip_prefix("link:") {
        let fields = split_escaped(rest, ':');
        match fields.as_slice() {
            [href] => Ok(Mark::Link(LinkMark::new(unescape_mark_field(href), None))),
            [href, title] => Ok(Mark::Link(LinkMark::new(
                unescape_mark_field(href),
                Some(unescape_mark_field(title)),
            ))),
            _ => Err(format!("bad link mark: {token}")),
        }
    } else {
        Err(format!("unknown mark: {token}"))
    }
}

fn split_escaped(text: &str, separator: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some(next) => {
                    current.push('\\');
                    current.push(next);
                }
                None => current.push('\\'),
            }
        } else if character == separator {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    fields.push(current);
    fields
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
}

pub fn unescape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
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

fn parse_inline(rest: &str) -> Result<InlineContent, String> {
    if rest.is_empty() {
        return InlineContent::new(Vec::new()).map_err(|error| error.to_string());
    }
    let fields: Vec<&str> = rest.split('\t').collect();
    if !fields.len().is_multiple_of(2) {
        return Err("inline runs must be text/marks pairs".to_owned());
    }
    let mut runs = Vec::new();
    for chunk in fields.chunks_exact(2) {
        let text = unescape_text(chunk[0]);
        let marks = parse_marks(chunk[1])?;
        if text.is_empty() && marks.is_empty() {
            continue;
        }
        runs.push(TextRun::new(text, marks).map_err(|error| error.to_string())?);
    }
    InlineContent::new(runs).map_err(|error| error.to_string())
}

pub fn parse_document(text: &str) -> Result<XiaomuDocument, String> {
    let mut lines = text.lines();
    match lines.next() {
        Some("xiaomu-fixture-doc v2") => {}
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

        fn leaf(&mut self, kind: NodeKind, rest: &str) -> Result<(), String> {
            let inline = parse_inline(rest)?;
            let attrs = self.take_attrs();
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

    for line in lines {
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
            "p" => builder.leaf(NodeKind::Paragraph, rest)?,
            "code" => builder.leaf(NodeKind::CodeBlock, rest)?,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..]
                    .parse::<u8>()
                    .map_err(|_| format!("bad heading level: {tag}"))?;
                let kind =
                    NodeKind::Heading(HeadingLevel::new(level).map_err(|error| error.to_string())?);
                builder.leaf(kind, rest)?;
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
