mod support;
use support::{document, run};
use xiaomu_codec_markdown::{MarkdownCodecError, from_markdown, to_markdown};
use xiaomu_core::document::NodeKind;

fn limited(result: Result<impl std::fmt::Debug, MarkdownCodecError>, resource: &str) {
    match result {
        Err(MarkdownCodecError::ResourceLimit {
            resource: actual, ..
        }) => assert_eq!(actual, resource),
        other => panic!("expected {resource} limit, got {other:?}"),
    }
}

#[test]
fn source_nodes_and_inline_work_have_shared_limits() {
    limited(from_markdown(&"x".repeat(1024 * 1024 + 1)), "source bytes");
    limited(from_markdown(&"x\n\n".repeat(20_000)), "nodes");
    limited(from_markdown(&"<".repeat(10_000)), "work");
    limited(from_markdown(&format!("{}x", "[".repeat(10_000))), "work");
    limited(
        from_markdown(&format!(
            "[{}]({})",
            "*x*".repeat(1_000),
            "x".repeat(20_000)
        )),
        "work",
    );
}

#[test]
fn quote_list_and_export_depth_are_bounded() {
    let ok = format!("{}text\n", "> ".repeat(63));
    let doc = from_markdown(&ok).unwrap();
    support::assert_semantics(&doc, &from_markdown(&to_markdown(&doc).unwrap()).unwrap());
    limited(
        from_markdown(&format!("{}text\n", "> ".repeat(64))),
        "nesting",
    );
    let list: String = (0..40)
        .map(|i| format!("{}- text\n", "  ".repeat(i)))
        .collect();
    limited(from_markdown(&list), "nesting");
    limited(
        from_markdown(&format!("{}{}", "> ".repeat(63), "- text\n")),
        "nesting",
    );
    let doc = document(
        NodeKind::Paragraph,
        vec![run("x", [])],
        &vec![NodeKind::Quote; 8192],
    );
    limited(to_markdown(&doc), "nesting");
    let big = document(
        NodeKind::Paragraph,
        vec![run(&"x".repeat(1024 * 1024), [])],
        &[],
    );
    limited(to_markdown(&big), "output estimate");
}

#[test]
fn deeply_nested_import_returns_error_in_a_subprocess() {
    const PROBE: &str = "XIAOMU_MARKDOWN_DEPTH_PROBE";
    if std::env::var_os(PROBE).is_some() {
        limited(
            from_markdown(&format!("{}text\n", "> ".repeat(8192))),
            "nesting",
        );
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "deeply_nested_import_returns_error_in_a_subprocess",
            "--nocapture",
        ])
        .env(PROBE, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn malformed_short_markers_and_destinations_never_panic() {
    for source in [
        "-",
        "*",
        "+",
        "1.",
        "12)",
        "[x](\\",
        "[x](<\\",
        "[x](url \"\\",
    ] {
        let _ = from_markdown(source);
    }
    // Tabs are indentation columns, never offsets into the short UTF-8 line.
    let doc = from_markdown("\t```\n\t树\n\t```\n").unwrap();
    assert!(to_markdown(&doc).unwrap().contains("树\n"));
    let doc = from_markdown("10. ten\n1. one\n").unwrap();
    assert_eq!(to_markdown(&doc).unwrap(), "1. ten\n2. one\n");
}

#[test]
fn bounded_deterministic_input_corpus_does_not_panic() {
    let alphabet = [
        "树", "🐾", "\\", "\t", "\n", "`", "*", "[", "]", "(", ")", "<", ">", "-", "1.", " ",
    ];
    let mut state = 0x5eed_u64;
    for size in 1..=256 {
        let source: String = (0..size)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                alphabet[(state as usize) % alphabet.len()]
            })
            .collect();
        if let Ok(doc) = from_markdown(&source)
            && let Ok(markdown) = to_markdown(&doc)
        {
            let _ = from_markdown(&markdown).expect("successful export must remain readable");
        }
    }
}
