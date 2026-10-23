//! Harness-internal fixture text format (v4/v5).
//!
//! Not a codec: this encodes current-stage canonical semantics for the
//! host-contract harness only. v5 adds table blocks (`table`/`trow`/`tcell`
//! container frames); the v4 write path still refuses tables so no snapshot
//! silently downgrades.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    AttrValue, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind, XiaomuDocument,
};
use xiaomu_runtime::persistence::PersistenceError;

use super::marks_text::encode_marks;

mod parse;
pub use parse::parse_document;

pub(crate) fn write_node(
    document: &XiaomuDocument,
    id: NodeId,
    out: &mut String,
) -> Result<(), PersistenceError> {
    write_node_versioned(document, id, out, false)
}

/// v5 write path: tables encode as `table`/`trow`/`tcell` container frames.
pub(crate) fn write_node_v5(
    document: &XiaomuDocument,
    id: NodeId,
    out: &mut String,
) -> Result<(), PersistenceError> {
    write_node_versioned(document, id, out, true)
}

/// Whether any node in `id`'s subtree is a table block.
pub(crate) fn document_contains_table(document: &XiaomuDocument, id: NodeId) -> bool {
    let Some(node) = document.node(id) else {
        return false;
    };
    if matches!(node.kind(), NodeKind::Table) {
        return true;
    }
    node.content().as_children().is_some_and(|children| {
        children
            .iter()
            .any(|child| document_contains_table(document, *child))
    })
}

fn write_node_versioned(
    document: &XiaomuDocument,
    id: NodeId,
    out: &mut String,
    v5: bool,
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
        (NodeKind::Table, NodeContent::Children(rows)) => {
            if !v5 {
                return Err(unsupported_node_error(node.kind()));
            }
            let columns = rows
                .first()
                .and_then(|row| {
                    document
                        .node(*row)
                        .and_then(|row| row.content().as_children().map(<[NodeId]>::len))
                })
                .unwrap_or(0);
            out.push_str(&format!("table\t{}\t{columns}\n", rows.len()));
            for row in rows {
                write_node_versioned(document, *row, out, v5)?;
            }
            out.push_str("end\n");
        }
        (_, NodeContent::Children(children)) => {
            match node.kind() {
                NodeKind::Quote => out.push_str("quote\n"),
                NodeKind::BulletList => out.push_str("ul\n"),
                NodeKind::OrderedList => out.push_str("ol\n"),
                NodeKind::ListItem => out.push_str("li\n"),
                NodeKind::TableRow => {
                    if !v5 {
                        return Err(unsupported_node_error(node.kind()));
                    }
                    out.push_str("trow\n");
                }
                NodeKind::TableCell => {
                    if !v5 {
                        return Err(unsupported_node_error(node.kind()));
                    }
                    out.push_str("tcell\n");
                }
                NodeKind::Document => {}
                _ => return Err(unsupported_node_error(node.kind())),
            }
            for child in children {
                write_node_versioned(document, *child, out, v5)?;
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
