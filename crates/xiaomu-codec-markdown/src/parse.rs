//! Baseline Markdown reader over the canonical document model.
//!
//! The reader is line-based and recursive: block structure comes from
//! indentation and prefix markers, inline structure from a one-pass scanner
//! with segment retro-marking. Import is deliberately strict where the
//! baseline contract has no representation (setext headings, indented code,
//! lazy quotes, inline images, reference links), so no import silently
//! reinterprets content the writer would not have produced.

use std::collections::BTreeMap;

use xiaomu_core::document::{
    HeadingLevel, ImageAttrs, ImageSource, InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId,
    NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};

use crate::CODE_BLOCK_ATTR_LANGUAGE;
use crate::error::{MarkdownCodecError, Result};
use crate::inline::scan_inline;
use crate::syntax::parse_image_syntax;

/// Parses baseline Markdown into a canonical document snapshot.
pub fn from_markdown(source: &str) -> Result<XiaomuDocument> {
    let lines = normalize_source(source);
    let mut parser = BlockParser {
        lines: &lines,
        pos: 0,
        line_base: 1,
        builder: NodeStoreBuilder::new(),
    };
    let children = parser.parse_blocks(0)?;
    let root = parser
        .builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .map_err(MarkdownCodecError::InvalidDocument)?;
    XiaomuDocument::new(root, parser.builder.finish()).map_err(MarkdownCodecError::InvalidDocument)
}

fn normalize_source(source: &str) -> Vec<String> {
    let mut lines: Vec<String> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

struct BlockParser<'a> {
    lines: &'a [String],
    pos: usize,
    line_base: usize,
    builder: NodeStoreBuilder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListMarker {
    ordered: bool,
    width: usize,
}

impl<'a> BlockParser<'a> {
    fn err(&self, reason: impl Into<String>) -> MarkdownCodecError {
        MarkdownCodecError::InvalidMarkdown {
            line: self.line_base + self.pos,
            reason: reason.into(),
        }
    }

    fn done(&self) -> bool {
        self.pos >= self.lines.len()
    }

    fn peek(&self) -> Option<&'a String> {
        self.lines.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_blank(&mut self) {
        while self
            .peek()
            .is_some_and(|line| line.chars().all(char::is_whitespace))
        {
            self.advance();
        }
    }

    fn insert(&mut self, kind: NodeKind, attrs: NodeAttrs, content: NodeContent) -> Result<NodeId> {
        self.builder
            .insert(kind, attrs, content)
            .map_err(MarkdownCodecError::InvalidDocument)
    }

    fn parse_blocks(&mut self, base: usize) -> Result<Vec<NodeId>> {
        let mut children = Vec::new();
        loop {
            self.skip_blank();
            if self.done() {
                break;
            }
            let line: &'a String = self.peek().expect("not done");
            let (indent, text) = split_indent(line);
            if indent < base {
                break;
            }

            if let Some((level, text)) = heading_marker(text) {
                self.advance();
                children.push(self.build_heading(level, text)?);
                continue;
            }
            if thematic_break(text) {
                self.advance();
                children.push(self.insert(
                    NodeKind::HorizontalRule,
                    NodeAttrs::empty(),
                    NodeContent::Atomic,
                )?);
                continue;
            }
            if fence_run(text).is_some() {
                children.push(self.parse_fence(indent)?);
                continue;
            }
            if text.starts_with('>') {
                children.push(self.parse_quote(indent)?);
                continue;
            }
            if let Some(marker) = list_marker(text) {
                children.push(self.parse_list(indent, marker)?);
                continue;
            }
            if indent >= base + 4 {
                return Err(self.err("indented code blocks are not supported"));
            }
            if text.starts_with("![") {
                self.advance();
                children.push(self.build_image(text)?);
                continue;
            }
            children.push(self.parse_paragraph(base)?);
        }
        Ok(children)
    }

    fn build_heading(&mut self, level: u8, text: &str) -> Result<NodeId> {
        let text = strip_closing_sequence(text);
        if text.is_empty() {
            return Err(self.err("empty heading"));
        }
        let inline = self.build_inline(&text)?;
        self.insert(
            NodeKind::Heading(
                HeadingLevel::new(level).map_err(MarkdownCodecError::InvalidDocument)?,
            ),
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
    }

    fn parse_fence(&mut self, indent: usize) -> Result<NodeId> {
        let line: &'a String = self.peek().expect("fence line");
        let (_, text) = split_indent(line);
        let fence_len = fence_run(text).expect("dispatched on fence");
        let info = &text[fence_len..];
        if info.contains('`') || info.contains(char::is_whitespace) {
            return Err(self.err("code fence info must be a single word without backticks"));
        }
        self.advance();

        let mut body: Vec<&'a str> = Vec::new();
        loop {
            if self.done() {
                return Err(self.err("unclosed code fence"));
            }
            let line: &'a String = self.peek().expect("not done");
            let (line_indent, _) = split_indent(line);
            let strip = line_indent.min(indent);
            let content = &line[strip..];
            if is_closing_fence(content, fence_len) {
                self.advance();
                break;
            }
            body.push(content);
            self.advance();
        }

        let mut attrs = BTreeMap::new();
        if !info.is_empty() {
            attrs.insert(
                CODE_BLOCK_ATTR_LANGUAGE.to_owned(),
                xiaomu_core::document::AttrValue::String(info.to_owned()),
            );
        }
        let content = if body.is_empty() {
            NodeContent::empty_inline()
        } else {
            let text = format!("{}\n", body.join("\n"));
            let inline = InlineContent::new([TextRun::new(text, MarkSet::empty()).expect("valid")])
                .expect("valid");
            NodeContent::Inline(inline)
        };
        self.insert(
            NodeKind::CodeBlock,
            NodeAttrs::new(attrs).map_err(MarkdownCodecError::InvalidDocument)?,
            content,
        )
    }

    fn parse_quote(&mut self, indent: usize) -> Result<NodeId> {
        let start_line = self.line_base + self.pos;
        let mut inner: Vec<String> = Vec::new();
        while let Some(line) = self.peek() {
            let (line_indent, text) = split_indent(line);
            if line_indent < indent || !text.starts_with('>') {
                if inner.is_empty() || text.chars().all(char::is_whitespace) {
                    break;
                }
                return Err(self.err("lazy quote continuation is not supported"));
            }
            let stripped = text[1..].strip_prefix(' ').unwrap_or(&text[1..]);
            inner.push(stripped.to_owned());
            self.advance();
        }
        if inner.is_empty() {
            return self.insert(
                NodeKind::Quote,
                NodeAttrs::empty(),
                NodeContent::children([]),
            );
        }

        let builder = std::mem::take(&mut self.builder);
        let mut nested = BlockParser {
            lines: &inner,
            pos: 0,
            line_base: start_line,
            builder,
        };
        let children = nested.parse_blocks(0)?;
        self.builder = nested.builder;
        self.insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
    }

    fn parse_list(&mut self, indent: usize, marker: ListMarker) -> Result<NodeId> {
        let mut items = Vec::new();
        loop {
            items.push(self.parse_list_item(indent, marker)?);
            self.skip_blank();
            if self.done() {
                break;
            }
            let line: &'a String = self.peek().expect("not done");
            let (line_indent, text) = split_indent(line);
            if line_indent != indent {
                break;
            }
            match list_marker(text) {
                Some(next) if next.ordered == marker.ordered => continue,
                _ => break,
            }
        }
        let kind = if marker.ordered {
            NodeKind::OrderedList
        } else {
            NodeKind::BulletList
        };
        self.insert(kind, NodeAttrs::empty(), NodeContent::children(items))
    }

    fn parse_list_item(&mut self, indent: usize, marker: ListMarker) -> Result<NodeId> {
        let line: &'a String = self.peek().expect("marker line");
        let (_, text) = split_indent(line);
        let rest = text[marker.width..].to_owned();
        let content_indent = indent + marker.width;
        self.advance();

        if rest.starts_with("![") {
            let image = self.build_image(&rest)?;
            self.reject_item_continuation(content_indent)?;
            return self.insert(
                NodeKind::ListItem,
                NodeAttrs::empty(),
                NodeContent::children([image]),
            );
        }

        let mut lines: Vec<String> = Vec::new();
        let mut breaks: Vec<bool> = Vec::new();
        let mut pending_hard = false;
        let mut nested: Vec<NodeId> = Vec::new();
        lines.push(rest.trim_start().to_owned());
        while let Some(line) = self.peek() {
            if line.chars().all(char::is_whitespace) {
                break;
            }
            let (line_indent, text) = split_indent(line);
            if line_indent < content_indent || line_indent >= content_indent + 4 {
                break;
            }
            if let Some(nested_marker) = list_marker(text) {
                nested.push(self.parse_list(line_indent, nested_marker)?);
                continue;
            }
            if !nested.is_empty()
                || text.starts_with('>')
                || fence_run(text).is_some()
                || heading_marker(text).is_some()
                || thematic_break(text)
            {
                return Err(
                    self.err("only paragraphs and nested lists are supported inside list items")
                );
            }
            breaks.push(pending_hard);
            let (hard, content) = strip_hard_break(text);
            lines.push(content.trim_start().to_owned());
            pending_hard = hard;
            self.advance();
        }

        let mut children = Vec::new();
        if lines.iter().any(|line| !line.is_empty()) {
            let joined = join_paragraph(&lines, &breaks);
            children.push(self.build_paragraph(&joined)?);
        } else if nested.is_empty() {
            return Err(self.err("empty list item"));
        }
        children.extend(nested);
        self.insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
    }

    fn reject_item_continuation(&mut self, content_indent: usize) -> Result<()> {
        let Some(line) = self.peek() else {
            return Ok(());
        };
        if line.chars().all(char::is_whitespace) {
            return Ok(());
        }
        let (line_indent, _) = split_indent(line);
        if line_indent < content_indent {
            return Ok(());
        }
        Err(self.err("content may not follow an image inside a list item"))
    }

    fn parse_paragraph(&mut self, base: usize) -> Result<NodeId> {
        let mut lines: Vec<String> = Vec::new();
        let mut breaks: Vec<bool> = Vec::new();
        let mut pending_hard = false;
        while let Some(line) = self.peek() {
            if line.chars().all(char::is_whitespace) {
                break;
            }
            let (indent, text) = split_indent(line);
            if !lines.is_empty() {
                if setext_underline(text) {
                    return Err(self
                        .err("setext-style headings are not supported; use ATX heading markers"));
                }
                if thematic_break(text) || interrupting_block(text) || indent >= base + 4 {
                    break;
                }
                breaks.push(pending_hard);
            }
            let (hard, content) = strip_hard_break(text);
            lines.push(content.trim_start().to_owned());
            pending_hard = hard;
            self.advance();
        }
        let joined = join_paragraph(&lines, &breaks);
        self.build_paragraph(&joined)
    }

    fn build_paragraph(&mut self, text: &str) -> Result<NodeId> {
        let inline = self.build_inline(text)?;
        self.insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(inline),
        )
    }

    fn build_image(&mut self, text: &str) -> Result<NodeId> {
        let Some((alt, url, title)) = parse_image_syntax(text) else {
            return Err(self.err("line must be exactly one image: ![alt](url)"));
        };
        if alt.trim().is_empty() {
            return Err(self.err("image alternative text is required"));
        }
        let image = ImageAttrs::new(ImageSource::ExternalUrl(url), alt, title, None, None)
            .map_err(MarkdownCodecError::InvalidDocument)?;
        let attrs = image
            .to_attrs()
            .map_err(MarkdownCodecError::InvalidDocument)?;
        self.insert(NodeKind::Image, attrs, NodeContent::Atomic)
    }

    fn build_inline(&mut self, text: &str) -> Result<InlineContent> {
        let segments = scan_inline(text).map_err(|reason| self.err(reason))?;
        if segments.is_empty() {
            return Err(self.err("inline content has no text"));
        }
        let runs = segments
            .into_iter()
            .map(|(text, marks)| TextRun::new(text, marks))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(MarkdownCodecError::InvalidDocument)?;
        InlineContent::new(runs).map_err(MarkdownCodecError::InvalidDocument)
    }
}

fn split_indent(line: &str) -> (usize, &str) {
    let mut indent = 0;
    let mut rest = line;
    loop {
        if let Some(stripped) = rest.strip_prefix(' ') {
            indent += 1;
            rest = stripped;
        } else if let Some(stripped) = rest.strip_prefix('\t') {
            indent += 4;
            rest = stripped;
        } else {
            return (indent, rest);
        }
    }
}

fn heading_marker(text: &str) -> Option<(u8, &str)> {
    let level = text.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &text[level..];
    if rest.is_empty() {
        return Some((level as u8, ""));
    }
    rest.strip_prefix(' ').map(|text| (level as u8, text))
}

fn thematic_break(text: &str) -> bool {
    matches!(text, "***" | "___")
        || (text.len() >= 3 && text.chars().all(|c| c == '-'))
        || (text.len() > 3 && text.chars().all(|c| c == '*'))
}

fn setext_underline(text: &str) -> bool {
    text.len() >= 3 && (text.chars().all(|c| c == '-') || text.chars().all(|c| c == '='))
}

fn fence_run(text: &str) -> Option<usize> {
    let run = text.chars().take_while(|&c| c == '`').count();
    (run >= 3).then_some(run)
}

fn is_closing_fence(content: &str, open_len: usize) -> bool {
    content.len() >= open_len && content.chars().all(|c| c == '`')
}

fn list_marker(text: &str) -> Option<ListMarker> {
    match text.as_bytes().first() {
        Some(b'-' | b'*' | b'+') => {
            if text.len() == 1 || text.as_bytes()[1] == b' ' {
                Some(ListMarker {
                    ordered: false,
                    width: 2,
                })
            } else {
                None
            }
        }
        Some(digit) if digit.is_ascii_digit() => {
            let digits = text.chars().take_while(char::is_ascii_digit).count();
            if digits > 9 {
                return None;
            }
            let rest = &text[digits..];
            match rest.as_bytes().first() {
                Some(b'.' | b')') if rest.len() == 1 || rest.as_bytes()[1] == b' ' => {
                    Some(ListMarker {
                        ordered: true,
                        width: digits + 2,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn interrupting_block(text: &str) -> bool {
    heading_marker(text).is_some()
        || fence_run(text).is_some()
        || text.starts_with('>')
        || list_marker(text).is_some()
        || text.starts_with("![")
}

/// Splits one raw line into `(hard_break_to_next, content)`.
fn strip_hard_break(text: &str) -> (bool, &str) {
    let trimmed = text.trim_end_matches([' ', '\t']);
    if text.len() - trimmed.len() >= 2 {
        return (true, trimmed);
    }
    let backslashes = trimmed.len() - trimmed.trim_end_matches('\\').len();
    if backslashes % 2 == 1 {
        return (true, &trimmed[..trimmed.len() - 1]);
    }
    (false, trimmed)
}

fn join_paragraph(lines: &[String], breaks: &[bool]) -> String {
    let mut joined = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            joined.push(if breaks[index - 1] { '\n' } else { ' ' });
        }
        joined.push_str(line);
    }
    joined
}

fn strip_closing_sequence(text: &str) -> String {
    let text = text.trim_end();
    let trimmed = text.trim_end_matches('#');
    if trimmed.len() < text.len() && (trimmed.is_empty() || trimmed.ends_with(' ')) {
        return trimmed.trim_end().to_owned();
    }
    text.to_owned()
}
