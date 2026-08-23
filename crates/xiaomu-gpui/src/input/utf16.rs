//! UTF-16 ↔ UTF-8 offset conversion for platform input.
//!
//! GPUI and the platform IME layer speak UTF-16 code units (matching
//! `NSTextInputClient` and friends), while Core coordinates are UTF-8 byte
//! offsets. All conversion lives here so the rest of the adapter never
//! mixes coordinate units. A UTF-16 index that lands between the halves of
//! a surrogate pair resolves to the pair's character boundary, which is
//! always a valid Core coordinate.

/// Converts a UTF-16 code-unit index into the matching UTF-8 byte offset.
///
/// Indices beyond the text clamp to its length. Indices inside a surrogate
/// pair resolve to the start of that character.
#[must_use]
pub fn utf8_offset(text: &str, utf16_index: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_index, character) in text.char_indices() {
        if utf16_count >= utf16_index {
            return byte_index;
        }
        utf16_count += character.len_utf16();
    }
    text.len()
}

/// Converts a UTF-8 byte offset into the matching UTF-16 code-unit count.
///
/// Non-boundary byte offsets round down to the character boundary at or
/// before them; offsets beyond the text clamp to its UTF-16 length.
#[must_use]
pub fn utf16_offset(text: &str, utf8_index: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_index, character) in text.char_indices() {
        if byte_index >= utf8_index {
            return utf16_count;
        }
        utf16_count += character.len_utf16();
    }
    utf16_count
}

#[cfg(test)]
mod tests {
    use super::*;

    // "a中👍": a=0..1, 中=1..4 (1 UTF-16 unit), 👍=4..8 (2 UTF-16 units).
    const MIXED: &str = "a中👍";

    #[test]
    fn ascii_ordinates_map_identity() {
        let text = "hello";
        for index in 0..=text.len() {
            assert_eq!(utf8_offset(text, index), index);
            assert_eq!(utf16_offset(text, index), index);
        }
        assert_eq!(utf8_offset(text, 99), text.len());
        assert_eq!(utf16_offset(text, 99), text.len());
    }

    #[test]
    fn bmp_characters_advance_one_unit() {
        assert_eq!(utf8_offset(MIXED, 0), 0);
        assert_eq!(utf8_offset(MIXED, 1), 1);
        assert_eq!(utf8_offset(MIXED, 2), 4); // after 中
        assert_eq!(utf16_offset(MIXED, 0), 0);
        assert_eq!(utf16_offset(MIXED, 1), 1);
        assert_eq!(utf16_offset(MIXED, 4), 2);
    }

    #[test]
    fn surrogate_pairs_resolve_to_the_character_boundary() {
        // utf16 2 and 3 both fall inside / after the high surrogate.
        assert_eq!(utf8_offset(MIXED, 2), 4);
        assert_eq!(utf8_offset(MIXED, 3), 8); // after the pair
        assert_eq!(utf8_offset(MIXED, 4), 8);
        assert_eq!(utf16_offset(MIXED, 8), 4);
    }

    #[test]
    fn round_trips_through_every_boundary() {
        let mut byte_index = 0usize;
        for character in MIXED.chars() {
            let units = utf16_offset(MIXED, byte_index);
            assert_eq!(utf8_offset(MIXED, units), byte_index);
            byte_index += character.len_utf8();
        }
        let end_units = utf16_offset(MIXED, MIXED.len());
        assert_eq!(utf8_offset(MIXED, end_units), MIXED.len());
    }

    #[test]
    fn empty_text_clamps_to_zero() {
        assert_eq!(utf8_offset("", 0), 0);
        assert_eq!(utf8_offset("", 7), 0);
        assert_eq!(utf16_offset("", 0), 0);
        assert_eq!(utf16_offset("", 7), 0);
    }
}
