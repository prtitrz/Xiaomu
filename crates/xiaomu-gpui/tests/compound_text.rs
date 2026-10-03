//! Pure, synthetic decoder vectors. These are NOT captured session payloads.
use xim_ctext::compound_text_to_utf8;

#[test]
fn ascii_and_empty_preserve_exact_text() {
    assert_eq!(
        compound_text_to_utf8(b"plain ASCII 123").unwrap(),
        "plain ASCII 123"
    );
    assert_eq!(compound_text_to_utf8(b"").unwrap(), "");
}

#[test]
fn gb2312_chinese_commit_is_supported() {
    let bytes = [0x1b, 0x24, 0x28, 0x41, 0x44, 0x63, 0x3a, 0x43];
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "你好");
}

#[test]
fn japanese_compound_text_is_supported() {
    let bytes = [27, 36, 40, 66, 69, 108, 53, 126];
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "東京");
}

#[test]
fn korean_compound_text_is_supported() {
    let bytes = [
        0x1b, 0x24, 0x28, 0x43, 0x33, 0x4d, 0x43, 0x56, 0x30, 0x6d, 0x3e, 0x5f,
    ];
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "넌최고야");
}

#[test]
fn mixed_japanese_and_gb2312_segments_preserve_all_characters() {
    let bytes = [
        0x1b, 0x24, 0x28, 0x42, 0x5f, 0x5a, 0x53, 0x28, 0x1b, 0x24, 0x28, 0x41, 0x44, 0x63,
    ];
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "炸哦你");
}

#[test]
fn ascii_plus_utf8_extension_preserves_unicode_with_or_without_closing_escape() {
    let mut bytes = b"prefix \x1b%G".to_vec();
    bytes.extend_from_slice("你好🙂".as_bytes());
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "prefix 你好🙂");
    bytes.extend_from_slice(b"\x1b%@");
    assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "prefix 你好🙂");
}

#[test]
fn unescaped_ascii_escape_like_prefixes_are_literal() {
    for text in [
        "", "%", "%@", "%G", "%Ghello", "$(", "$(Aabc", "(Bhello", "(Jworld", "-Ahello", "$value",
    ] {
        assert_eq!(
            compound_text_to_utf8(text.as_bytes()).unwrap(),
            text,
            "{text:?}"
        );
    }
    // All printable two-byte prefixes, not merely the ordinary word "plain".
    for a in b' '..=b'~' {
        for b in b' '..=b'~' {
            let bytes = [a, b, b' ', b'x'];
            assert_eq!(compound_text_to_utf8(&bytes).unwrap().as_bytes(), bytes);
        }
    }
}

#[test]
fn literal_prefix_then_real_escape_and_ascii_suffix_preserve_every_character() {
    for prefix in ["%@", "%Ghello", "$(", "(B", "(J", "-A", "ordinary "] {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(b"\x1b%G");
        bytes.extend_from_slice("你好🙂".as_bytes());
        bytes.extend_from_slice(b"\x1b%@ tail");
        assert_eq!(
            compound_text_to_utf8(&bytes).unwrap(),
            format!("{prefix}你好🙂 tail")
        );
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(&[0x1b, 0x24, 0x28, 0x41, 0x44, 0x63, 0x3a, 0x43]);
        bytes.extend_from_slice(b"\x1b(B tail");
        assert_eq!(
            compound_text_to_utf8(&bytes).unwrap(),
            format!("{prefix}你好 tail")
        );
    }
}

#[test]
fn real_charset_return_markers_are_not_literal_prefixes() {
    for marker in [b"\x1b(B".as_slice(), b"\x1b(J"] {
        let mut bytes = vec![27, 36, 40, 66, 69, 108, 53, 126]; // 東京
        bytes.extend_from_slice(marker);
        bytes.extend_from_slice(b" ASCII");
        assert_eq!(compound_text_to_utf8(&bytes).unwrap(), "東京 ASCII");
    }
}

#[test]
fn mixed_japanese_chinese_korean_and_ascii_segments_round_trip() {
    let mut bytes = b"%@ prefix ".to_vec();
    bytes.extend_from_slice(&[27, 36, 40, 66, 69, 108, 53, 126]); // 東京
    bytes.extend_from_slice(&[0x1b, 0x24, 0x28, 0x41, 0x44, 0x63, 0x3a, 0x43]); // 你好
    bytes.extend_from_slice(&[
        0x1b, 0x24, 0x28, 0x43, 0x33, 0x4d, 0x43, 0x56, 0x30, 0x6d, 0x3e, 0x5f,
    ]); // 넌최고야
    bytes.extend_from_slice(b"\x1b(B suffix $(");
    assert_eq!(
        compound_text_to_utf8(&bytes).unwrap(),
        "%@ prefix 東京你好넌최고야 suffix $("
    );
}

#[test]
fn high_bit_multibyte_payload_is_a_stable_error_not_an_overflow() {
    for charset in *b"AC" {
        for byte in 0x80..=0xff {
            for invalid_pair in [[byte, b'!'], [b'!', byte]] {
                for has_valid_prefix in [false, true] {
                    let mut bytes = vec![0x1b, b'$', b'(', charset];
                    if has_valid_prefix {
                        bytes.extend_from_slice(b"!!");
                    }
                    bytes.extend_from_slice(&invalid_pair);
                    let outcome = std::panic::catch_unwind(|| compound_text_to_utf8(&bytes));
                    assert!(matches!(
                        outcome,
                        Ok(Err(xim_ctext::DecodeError::InvalidEncoding))
                    ));
                }
            }
        }
    }
}

#[test]
fn selected_malformed_sequences_return_errors_without_panicking() {
    for bytes in [b"\x1b$(".as_slice(), b"\x1b$(?x", b"\x1b%G\xff"] {
        assert!(matches!(
            std::panic::catch_unwind(|| compound_text_to_utf8(bytes)),
            Ok(Err(_))
        ));
    }
}

#[test]
fn utf8_scalar_boundaries_survive_extension_and_ascii_suffix() {
    let text = "\u{007f}\u{0080}\u{07ff}\u{0800}\u{ffff}\u{10000}\u{10ffff}";
    let mut bytes = b"%@ literal \x1b%G".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b%@ suffix");
    assert_eq!(
        compound_text_to_utf8(&bytes).unwrap(),
        format!("%@ literal {text} suffix")
    );
}

#[test]
fn default_charset_prefix_and_reset_suffix_preserve_latin1_boundaries() {
    let mut bytes = vec![0x80, 0xff, 0x1b, b'%', b'G'];
    bytes.extend_from_slice("中".as_bytes());
    bytes.extend_from_slice(&[0x1b, b'%', b'@', 0x80, 0xff]);
    assert_eq!(
        compound_text_to_utf8(&bytes).unwrap(),
        "\u{0080}ÿ中\u{0080}ÿ"
    );
}

#[test]
fn malformed_utf8_extension_is_rejected_without_partial_success() {
    for invalid in [
        &[0xc2][..],                   // truncated
        &[0xe0, 0x80, 0x80][..],       // overlong
        &[0xed, 0xa0, 0x80][..],       // surrogate
        &[0xf4, 0x90, 0x80, 0x80][..], // above U+10FFFF
    ] {
        let mut bytes = b"valid prefix \x1b%G".to_vec();
        bytes.extend_from_slice(invalid);
        let outcome = std::panic::catch_unwind(|| compound_text_to_utf8(&bytes));
        assert!(matches!(
            outcome,
            Ok(Err(xim_ctext::DecodeError::Utf8Error(_)))
        ));
    }
}
