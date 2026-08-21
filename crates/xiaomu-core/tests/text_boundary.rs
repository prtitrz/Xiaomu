use xiaomu_core::{
    Error,
    text::{TextBuffer, TextOffset, TextRange},
};

#[test]
fn empty_buffer_has_stable_zero_coordinates() {
    let buffer = TextBuffer::new();

    assert!(buffer.is_empty());
    assert_eq!(buffer.len_bytes(), 0);
    assert_eq!(buffer.offset_at(0).unwrap(), TextOffset::ZERO);
    assert_eq!(buffer.end_offset(), TextOffset::ZERO);
}

#[test]
fn utf8_offsets_match_rust_character_boundaries_for_regression_fixtures() {
    let fixtures = [
        "plain ASCII",
        "中文",
        "中文 and Latin 123",
        "🙂🚀",
        "e\u{301} cafe\u{301}",
        "abc אבג 123",
    ];

    for fixture in fixtures {
        let buffer = TextBuffer::from(fixture);

        for byte_index in 0..=fixture.len() {
            assert_eq!(
                buffer.offset_at(byte_index).is_ok(),
                fixture.is_char_boundary(byte_index),
                "fixture {fixture:?}, byte index {byte_index}"
            );
        }
    }
}

#[test]
fn chinese_offsets_reject_interior_utf8_bytes() {
    let buffer = TextBuffer::from("中文");

    assert_eq!(buffer.offset_at(0).unwrap().as_usize(), 0);
    assert_eq!(buffer.offset_at(3).unwrap().as_usize(), 3);
    assert_eq!(buffer.offset_at(6).unwrap().as_usize(), 6);
    assert_eq!(
        buffer.offset_at(1),
        Err(Error::InvalidTextBoundary { offset: 1 })
    );
    assert_eq!(
        buffer.offset_at(5),
        Err(Error::InvalidTextBoundary { offset: 5 })
    );
}

#[test]
fn emoji_offsets_reject_interior_utf8_bytes() {
    let buffer = TextBuffer::from("a🙂b");

    assert!(buffer.offset_at(0).is_ok());
    assert!(buffer.offset_at(1).is_ok());
    assert!(buffer.offset_at(5).is_ok());
    assert!(buffer.offset_at(6).is_ok());

    for byte_index in 2..5 {
        assert_eq!(
            buffer.offset_at(byte_index),
            Err(Error::InvalidTextBoundary { offset: byte_index })
        );
    }
}

#[test]
fn combining_marks_are_scalar_boundaries_not_grapheme_boundaries() {
    let text = "e\u{301}";
    let buffer = TextBuffer::from(text);

    let after_base = buffer.offset_at(1).unwrap();
    let end = buffer.end_offset();
    let combining_mark = buffer.range(after_base, end).unwrap();

    assert_eq!(buffer.slice(combining_mark).unwrap(), "\u{301}");
    assert_eq!(
        buffer.offset_at(2),
        Err(Error::InvalidTextBoundary { offset: 2 })
    );
}

#[test]
fn bidi_text_uses_logical_utf8_coordinates() {
    let text = "abc אבג 123";
    let buffer = TextBuffer::from(text);

    for (byte_index, _) in text.char_indices() {
        assert!(buffer.offset_at(byte_index).is_ok());
    }
    assert!(buffer.offset_at(text.len()).is_ok());
}

#[test]
fn ranges_are_half_open_and_slice_without_panicking() {
    let buffer = TextBuffer::from("甲乙🙂abc");
    let start = buffer.offset_at(3).unwrap();
    let end = buffer.offset_at(10).unwrap();
    let range = buffer.range(start, end).unwrap();

    assert_eq!(buffer.slice(range).unwrap(), "乙🙂");
    assert_eq!(range.start(), start);
    assert_eq!(range.end(), end);
    assert_eq!(range.len_bytes(), 7);
    assert!(!range.is_empty());
}

#[test]
fn reversed_ranges_are_rejected() {
    let buffer = TextBuffer::from("abc");
    let start = buffer.offset_at(3).unwrap();
    let end = buffer.offset_at(1).unwrap();

    assert_eq!(
        TextRange::new(start, end),
        Err(Error::InvalidTextRange { start: 3, end: 1 })
    );
}

#[test]
fn out_of_bounds_offsets_return_controlled_errors() {
    let buffer = TextBuffer::from("中");

    assert_eq!(
        buffer.offset_at(4),
        Err(Error::TextOutOfBounds { offset: 4, len: 3 })
    );
}

#[test]
fn stale_offsets_are_revalidated_against_the_target_buffer() {
    let original = TextBuffer::from("abc🙂");
    let stale_end = original.end_offset();
    let shorter = TextBuffer::from("a");

    assert_eq!(
        shorter.validate_offset(stale_end),
        Err(Error::TextOutOfBounds { offset: 7, len: 1 })
    );
}

#[test]
fn replacement_returns_a_new_buffer_and_preserves_the_original() {
    let original = TextBuffer::from("你好🙂 world");
    let start = original.offset_at(3).unwrap();
    let end = original.offset_at(10).unwrap();
    let range = original.range(start, end).unwrap();

    let replaced = original.replaced(range, "晓木").unwrap();

    assert_eq!(original.as_str(), "你好🙂 world");
    assert_eq!(replaced.as_str(), "你晓木 world");
}

#[test]
fn insertion_and_deletion_use_empty_and_non_empty_ranges() {
    let original = TextBuffer::from("ac");
    let at = original.offset_at(1).unwrap();
    let insertion = TextRange::empty(at);
    let inserted = original.replaced(insertion, "中").unwrap();

    assert_eq!(inserted.as_str(), "a中c");

    let start = inserted.offset_at(1).unwrap();
    let end = inserted.offset_at(4).unwrap();
    let deletion = inserted.range(start, end).unwrap();
    let deleted = inserted.replaced(deletion, "").unwrap();

    assert_eq!(deleted.as_str(), "ac");
}

#[test]
fn every_invalid_byte_boundary_is_a_controlled_error() {
    let fixtures = ["中文🙂", "e\u{301}🙂", "אבג🙂"];

    for fixture in fixtures {
        let buffer = TextBuffer::from(fixture);
        for byte_index in 0..=fixture.len() {
            let result = buffer.offset_at(byte_index);
            if fixture.is_char_boundary(byte_index) {
                assert!(result.is_ok());
            } else {
                assert_eq!(result, Err(Error::InvalidTextBoundary { offset: byte_index }));
            }
        }
    }
}
