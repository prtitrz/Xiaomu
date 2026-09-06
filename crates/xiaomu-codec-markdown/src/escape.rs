//! Backslash-escaping helpers shared by the writer and the reader.
//!
//! The baseline escapes a conservative punctuation set so canonical text can
//! never be re-read as Markdown structure. Escapes are valid CommonMark
//! backslash escapes for ASCII punctuation.

/// Punctuation characters escaped everywhere in ordinary inline text.
const ESCAPE_ALL: &str = "\\`*_~![]<>#+-.";

/// Escapes structural punctuation in one line of inline text.
///
/// Hard breaks are structural and never pass through this function: callers
/// split on `\n` first and re-join with the backslash form themselves.
pub(crate) fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if ESCAPE_ALL.contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Escapes a link or image destination.
///
/// Destinations containing whitespace or angle brackets are wrapped in `<>`;
/// otherwise structural characters are backslash-escaped directly.
pub(crate) fn escape_destination(destination: &str) -> String {
    let wrap = destination.is_empty()
        || destination
            .chars()
            .any(|character| matches!(character, ' ' | '<' | '>'));
    let escaped = escape_into(destination, "\\<>()");
    if wrap {
        format!("<{escaped}>")
    } else {
        escaped
    }
}

/// Escapes a quoted link or image title.
pub(crate) fn escape_title(title: &str) -> String {
    escape_into(title, "\\\"")
}

fn escape_into(text: &str, set: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if set.contains(character) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// Chooses a code-span fence longer than any backtick run in `content`.
pub(crate) fn code_span_fence(content: &str) -> String {
    let longest = longest_backtick_run(content);
    "`".repeat(longest.max(1) + 1)
}

/// Chooses a fenced code block fence longer than any backtick run in the body.
pub(crate) fn code_block_fence(content: &str) -> String {
    "`".repeat(longest_backtick_run(content).max(2) + 1)
}

fn longest_backtick_run(content: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for character in content.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_structure_everywhere() {
        assert_eq!(escape_text("a*b_c"), "a\\*b\\_c");
        assert_eq!(escape_text("# heading"), "\\# heading");
        assert_eq!(escape_text("plain"), "plain");
    }

    #[test]
    fn destinations_wrap_only_when_needed() {
        assert_eq!(escape_destination("a(b)"), "a\\(b\\)");
        assert_eq!(escape_destination("a b"), "<a b>");
        assert_eq!(escape_destination("plain"), "plain");
        assert_eq!(escape_destination(""), "<>");
    }

    #[test]
    fn titles_escape_quotes_and_backslashes() {
        assert_eq!(escape_title("a \"b\" \\ c"), "a \\\"b\\\" \\\\ c");
    }

    #[test]
    fn fences_grow_with_content_backticks() {
        assert_eq!(code_span_fence("plain"), "``");
        assert_eq!(code_span_fence("a `` b"), "```");
        assert_eq!(code_block_fence("plain"), "```");
        assert_eq!(code_block_fence("```\ncode\n```"), "````");
    }
}
