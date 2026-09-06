//! Baseline Markdown writer over the canonical document model.
//!
//! The writer is total over the built-in semantics it supports and refuses
//! everything else. It never silently drops content: unknown node kinds,
//! inline atoms, unknown attributes, host asset images, and whitespace that
//! cannot survive a round-trip all fail export.

use xiaomu_core::document::{
    IMAGE_ATTR_ALT, IMAGE_ATTR_ASSET, IMAGE_ATTR_HEIGHT, IMAGE_ATTR_SRC, IMAGE_ATTR_TITLE,
    IMAGE_ATTR_WIDTH, ImageAttrs, ImageSource, InlineContent, LinkMark, Mark, MarkKind, Node,
    NodeContent, NodeKind, TextRun, XiaomuDocument,
};

use xiaomu_core::Error;

use crate::CODE_BLOCK_ATTR_LANGUAGE;
use crate::error::{MarkdownCodecError, Result};
use crate::escape::{
    code_block_fence, code_span_fence, escape_destination, escape_text, escape_title,
};

/// Serializes a canonical document into baseline Markdown.
///
/// The output is canonical: blocks are separated by exactly one blank line,
/// bullet lists use `- `, ordered lists renumber from 1, and hard breaks use
/// the backslash form. The canonical document is never modified.
pub fn to_markdown(document: &XiaomuDocument) -> Result<String> {
    let root = document
        .node(document.root())
        .ok_or(MarkdownCodecError::InvalidDocument(Error::UnknownNode))?;
    require_known_attrs(root, "Document", &[])?;

    let children = match root.content() {
        NodeContent::Children(children) => children,
        _ => {
            return Err(MarkdownCodecError::InvalidDocument(
                Error::InvalidNodeContent,
            ));
        }
    };

    let mut out = String::new();
    for (index, &child) in children.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        write_block(document, child, 0, &mut out)?;
    }
    Ok(out)
}

fn write_block(
    document: &XiaomuDocument,
    id: xiaomu_core::document::NodeId,
    indent: usize,
    out: &mut String,
) -> Result<()> {
    let node = document
        .node(id)
        .ok_or(MarkdownCodecError::InvalidDocument(Error::UnknownNode))?;
    let pad = " ".repeat(indent);

    match node.kind() {
        NodeKind::Paragraph => {
            let text = paragraph_lines(document, node)?;
            emit_indented_lines(&text, &pad, out);
        }
        NodeKind::Heading(level) => {
            require_known_attrs(node, "Heading", &[])?;
            let text = inline_text(document, node)?;
            if text.is_empty() {
                return Err(MarkdownCodecError::EmptyParagraph);
            }
            if text.contains('\n') {
                return Err(MarkdownCodecError::HardBreakNotRepresentable {
                    kind: "Heading".to_owned(),
                });
            }
            reject_lossy_whitespace(&text, "Heading")?;
            let trimmed = text.trim_end_matches('#');
            if trimmed.len() < text.len() && (trimmed.is_empty() || trimmed.ends_with(' ')) {
                return Err(MarkdownCodecError::LossyWhitespace {
                    kind: "Heading".to_owned(),
                });
            }
            out.push_str(&pad);
            out.push_str(&"#".repeat(level.as_u8().into()));
            out.push(' ');
            out.push_str(&escape_text(&text));
            out.push('\n');
        }
        NodeKind::HorizontalRule => {
            require_known_attrs(node, "HorizontalRule", &[])?;
            out.push_str(&pad);
            out.push_str("---\n");
        }
        NodeKind::Image => write_image(node, &pad, out)?,
        NodeKind::CodeBlock => write_code_block(node, &pad, out)?,
        NodeKind::Quote => {
            require_known_attrs(node, "Quote", &[])?;
            let children = children_of(node)?;
            let mut inner = String::new();
            for (index, &child) in children.iter().enumerate() {
                if index > 0 {
                    inner.push('\n');
                }
                write_block(document, child, 0, &mut inner)?;
            }
            for line in inner.split_inclusive('\n') {
                let content = line.trim_end_matches('\n');
                out.push_str(&pad);
                if content.is_empty() {
                    out.push('>');
                } else {
                    out.push_str("> ");
                    out.push_str(content);
                }
                out.push('\n');
            }
            if inner.is_empty() {
                out.push_str(&pad);
                out.push_str(">\n");
            }
        }
        NodeKind::BulletList | NodeKind::OrderedList => {
            write_list(document, node, indent, out)?;
        }
        NodeKind::InlineAtom(_)
        | NodeKind::Custom(_)
        | NodeKind::ListItem
        | NodeKind::Document
        | _ => {
            return Err(MarkdownCodecError::UnsupportedNodeKind {
                kind: describe_kind(node.kind()),
            });
        }
    }
    Ok(())
}

fn write_image(node: &Node, pad: &str, out: &mut String) -> Result<()> {
    require_known_attrs(
        node,
        "Image",
        &[
            IMAGE_ATTR_SRC,
            IMAGE_ATTR_ASSET,
            IMAGE_ATTR_ALT,
            IMAGE_ATTR_TITLE,
            IMAGE_ATTR_WIDTH,
            IMAGE_ATTR_HEIGHT,
        ],
    )?;
    let image =
        ImageAttrs::from_attrs(node.attrs()).map_err(MarkdownCodecError::InvalidDocument)?;

    let mut dropped = Vec::new();
    if image.width().is_some() {
        dropped.push(IMAGE_ATTR_WIDTH.to_owned());
    }
    if image.height().is_some() {
        dropped.push(IMAGE_ATTR_HEIGHT.to_owned());
    }
    if !dropped.is_empty() {
        return Err(MarkdownCodecError::UnsupportedAttributes {
            kind: "Image".to_owned(),
            keys: dropped,
        });
    }
    let url = match image.source() {
        ImageSource::ExternalUrl(url) => url,
        ImageSource::AssetRef(_) => return Err(MarkdownCodecError::AssetImageNotExportable),
    };

    out.push_str(pad);
    out.push_str("![");
    out.push_str(&escape_text(image.alt()));
    out.push_str("](");
    out.push_str(&escape_destination(url));
    if let Some(title) = image.title() {
        out.push_str(" \"");
        out.push_str(&escape_title(title));
        out.push('"');
    }
    out.push_str(")\n");
    Ok(())
}

fn write_code_block(node: &Node, pad: &str, out: &mut String) -> Result<()> {
    require_known_attrs(node, "CodeBlock", &[CODE_BLOCK_ATTR_LANGUAGE])?;
    let language = match node.attrs().get(CODE_BLOCK_ATTR_LANGUAGE) {
        None => None,
        Some(xiaomu_core::document::AttrValue::String(value)) => Some(value.as_str()),
        Some(_) => {
            return Err(MarkdownCodecError::InvalidDocument(
                Error::InvalidNodeContent,
            ));
        }
    };

    let inline = inline_content(node)?;
    let mut body = String::new();
    for run in inline.runs() {
        if let Some(mark) = run.marks().as_slice().first() {
            return Err(MarkdownCodecError::UnsupportedMark {
                mark: describe_mark(mark),
            });
        }
        body.push_str(run.text().as_str());
    }
    let fence = code_block_fence(&body);
    out.push_str(pad);
    out.push_str(&fence);
    if let Some(language) = language {
        out.push_str(language);
    }
    out.push('\n');
    if !body.is_empty() {
        out.push_str(&body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out.push_str(pad);
    out.push_str(&fence);
    out.push('\n');
    Ok(())
}

fn write_list(
    document: &XiaomuDocument,
    node: &Node,
    indent: usize,
    out: &mut String,
) -> Result<()> {
    let ordered = matches!(node.kind(), NodeKind::OrderedList);
    require_known_attrs(node, "List", &[])?;
    let children = children_of(node)?;

    for (index, &item) in children.iter().enumerate() {
        let item_node = document
            .node(item)
            .ok_or(MarkdownCodecError::InvalidDocument(Error::UnknownNode))?;
        if !matches!(item_node.kind(), NodeKind::ListItem) {
            return Err(MarkdownCodecError::InvalidDocument(Error::InvalidChildKind));
        }
        require_known_attrs(item_node, "ListItem", &[])?;
        let item_children = children_of(item_node)?;

        let marker = if ordered {
            format!("{}. ", index + 1)
        } else {
            "- ".to_owned()
        };
        let item_pad = " ".repeat(indent + marker.len());
        let mut first_line = true;
        for (position, &child) in item_children.iter().enumerate() {
            let child_node = document
                .node(child)
                .ok_or(MarkdownCodecError::InvalidDocument(Error::UnknownNode))?;
            match child_node.kind() {
                NodeKind::Paragraph if position == 0 => {
                    let lines = paragraph_lines(document, child_node)?;
                    for line in lines {
                        if first_line {
                            out.push_str(&" ".repeat(indent));
                            out.push_str(&marker);
                            first_line = false;
                        } else {
                            out.push_str(&item_pad);
                        }
                        out.push_str(&line);
                        out.push('\n');
                    }
                }
                NodeKind::BulletList | NodeKind::OrderedList if position > 0 => {
                    let mut nested = String::new();
                    write_list(document, child_node, indent + marker.len(), &mut nested)?;
                    if first_line {
                        return Err(MarkdownCodecError::UnsupportedNodeKind {
                            kind: "ListItem".to_owned(),
                        });
                    }
                    out.push_str(&nested);
                }
                _ => {
                    return Err(MarkdownCodecError::UnsupportedNodeKind {
                        kind: "ListItem".to_owned(),
                    });
                }
            }
        }
        if first_line {
            // A list item without a leading paragraph has no baseline form.
            return Err(MarkdownCodecError::UnsupportedNodeKind {
                kind: "ListItem".to_owned(),
            });
        }
    }
    Ok(())
}

fn paragraph_lines(document: &XiaomuDocument, node: &Node) -> Result<Vec<String>> {
    require_known_attrs(node, "Paragraph", &[])?;
    let inline = inline_content(node)?;
    if !inline.atoms().is_empty() {
        let atom = document.node(inline.atoms()[0].atom());
        let kind = atom
            .map(|atom| describe_kind(atom.kind()))
            .unwrap_or_else(|| "InlineAtom".to_owned());
        return Err(MarkdownCodecError::UnsupportedNodeKind { kind });
    }
    if inline.runs().is_empty() {
        return Err(MarkdownCodecError::EmptyParagraph);
    }

    let mut lines: Vec<String> = vec![String::new()];
    for run in inline.runs() {
        for (index, piece) in render_run(run)?.into_iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            lines.last_mut().expect("always non-empty").push_str(&piece);
        }
    }
    for (index, line) in lines.iter().enumerate() {
        if !line.is_empty()
            && (line.starts_with(' ')
                || line.starts_with('\t')
                || line.ends_with(' ')
                || line.ends_with('\t'))
        {
            return Err(MarkdownCodecError::LossyWhitespace {
                kind: "Paragraph".to_owned(),
            });
        }
        if line.is_empty() && index + 1 < lines.len() {
            return Err(MarkdownCodecError::LossyWhitespace {
                kind: "Paragraph".to_owned(),
            });
        }
    }
    Ok(lines)
}

fn render_run(run: &TextRun) -> Result<Vec<String>> {
    let marks = run.marks();
    if marks.contains(MarkKind::Underline) {
        return Err(MarkdownCodecError::UnsupportedMark {
            mark: "Underline".to_owned(),
        });
    }

    let text = run.text().as_str();
    if marks.contains(MarkKind::Code) {
        for mark in marks.as_slice() {
            if !matches!(mark, Mark::Code | Mark::Link(_)) {
                return Err(MarkdownCodecError::UnsupportedMark {
                    mark: format!("Code + {}", describe_mark(mark)),
                });
            }
        }
        if text.contains('\n') {
            return Err(MarkdownCodecError::UnsupportedMark {
                mark: "Code hard break".to_owned(),
            });
        }
        let fence = code_span_fence(text);
        let mut piece = format!("{fence}{text}{fence}");
        if let Some(link) = link_mark(marks) {
            piece = wrap_link(&piece, &link);
        }
        return Ok(vec![piece]);
    }

    let mut piece = escape_text(text);
    if marks.contains(MarkKind::Italic) {
        piece = format!("*{piece}*");
    }
    if marks.contains(MarkKind::Strike) {
        piece = format!("~~{piece}~~");
    }
    if marks.contains(MarkKind::Bold) {
        piece = format!("**{piece}**");
    }
    if let Some(link) = link_mark(marks) {
        piece = wrap_link(&piece, &link);
    }
    Ok(piece.split('\n').map(str::to_owned).collect())
}

fn wrap_link(inner: &str, link: &LinkMark) -> String {
    let mut out = format!("[{inner}](");
    out.push_str(&escape_destination(link.href()));
    if let Some(title) = link.title() {
        out.push_str(" \"");
        out.push_str(&escape_title(title));
        out.push('"');
    }
    out.push(')');
    out
}

fn link_mark(marks: &xiaomu_core::document::MarkSet) -> Option<LinkMark> {
    marks.as_slice().iter().find_map(|mark| match mark {
        Mark::Link(link) => Some(link.clone()),
        _ => None,
    })
}

fn describe_mark(mark: &Mark) -> String {
    match mark {
        Mark::Bold => "Bold".to_owned(),
        Mark::Italic => "Italic".to_owned(),
        Mark::Code => "Code".to_owned(),
        Mark::Underline => "Underline".to_owned(),
        Mark::Strike => "Strike".to_owned(),
        Mark::Link(_) => "Link".to_owned(),
        _ => "Unknown".to_owned(),
    }
}

fn inline_text(document: &XiaomuDocument, node: &Node) -> Result<String> {
    let inline = inline_content(node)?;
    if !inline.atoms().is_empty() {
        let atom = document.node(inline.atoms()[0].atom());
        let kind = atom
            .map(|atom| describe_kind(atom.kind()))
            .unwrap_or_else(|| "InlineAtom".to_owned());
        return Err(MarkdownCodecError::UnsupportedNodeKind { kind });
    }
    Ok(inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect::<String>())
}

fn inline_content(node: &Node) -> Result<&InlineContent> {
    match node.content() {
        NodeContent::Inline(inline) => Ok(inline),
        _ => Err(MarkdownCodecError::InvalidDocument(
            Error::InvalidNodeContent,
        )),
    }
}

fn children_of(node: &Node) -> Result<&[xiaomu_core::document::NodeId]> {
    match node.content() {
        NodeContent::Children(children) => Ok(children),
        _ => Err(MarkdownCodecError::InvalidDocument(
            Error::InvalidNodeContent,
        )),
    }
}

fn require_known_attrs(node: &Node, description: &str, allowed: &[&str]) -> Result<()> {
    let unsupported: Vec<String> = node
        .attrs()
        .iter()
        .map(|(key, _)| key.to_owned())
        .filter(|key| !allowed.contains(&key.as_str()))
        .collect();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(MarkdownCodecError::UnsupportedAttributes {
            kind: description.to_owned(),
            keys: unsupported,
        })
    }
}

fn reject_lossy_whitespace(text: &str, kind: &str) -> Result<()> {
    if text.starts_with(' ') || text.starts_with('\t') {
        return Err(MarkdownCodecError::LossyWhitespace {
            kind: kind.to_owned(),
        });
    }
    Ok(())
}

fn emit_indented_lines(lines: &[String], pad: &str, out: &mut String) {
    let mut iter = lines.iter();
    let Some(first) = iter.next() else {
        return;
    };
    out.push_str(pad);
    out.push_str(first);
    for line in iter {
        out.push('\\');
        out.push('\n');
        out.push_str(pad);
        out.push_str(line);
    }
    out.push('\n');
}

pub(crate) fn describe_kind(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Document => "Document".to_owned(),
        NodeKind::Paragraph => "Paragraph".to_owned(),
        NodeKind::Heading(level) => format!("Heading{}", level.as_u8()),
        NodeKind::Quote => "Quote".to_owned(),
        NodeKind::BulletList => "BulletList".to_owned(),
        NodeKind::OrderedList => "OrderedList".to_owned(),
        NodeKind::ListItem => "ListItem".to_owned(),
        NodeKind::CodeBlock => "CodeBlock".to_owned(),
        NodeKind::HorizontalRule => "HorizontalRule".to_owned(),
        NodeKind::Image => "Image".to_owned(),
        NodeKind::InlineAtom(atom) => format!("InlineAtom({})", atom.as_str()),
        NodeKind::Custom(key) => format!("Custom({key})"),
        _ => "Unknown".to_owned(),
    }
}
