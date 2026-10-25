//! One-pass inline scanner with segment retro-marking.
//!
//! Marks are applied retroactively to sealed segment ranges, so emphasis and
//! link spans need no backtracking. Tokens that never close are spliced back
//! into the text as literals, matching the CommonMark fallback.

use xiaomu_core::document::{LinkMark, Mark, MarkSet};

use crate::limits::Budget;
use crate::syntax::parse_destination;
use crate::{MarkdownCodecError, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
enum EmphToken {
    Bold,
    Italic,
    Strike,
}

impl EmphToken {
    fn mark(self) -> Mark {
        match self {
            Self::Bold => Mark::Bold,
            Self::Italic => Mark::Italic,
            Self::Strike => Mark::Strike,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italic => "*",
            Self::Strike => "~~",
        }
    }
}

struct Segment {
    text: String,
    marks: Vec<Mark>,
}

struct EmphOpen {
    token: EmphToken,
    seg_start: usize,
}

#[derive(Clone, Copy)]
struct LinkFrame {
    seg_start: usize,
}

/// Scans one line-joined paragraph into `(text, marks)` segments.
pub(crate) fn scan_inline(
    text: &str,
    line: usize,
    budget: &mut Budget,
) -> Result<Vec<(String, MarkSet)>> {
    let mut scanner = InlineScanner {
        src: text,
        line,
        budget,
        pos: 0,
        segments: Vec::new(),
        opens: Vec::new(),
        links: Vec::new(),
    };
    scanner.run()?;
    scanner.finish()
}

struct InlineScanner<'a> {
    src: &'a str,
    line: usize,
    budget: &'a mut Budget,
    pos: usize,
    segments: Vec<Segment>,
    opens: Vec<EmphOpen>,
    links: Vec<LinkFrame>,
}

impl<'a> InlineScanner<'a> {
    fn err(&self, reason: &str) -> MarkdownCodecError {
        MarkdownCodecError::InvalidMarkdown {
            line: self.line,
            reason: reason.to_owned(),
        }
    }

    /// Starts a fresh segment so retro-applied mark ranges stay precise.
    fn seal(&mut self) {
        if self
            .segments
            .last()
            .is_none_or(|segment| !segment.text.is_empty())
        {
            self.segments.push(Segment {
                text: String::new(),
                marks: Vec::new(),
            });
        }
    }

    fn push_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut() {
            last.text.push_str(text);
            return;
        }
        self.segments.push(Segment {
            text: text.to_owned(),
            marks: Vec::new(),
        });
    }

    fn run(&mut self) -> Result<()> {
        while self.pos < self.src.len() {
            self.budget.work(1)?;
            match self.src.as_bytes()[self.pos] {
                b'\\'
                    if self.src[self.pos + 1..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_punctuation()) =>
                {
                    let ch = self.src[self.pos + 1..]
                        .chars()
                        .next()
                        .expect("punctuation");
                    self.push_str(&ch.to_string());
                    self.pos += 1 + ch.len_utf8();
                }
                b'\\' => {
                    self.push_str("\\");
                    self.pos += 1;
                }
                b'`' => self.code_span()?,
                b'*' | b'~' => self.emphasis()?,
                b'[' => {
                    self.seal();
                    self.links.push(LinkFrame {
                        seg_start: self.segments.len() - 1,
                    });
                    self.pos += 1;
                }
                b']' => self.link_close()?,
                b'!' if self.src.as_bytes().get(self.pos + 1) == Some(&b'[') => {
                    return Err(
                        self.err("inline image syntax is not supported in baseline Markdown")
                    );
                }
                b'<' => self.autolink()?,
                _ => {
                    let ch = self.src[self.pos..].chars().next().expect("non-empty");
                    self.push_str(&ch.to_string());
                    self.pos += ch.len_utf8();
                }
            }
        }
        Ok(())
    }

    fn code_span(&mut self) -> Result<()> {
        self.budget.work(self.src.len() - self.pos)?;
        let open_len = self.src[self.pos..]
            .chars()
            .take_while(|&c| c == '`')
            .count();
        self.seal();
        let mut index = self.pos + open_len;
        let mut found = None;
        while index < self.src.len() {
            if self.src.as_bytes()[index] == b'`' {
                let run = self.src[index..].chars().take_while(|&c| c == '`').count();
                if run == open_len {
                    found = Some(index);
                    break;
                }
                index += run;
                continue;
            }
            index += 1;
        }
        let Some(end) = found else {
            self.push_str(&"`".repeat(open_len));
            self.pos += open_len;
            return Ok(());
        };
        let mut content = &self.src[self.pos + open_len..end];
        if content.starts_with(' ') && content.ends_with(' ') && content.chars().any(|c| c != ' ') {
            content = &content[1..content.len() - 1];
        }
        self.segments.push(Segment {
            text: content.to_owned(),
            marks: vec![Mark::Code],
        });
        self.seal();
        self.pos = end + open_len;
        Ok(())
    }

    fn emphasis(&mut self) -> Result<()> {
        self.budget.work(self.opens.len() + self.segments.len())?;
        let (token, width) = match self.src.as_bytes()[self.pos] {
            b'*' if self.src.as_bytes().get(self.pos + 1) == Some(&b'*') => (EmphToken::Bold, 2),
            b'*' => (EmphToken::Italic, 1),
            _ if self.src.as_bytes().get(self.pos + 1) == Some(&b'~') => (EmphToken::Strike, 2),
            _ => {
                self.push_str("~");
                self.pos += 1;
                return Ok(());
            }
        };

        let closes = self.pos > 0
            && !self.src.as_bytes()[self.pos - 1].is_ascii_whitespace()
            && self.opens.iter().any(|open| open.token == token);
        if closes {
            let index = self
                .opens
                .iter()
                .rposition(|open| open.token == token)
                .expect("checked above");
            let open = self.opens.remove(index);
            self.seal();
            let end = self.segments.len() - 1;
            self.budget
                .work((end - open.seg_start).saturating_mul(std::mem::size_of::<Mark>()))?;
            for segment in &mut self.segments[open.seg_start..end] {
                segment.marks.push(token.mark());
            }
            self.pos += width;
            return Ok(());
        }

        let opens = self
            .src
            .as_bytes()
            .get(self.pos + width)
            .is_some_and(|&b| !b.is_ascii_whitespace());
        if opens {
            self.seal();
            self.opens.push(EmphOpen {
                token,
                seg_start: self.segments.len() - 1,
            });
            self.pos += width;
            return Ok(());
        }
        self.push_str(token.text());
        self.pos += width;
        Ok(())
    }

    fn link_close(&mut self) -> Result<()> {
        let Some(frame) = self.links.last().copied() else {
            self.push_str("]");
            self.pos += 1;
            return Ok(());
        };
        let rest = &self.src[self.pos + 1..];
        let Some(after) = rest.strip_prefix('(') else {
            self.links.pop();
            self.push_str("]");
            self.pos += 1;
            return Ok(());
        };
        self.budget.work(after.len() + self.segments.len())?;
        match parse_destination(after) {
            Some((url, title, _consumed)) => {
                self.links.pop();
                self.seal();
                let end = self.segments.len() - 1;
                if self.segments[frame.seg_start..end]
                    .iter()
                    .all(|s| s.text.is_empty())
                {
                    return Err(self.err("link text must not be empty"));
                }
                let clone_bytes = url
                    .len()
                    .saturating_add(title.as_ref().map_or(0, String::len))
                    .saturating_add(std::mem::size_of::<Mark>());
                self.budget
                    .work((end - frame.seg_start).saturating_mul(clone_bytes))?;
                let link = LinkMark::new(url, title);
                for segment in &mut self.segments[frame.seg_start..end] {
                    segment.marks.push(Mark::Link(link.clone()));
                }
                self.pos += 2 + _consumed;
                Ok(())
            }
            None => {
                self.links.pop();
                self.push_str("]");
                self.pos += 1;
                Ok(())
            }
        }
    }

    fn autolink(&mut self) -> Result<()> {
        self.budget.work(self.src.len() - self.pos)?;
        let rest = &self.src[self.pos + 1..];
        let Some(close) = rest.find('>') else {
            self.push_str("<");
            self.pos += 1;
            return Ok(());
        };
        let uri = &rest[..close];
        let scheme = uri.split_once(':').filter(|(scheme, _)| {
            !scheme.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        });
        if scheme.is_none() {
            self.push_str("<");
            self.pos += 1;
            return Ok(());
        }
        self.seal();
        self.segments.push(Segment {
            text: uri.to_owned(),
            marks: vec![Mark::Link(LinkMark::new(uri, None))],
        });
        self.seal();
        self.pos += close + 2;
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<(String, MarkSet)>> {
        let mut pending: Vec<(usize, &'static str)> = self
            .links
            .drain(..)
            .map(|frame| (frame.seg_start, "["))
            .chain(
                self.opens
                    .drain(..)
                    .map(|open| (open.seg_start, open.token.text())),
            )
            .collect();
        pending.sort_by_key(|(seg_start, _)| std::cmp::Reverse(*seg_start));
        for (seg_start, literal) in pending {
            self.budget
                .work(self.segments.get(seg_start).map_or(0, |s| s.text.len()))?;
            self.splice_literal(seg_start, literal);
        }

        let line = self.line;
        self.segments
            .into_iter()
            .filter(|segment| !segment.text.is_empty())
            .map(|segment| {
                MarkSet::new(segment.marks)
                    .map(|marks| (segment.text, marks))
                    .map_err(|_| MarkdownCodecError::InvalidMarkdown {
                        line,
                        reason: "conflicting marks on one text run".to_owned(),
                    })
            })
            .collect()
    }

    fn splice_literal(&mut self, seg_start: usize, literal: &str) {
        if seg_start < self.segments.len() {
            self.segments[seg_start].text.insert_str(0, literal);
        } else if let Some(last) = self.segments.last_mut() {
            last.text.push_str(literal);
        } else {
            self.segments.push(Segment {
                text: literal.to_owned(),
                marks: Vec::new(),
            });
        }
    }
}
