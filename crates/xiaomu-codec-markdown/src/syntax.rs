//! Shared link/image destination syntax helpers.

/// Parses one full-line image `![alt](url "title")`.
pub(crate) fn parse_image_syntax(text: &str) -> Option<(String, String, Option<String>)> {
    let rest = text.strip_prefix("![")?;
    let close = find_unescaped(rest, b']')?;
    let alt = unescape(&rest[..close]);
    let after = rest[close + 1..].strip_prefix('(')?;
    let (url, title, consumed) = parse_destination(after)?;
    if consumed != after.len() {
        return None;
    }
    Some((alt, url, title))
}

fn find_unescaped(text: &str, needle: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] == needle {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Reads a link/image destination with optional `"title"`.
///
/// `text` starts right after the opening `(`. Returns the destination, the
/// optional title, and the consumed length including the closing `)`.
pub(crate) fn parse_destination(text: &str) -> Option<(String, Option<String>, usize)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    let destination;
    if bytes.first() == Some(&b'<') {
        index = 1;
        let start = index;
        while index < bytes.len() && bytes[index] != b'>' {
            if bytes[index] == b'\\' {
                index += 1;
            }
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        destination = unescape(&text[start..index]);
        index += 1;
    } else {
        let start = index;
        while index < bytes.len() && bytes[index] != b')' {
            match bytes[index] {
                b'\\' => index += 1,
                b' ' | b'\t' | b'\n' => break,
                b'(' => return None,
                _ => {}
            }
            index += 1;
        }
        destination = unescape(&text[start..index]);
    }

    while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
        index += 1;
    }
    let mut title = None;
    if index < bytes.len() && bytes[index] == b'"' {
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index] != b'"' {
            if bytes[index] == b'\\' {
                index += 1;
            }
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        title = Some(unescape(&text[start..index]));
        index += 1;
        while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
            index += 1;
        }
    }
    if index >= bytes.len() || bytes[index] != b')' {
        return None;
    }
    Some((destination, title, index + 1))
}

fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 1 < bytes.len()
            && bytes[index + 1].is_ascii_punctuation()
        {
            out.push(bytes[index + 1] as char);
            index += 2;
        } else {
            let ch = text[index..].chars().next().expect("non-empty");
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}
