use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use xiaomu_core::{
    document::{
        AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet, NodeAttrs,
        NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    text::TextBuffer,
};
use xiaomu_runtime::reading::{
    AtomText, ReadingBudget, ReadingCase, ReadingError, ReadingProjection, ReadingProjectionLimits,
    ReadingProjectionOptions, ReadingSearchLimits, UNICODE_SIMPLE_FOLD_VERSION,
};

fn document(text: &str) -> (XiaomuDocument, NodeId) {
    mixed(text, &[])
}

fn mixed(text: &str, atoms: &[(usize, bool)]) -> (XiaomuDocument, NodeId) {
    let mut b = NodeStoreBuilder::new();
    let buffer = TextBuffer::from_string(text.into());
    let mut placements = Vec::new();
    for &(offset, hard_break) in atoms {
        let (kind, content) = if hard_break {
            (AtomKind::hard_break(), InlineAtomContent::hard_break())
        } else {
            (
                AtomKind::new("reference").unwrap(),
                InlineAtomContent::new("not searchable fallback").unwrap(),
            )
        };
        let atom = b
            .insert(
                NodeKind::InlineAtom(kind),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(content),
            )
            .unwrap();
        placements.push(InlineAtomPlacement::new(
            atom,
            buffer.offset_at(offset).unwrap(),
        ));
    }
    let runs = if text.is_empty() {
        vec![]
    } else {
        vec![TextRun::new(text, MarkSet::empty()).unwrap()]
    };
    let block = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([block]),
        )
        .unwrap();
    (XiaomuDocument::new(root, b.finish()).unwrap(), block)
}

fn projection(doc: &XiaomuDocument) -> ReadingProjection {
    ReadingProjection::build(
        doc,
        ReadingProjectionOptions {
            hard_break: AtomText::Character('\n'),
            other_atoms: AtomText::Character('\u{fffc}'),
        },
        ReadingProjectionLimits::default(),
    )
    .unwrap()
}

fn ranges(haystack: &str, needle: &str, case: ReadingCase) -> Vec<Range<usize>> {
    let (doc, _) = document(haystack);
    let p = projection(&doc);
    p.find_literal(needle, case, ReadingSearchLimits::default())
        .unwrap()
        .matches()
        .iter()
        .map(|m| {
            m.start().validate(&doc).unwrap();
            m.end().validate(&doc).unwrap();
            m.projected_range()
        })
        .collect()
}

#[test]
fn original_unicode_cases_preserve_source_widths_and_no_normalization() {
    assert_eq!(UNICODE_SIMPLE_FOLD_VERSION, "17.0.0");
    for (text, query, expected) in [
        ("İiıI", "i", vec![2..3, 5..6]),
        ("ſsS", "s", vec![0..2, 2..3, 3..4]),
        ("KkK", "k", vec![0..3, 3..4, 4..5]),
        ("Σσς", "σ", vec![0..2, 2..4, 4..6]),
        ("ßẞss", "ß", vec![0..2, 2..5]),
        ("ßẞss", "ss", std::iter::once(5..7).collect()),
        ("ée\u{301}", "é", std::iter::once(0..2).collect()),
        ("ée\u{301}", "e\u{301}", std::iter::once(2..5).collect()),
        ("\u{a7ce}\u{a7cf}", "\u{a7cf}", vec![0..3, 3..6]),
        ("\u{16ea0}\u{16ebb}", "\u{16ebb}", vec![0..4, 4..8]),
    ] {
        assert_eq!(
            ranges(text, query, ReadingCase::UnicodeSimple),
            expected,
            "{text:?}/{query:?}"
        );
    }
    assert_eq!(ranges("ſsS", "s", ReadingCase::Exact), vec![2..3]);
}

#[test]
fn literal_nonoverlapping_search_never_interprets_query_syntax() {
    assert_eq!(ranges("aaaaa", "aa", ReadingCase::Exact), vec![0..2, 2..4]);
    assert_eq!(
        ranges("[a.*]+? [a.*]+?", "[a.*]+?", ReadingCase::UnicodeSimple),
        vec![0..7, 8..15]
    );
    assert_eq!(ranges("a\0b\0", "\0", ReadingCase::Exact), vec![1..2, 3..4]);
    assert!(ranges("any text", "", ReadingCase::Exact).is_empty());
    assert_eq!(
        ranges("ababcabcabababd", "ababd", ReadingCase::Exact),
        vec![10..15]
    );
}

#[test]
fn matches_cross_marks_but_never_cross_blocks() {
    let mut b = NodeStoreBuilder::new();
    let first = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("a", MarkSet::empty()).unwrap(),
                    TextRun::new("b", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                ])
                .unwrap(),
            ),
        )
        .unwrap();
    let second = b
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("c", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = b
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let doc = XiaomuDocument::new(root, b.finish()).unwrap();
    let p = projection(&doc);
    assert_eq!(
        p.find_literal("ab", ReadingCase::Exact, Default::default())
            .unwrap()
            .matches()
            .len(),
        1
    );
    for query in ["bc", "b\nc", "abc"] {
        assert!(
            p.find_literal(query, ReadingCase::Exact, Default::default())
                .unwrap()
                .matches()
                .is_empty()
        );
    }
}

#[test]
fn typed_atoms_are_searchable_but_never_disguised_as_text() {
    let (doc, node) = mixed("ab", &[(1, true), (1, false)]);
    let p = projection(&doc);
    assert!(
        p.find_literal("ab", ReadingCase::Exact, Default::default())
            .unwrap()
            .matches()
            .is_empty()
    );
    let found = p
        .find_literal("a\n\u{fffc}b", ReadingCase::Exact, Default::default())
        .unwrap();
    let m = &found.matches()[0];
    assert!(m.contains_atoms());
    assert_eq!(
        (
            m.start().text_offset().as_usize(),
            m.end().text_offset().as_usize()
        ),
        (0, 2)
    );
    let found = p
        .find_literal("b", ReadingCase::Exact, Default::default())
        .unwrap();
    let m = &found.matches()[0];
    assert_eq!(m.start().node_id(), node);
    assert_eq!(
        (m.start().text_offset().as_usize(), m.start().atom_index()),
        (1, 2)
    );
    assert!(!m.contains_atoms());
    let found = p
        .find_literal("\n", ReadingCase::Exact, Default::default())
        .unwrap();
    assert_eq!(found.matches()[0].start().atom_index(), 0);
    assert_eq!(found.matches()[0].end().atom_index(), 1);
    let (doc, _) = document("a\nb");
    let p = projection(&doc);
    assert!(
        !p.find_literal("a\nb", ReadingCase::Exact, Default::default())
            .unwrap()
            .matches()[0]
            .contains_atoms()
    );
}

#[test]
fn omitted_atoms_still_block_source_text_only_replacement_assumptions() {
    let (doc, _) = mixed("ab", &[(0, true), (1, true), (1, false), (2, true)]);
    let p = ReadingProjection::build(&doc, Default::default(), Default::default()).unwrap();
    let found = p
        .find_literal("ab", ReadingCase::Exact, Default::default())
        .unwrap();
    let m = &found.matches()[0];
    assert!(m.contains_atoms());
    assert_eq!(m.start().atom_index(), 1);
    assert_eq!(m.end().atom_index(), 0);
    for (query, start, end) in [("a", 1, 0), ("b", 2, 0)] {
        let found = p
            .find_literal(query, ReadingCase::Exact, Default::default())
            .unwrap();
        let m = &found.matches()[0];
        assert!(!m.contains_atoms());
        assert_eq!(m.start().atom_index(), start);
        assert_eq!(m.end().atom_index(), end);
        m.start().validate(&doc).unwrap();
        m.end().validate(&doc).unwrap();
    }
}

#[test]
fn leading_and_only_atom_groups_preserve_distinct_ordinals() {
    let (doc, _) = mixed("😀", &[(0, true), (0, true), (0, false)]);
    let p = projection(&doc);
    let found = p
        .find_literal("😀", ReadingCase::Exact, Default::default())
        .unwrap();
    assert_eq!(found.matches()[0].start().atom_index(), 3);
    assert!(!found.matches()[0].contains_atoms());
    let (doc, _) = mixed("", &[(0, true), (0, true)]);
    let p = projection(&doc);
    let found = p
        .find_literal("\n", ReadingCase::Exact, Default::default())
        .unwrap();
    assert_eq!(found.matches().len(), 2);
    for (i, m) in found.matches().iter().enumerate() {
        assert_eq!((m.start().atom_index(), m.end().atom_index()), (i, i + 1));
        assert!(m.contains_atoms());
    }
}

#[test]
fn query_and_match_limits_fail_without_partial_results() {
    let (doc, _) = document("aaa");
    let p = projection(&doc);
    assert_eq!(
        p.find_literal(
            "abc",
            ReadingCase::Exact,
            ReadingSearchLimits {
                max_query_bytes: 2,
                ..Default::default()
            }
        )
        .unwrap_err(),
        ReadingError::BudgetExceeded(ReadingBudget::QueryBytes)
    );
    assert_eq!(
        p.find_literal(
            "a",
            ReadingCase::Exact,
            ReadingSearchLimits {
                max_matches: 2,
                ..Default::default()
            }
        )
        .unwrap_err(),
        ReadingError::BudgetExceeded(ReadingBudget::Matches)
    );
    for query in ["", "x"] {
        assert!(
            p.find_literal(
                query,
                ReadingCase::UnicodeSimple,
                ReadingSearchLimits {
                    max_matches: 0,
                    ..Default::default()
                }
            )
            .unwrap()
            .matches()
            .is_empty()
        );
    }
    assert_eq!(
        p.find_literal("a", ReadingCase::Exact, Default::default())
            .unwrap()
            .matches()
            .len(),
        3
    );
}

fn official_folds() -> BTreeMap<char, char> {
    include_str!("../src/reading/unicode/CaseFolding-17.0.0.txt")
        .lines()
        .filter_map(|line| {
            let fields: Vec<_> = line
                .split('#')
                .next()
                .unwrap()
                .split(';')
                .map(str::trim)
                .collect();
            if fields.len() < 3 || !matches!(fields[1], "C" | "S") {
                return None;
            }
            Some((
                char::from_u32(u32::from_str_radix(fields[0], 16).unwrap()).unwrap(),
                char::from_u32(u32::from_str_radix(fields[2], 16).unwrap()).unwrap(),
            ))
        })
        .collect()
}

#[test]
fn every_official_simple_fold_class_matches_in_both_directions() {
    let mut classes: BTreeMap<char, BTreeSet<char>> = BTreeMap::new();
    for (source, target) in official_folds() {
        classes.entry(target).or_default().extend([source, target]);
    }
    let mut checked = 0;
    for class in classes.values() {
        for &source in class {
            for &query in class {
                assert_eq!(
                    ranges(
                        &source.to_string(),
                        &query.to_string(),
                        ReadingCase::UnicodeSimple
                    ),
                    vec![0..source.len_utf8()]
                );
                let exact = ranges(&source.to_string(), &query.to_string(), ReadingCase::Exact);
                assert_eq!(exact.len(), usize::from(source == query));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 6084);
}

#[test]
fn streaming_kmp_agrees_with_independent_naive_scalar_search() {
    let folds = official_folds();
    let alphabet = [
        'a', 'b', 'A', 'ſ', 's', 'K', 'k', 'Σ', 'σ', 'ς', 'ß', 'ẞ', '😀', '\u{301}', '\n', '.',
        '\0',
    ];
    let mut seed = 0x55aa77u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as usize
    };
    for _ in 0..2500 {
        let haystack: String = (0..next() % 45)
            .map(|_| alphabet[next() % alphabet.len()])
            .collect();
        let query: String = (0..next() % 9)
            .map(|_| alphabet[next() % alphabet.len()])
            .collect();
        for case in [ReadingCase::Exact, ReadingCase::UnicodeSimple] {
            let key = |ch| {
                if case == ReadingCase::Exact {
                    ch
                } else {
                    folds.get(&ch).copied().unwrap_or(ch)
                }
            };
            let chars: Vec<_> = haystack.char_indices().collect();
            let needle: Vec<_> = query.chars().map(key).collect();
            let mut expected = Vec::new();
            let mut index = 0;
            while !needle.is_empty() && index + needle.len() <= chars.len() {
                if chars[index..index + needle.len()]
                    .iter()
                    .map(|entry| key(entry.1))
                    .eq(needle.iter().copied())
                {
                    let last = chars[index + needle.len() - 1];
                    expected.push(chars[index].0..last.0 + last.1.len_utf8());
                    index += needle.len();
                } else {
                    index += 1;
                }
            }
            assert_eq!(
                ranges(&haystack, &query, case),
                expected,
                "{haystack:?}/{query:?}/{case:?}"
            );
        }
    }
}
