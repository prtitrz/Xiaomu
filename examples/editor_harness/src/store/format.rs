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
        let Some(href) = fields.first() else {
            return Err("link mark missing href".to_owned());
        };
        if fields.len() > 2 {
            return Err("link mark has too many fields".to_owned());
        }
        let title = fields.get(1).map(|value| unescape_mark_field(value));
        return Ok(Mark::Link(LinkMark::new(
            unescape_mark_field(href),
            title,
        )));
    }
    Err(format!("unknown mark: {token}"))
}

fn split_escaped(spec: &str, delimiter: char) -> Vec<String> {
    let mut parts = Vec::new();
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
        } else if character == delimiter {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    parts.push(current);
    parts
}

pub(crate) fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

pub(crate) fn unescape_text(text: &str) -> String {
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

fn encode_attrs(attrs: &NodeAttrs) -> Result<String, PersistenceError> {
    attrs
        .iter()
        .map(|(key, value)| {
            Ok(format!(
                "{}={}",
                escape_attr_field(key),
                encode_attr_value(value)?
            ))
        })
        .collect::<Result<Vec<_>, PersistenceError>>()
        .map(|parts| parts.join(";"))
}

fn encode_attr_value(value: &AttrValue) -> Result<String, PersistenceError> {
    match value {
        AttrValue::Bool(value) => Ok(format!("b:{value}")),
        AttrValue::Integer(value) => Ok(format!("i:{value}")),
        AttrValue::String(value) => Ok(format!("s:{}", escape_attr_field(value))),
        AttrValue::List(values) => values
            .iter()
            .map(encode_attr_value)
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| format!("l:[{}]", parts.join(","))),
        AttrValue::Object(values) => values
            .iter()
            .map(|(key, value)| {
                Ok(format!(
                    "{}={}",
                    escape_attr_field(key),
                    encode_attr_value(value)?
                ))
            })
            .collect::<Result<Vec<_>, PersistenceError>>()
            .map(|parts| format!("o:{{{}}}", parts.join(";"))),
        _ => Err(PersistenceError(
            "fixture format does not encode this node attribute".to_owned(),
        )),
    }
}

fn escape_attr_field(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('{', "\\{")
        .replace('}', "\\}")
}

fn parse_attrs(spec: &str) -> Result<NodeAttrs, String> {
    let mut values = BTreeMap::new();
    for item in split_escaped(spec, ';') {
        let parts = split_escaped(&item, '=');
        if parts.len() != 2 {
            return Err("invalid node attribute entry".to_owned());
        }
        values.insert(
            unescape_attr_field(&parts[0]),
            parse_attr_value(&parts[1])?,
        );
    }
    NodeAttrs::new(values).map_err(|error| error.to_string())
}

fn unescape_attr_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            } else {
                out.push('\\');
            }
        } else {
            out.push(character);
        }
    }
    out
}

fn parse_attr_value(spec: &str) -> Result<AttrValue, String> {
    if let Some(value) = spec.strip_prefix("b:") {
        return match value {
            "true" => Ok(AttrValue::Bool(true)),
            "false" => Ok(AttrValue::Bool(false)),
            _ => Err("invalid bool attribute".to_owned()),
        };
    }
    if let Some(value) = spec.strip_prefix("i:") {
        return value
            .parse::<i64>()
            .map(AttrValue::Integer)
            .map_err(|_| "invalid integer attribute".to_owned());
    }
    if let Some(value) = spec.strip_prefix("s:") {
        return Ok(AttrValue::String(unescape_attr_field(value)));
    }
    if let Some(value) = spec.strip_prefix("l:[").and_then(|v| v.strip_suffix(']')) {
        let values = if value.is_empty() {
            Vec::new()
        } else {
            split_escaped(value, ',')
                .iter()
                .map(|item| parse_attr_value(item))
                .collect::<Result<Vec<_>, _>>()?
        };
        return Ok(AttrValue::List(values));
    }
    if let Some(value) = spec.strip_prefix("o:{").and_then(|v| v.strip_suffix('}')) {
        let mut values = BTreeMap::new();
        if !value.is_empty() {
            for item in split_escaped(value, ';') {
                let parts = split_escaped(&item, '=');
                if parts.len() != 2 {
                    return Err("invalid object attribute entry".to_owned());
                }
                values.insert(
                    unescape_attr_field(&parts[0]),
                    parse_attr_value(&parts[1])?,
                );
            }
        }
        return Ok(AttrValue::Object(values));
    }
    Err("unknown node attribute encoding".to_owned())
}

pub fn parse_document(text: &str) -> Result<XiaomuDocument, String> {
    let mut lines = text.lines();
    if lines.next() != Some("xiaomu-fixture-doc v2") {
        return Err("unsupported fixture header".to_owned());
    }
    let mut builder = NodeStoreBuilder::new();
    let mut stack: Vec<(NodeKind, NodeAttrs, Vec<NodeId>)> = vec![
        (NodeKind::Document, NodeAttrs::empty(), Vec::new()),
    ];
    let mut pending_attrs = NodeAttrs::empty();

    for line in lines {
        if let Some(spec) = line.strip_prefix("@\t") {
            if !pending_attrs.is_empty() {
                return Err("multiple attribute lines before one node".to_owned());
            }
            pending_attrs = parse_attrs(spec)?;
            continue;
        }
        if line == "end" {
            let Some((kind, attrs, children)) = stack.pop() else {
                return Err("unexpected end".to_owned());
            };
            if matches!(kind, NodeKind::Document) {
                return Err("cannot close document root explicitly".to_owned());
            }
            let id = builder
                .insert(kind, attrs, NodeContent::children(children))
                .map_err(|error| error.to_string())?;
            let Some((_, _, parent_children)) = stack.last_mut() else {
                return Err("container has no parent".to_owned());
            };
            parent_children.push(id);
            continue;
        }

        let mut columns = line.split('\t');
        let tag = columns.next().unwrap_or_default();
        match tag {
            "quote" | "ul" | "ol" | "li" => {
                if columns.next().is_some() {
                    return Err("container line has unexpected fields".to_owned());
                }
                let kind = match tag {
                    "quote" => NodeKind::Quote,
                    "ul" => NodeKind::BulletList,
                    "ol" => NodeKind::OrderedList,
                    "li" => NodeKind::ListItem,
                    _ => unreachable!(),
                };
                stack.push((kind, std::mem::take(&mut pending_attrs), Vec::new()));
            }
            "p" | "code" => {
                let inline = parse_inline_columns(columns)?;
                let kind = if tag == "p" {
                    NodeKind::Paragraph
                } else {
                    NodeKind::CodeBlock
                };
                let id = builder
                    .insert(
                        kind,
                        std::mem::take(&mut pending_attrs),
                        NodeContent::Inline(inline),
                    )
                    .map_err(|error| error.to_string())?;
                stack
                    .last_mut()
                    .ok_or_else(|| "leaf has no parent".to_owned())?
                    .2
                    .push(id);
            }
            tag if tag.starts_with('h') => {
                let level = tag[1..]
                    .parse::<u8>()
                    .map_err(|_| "invalid heading tag".to_owned())?;
                let inline = parse_inline_columns(columns)?;
                let id = builder
                    .insert(
                        NodeKind::Heading(
                            HeadingLevel::new(level).map_err(|error| error.to_string())?,
                        ),
                        std::mem::take(&mut pending_attrs),
                        NodeContent::Inline(inline),
                    )
                    .map_err(|error| error.to_string())?;
                stack
                    .last_mut()
                    .ok_or_else(|| "heading has no parent".to_owned())?
                    .2
                    .push(id);
            }
            _ => return Err(format!("unknown fixture tag: {tag}")),
        }
    }

    if !pending_attrs.is_empty() {
        return Err("attribute line without following node".to_owned());
    }
    if stack.len() != 1 {
        return Err("unclosed container".to_owned());
    }
    let (_, _, children) = stack.pop().unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .map_err(|error| error.to_string())?;
    XiaomuDocument::new(root, builder.finish()).map_err(|error| error.to_string())
}

fn parse_inline_columns<'a>(columns: impl Iterator<Item = &'a str>) -> Result<InlineContent, String> {
    let fields: Vec<&str> = columns.collect();
    if fields.len() % 2 != 0 {
        return Err("inline run fields must be text/marks pairs".to_owned());
    }
    let mut runs = Vec::new();
    for pair in fields.chunks_exact(2) {
        let text = unescape_text(pair[0]);
        if text.is_empty() {
            continue;
        }
        runs.push(TextRun::new(text, parse_marks(pair[1])?).map_err(|error| error.to_string())?);
    }
    InlineContent::new(runs).map_err(|error| error.to_string())
}
