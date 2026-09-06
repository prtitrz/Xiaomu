//! Mark-set text encoding for the harness fixture format.

use xiaomu_core::document::{LinkMark, Mark, MarkSet};
use xiaomu_runtime::persistence::PersistenceError;

pub(crate) fn encode_marks(marks: &MarkSet) -> Result<String, PersistenceError> {
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

pub(crate) fn parse_marks(spec: &str) -> Result<MarkSet, String> {
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
