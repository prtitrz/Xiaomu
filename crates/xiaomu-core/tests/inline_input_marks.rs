//! Input inheritance follows canonical mixed-inline children, independently of
//! visual affinity, while replacement inverses restore every original mark.

use std::collections::BTreeMap;

use xiaomu_core::{
    document::{
        AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
        LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
    },
    selection::{CursorAffinity, InlinePoint},
    text::{TextBuffer, TextOffset},
    transaction::{AppliedTransaction, Transaction, TransactionOrigin, TransactionStep},
};

fn bold() -> MarkSet {
    MarkSet::new([Mark::Bold]).unwrap()
}

fn italic() -> MarkSet {
    MarkSet::new([Mark::Italic]).unwrap()
}

fn code() -> MarkSet {
    MarkSet::new([Mark::Code]).unwrap()
}

fn link() -> MarkSet {
    MarkSet::new([Mark::Link(LinkMark::from_attributes(
        LinkAttributes::default()
            .with_href(" https://example.test/原🙂?q=1 ".into())
            .with_target(StringAttribute::Null)
            .with_rel("".into()),
    ))])
    .unwrap()
}

fn rich_marks() -> MarkSet {
    MarkSet::new(
        [
            Mark::Code,
            Mark::Bold,
            Mark::TextStyle(TextStyleMark::from_attributes(
                TextStyleAttributes::default()
                    .with_color(StringAttribute::Null)
                    .with_font_family("宋体, serif".into())
                    .with_font_size("calc(1em + 2px)".into()),
            )),
        ]
        .into_iter()
        .chain(link().as_slice().iter().cloned()),
    )
    .unwrap()
}

// Construct offsets independently of the target to test its own validation.
fn offset(byte: usize) -> TextOffset {
    TextBuffer::from(" ".repeat(byte)).offset_at(byte).unwrap()
}

fn point(node: NodeId, byte: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(node, offset(byte), ordinal, CursorAffinity::Before)
}

struct AtomSpec {
    byte: usize,
    kind: AtomKind,
    attrs: NodeAttrs,
    content: InlineAtomContent,
}

fn hard_break(byte: usize, marks: MarkSet) -> AtomSpec {
    AtomSpec {
        byte,
        kind: AtomKind::hard_break(),
        attrs: NodeAttrs::empty(),
        content: InlineAtomContent::hard_break().with_marks(marks),
    }
}

fn extension(byte: usize, marks: MarkSet) -> AtomSpec {
    AtomSpec {
        byte,
        // A colliding label must not confer typed-hard-break semantics.
        kind: AtomKind::new("hardBreak").unwrap(),
        attrs: NodeAttrs::new(BTreeMap::from([
            ("future".into(), AttrValue::Null),
            ("payload".into(), AttrValue::String("原🙂".into())),
        ]))
        .unwrap(),
        content: InlineAtomContent::new("@张🙂\r\n")
            .unwrap()
            .with_marks(marks),
    }
}

struct Fixture {
    document: XiaomuDocument,
    paragraph: NodeId,
    other: NodeId,
    missing: NodeId,
    atoms: Vec<NodeId>,
}

fn fixture(runs: &[(&str, MarkSet)], specs: Vec<AtomSpec>) -> Fixture {
    let mut builder = NodeStoreBuilder::new();
    let mut atoms = Vec::new();
    let placements: Vec<_> = specs
        .into_iter()
        .map(|spec| {
            let atom = builder
                .insert(
                    NodeKind::InlineAtom(spec.kind),
                    spec.attrs,
                    NodeContent::InlineAtom(spec.content),
                )
                .unwrap();
            atoms.push(atom);
            InlineAtomPlacement::new(atom, offset(spec.byte))
        })
        .collect();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    runs.iter()
                        .map(|(text, marks)| TextRun::new(*text, marks.clone()).unwrap()),
                    placements,
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let other = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("other", MarkSet::empty()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph, other]),
        )
        .unwrap();
    let missing = builder.peek_next_id();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
        other,
        missing,
        atoms,
    }
}

fn mixed_fixture() -> Fixture {
    fixture(
        &[
            ("中", bold()),
            ("🙂", italic()),
            ("e\u{301}", code()),
            ("\nZ", link()),
        ],
        vec![
            hard_break(0, MarkSet::empty()),
            hard_break(3, bold()),
            hard_break(3, italic()),
            hard_break(3, code()),
            hard_break(3, MarkSet::empty()),
            hard_break(3, link()),
            hard_break(12, italic()),
            hard_break(12, MarkSet::empty()),
        ],
    )
}

fn caret_cases() -> Vec<(usize, usize, MarkSet)> {
    vec![
        (0, 0, MarkSet::empty()),
        (0, 1, MarkSet::empty()),
        (3, 0, bold()),
        (3, 1, bold()),
        (3, 2, italic()),
        (3, 3, code()),
        (3, 4, MarkSet::empty()),
        (3, 5, link()),
        (7, 0, italic()),
        (8, 0, code()),
        (10, 0, code()),
        (11, 0, link()),
        (12, 0, link()),
        (12, 1, italic()),
        (12, 2, MarkSet::empty()),
    ]
}

fn inline(document: &XiaomuDocument, node: NodeId) -> &InlineContent {
    document.node(node).unwrap().content().as_inline().unwrap()
}

fn text(document: &XiaomuDocument, node: NodeId) -> String {
    inline(document, node)
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn apply_replace(
    f: &Fixture,
    at: InlinePoint,
    end: usize,
    replacement: &str,
) -> AppliedTransaction {
    Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::ReplaceInlineText {
            at,
            end: offset(end),
            replacement: replacement.into(),
        })
        .apply_with_changes(&f.document)
        .unwrap()
}

fn assert_span_marks(
    document: &XiaomuDocument,
    node: NodeId,
    start: usize,
    end: usize,
    marks: &MarkSet,
) {
    let mut cursor = 0;
    let mut covered = 0;
    for run in inline(document, node).runs() {
        let run_end = cursor + run.len_bytes();
        let overlap_start = cursor.max(start);
        let overlap_end = run_end.min(end);
        if overlap_start < overlap_end {
            assert_eq!(
                run.marks(),
                marks,
                "marks at {overlap_start}..{overlap_end}"
            );
            covered += overlap_end - overlap_start;
        }
        cursor = run_end;
    }
    assert_eq!(covered, end - start);
}

fn assert_roundtrip(f: &Fixture, applied: &AppliedTransaction) {
    for atom in &f.atoms {
        // Identity, typed kind, independent marks, attrs and fallback are exact.
        assert_eq!(applied.document().node(*atom), f.document.node(*atom));
        assert_eq!(applied.document().parent_of(*atom), Some(f.paragraph));
    }
    assert_eq!(applied.document().node_count(), f.document.node_count());
    assert_eq!(applied.document().node(f.other), f.document.node(f.other));
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), f.document.store());
    assert_eq!(undone.document().root(), f.document.root());
    let redone = undone
        .inverse()
        .apply_with_changes(undone.document())
        .unwrap();
    assert_eq!(redone.document().store(), applied.document().store());
    let undone_again = redone.inverse().apply(redone.document()).unwrap();
    assert_eq!(undone_again.store(), f.document.store());
}

#[test]
fn caret_inherits_every_canonical_seam_and_utf8_boundary_independently_of_affinity() {
    let f = mixed_fixture();
    for (byte, ordinal, expected) in caret_cases() {
        for affinity in [CursorAffinity::Before, CursorAffinity::After] {
            let at = point(f.paragraph, byte, ordinal).with_affinity(affinity);
            assert_eq!(
                f.document.inherited_inline_marks(at).unwrap(),
                expected,
                "{at:?}"
            );
        }
    }
}

#[test]
fn range_uses_the_right_child_even_for_collapsed_ranges_and_returns_none_at_tail() {
    let f = mixed_fixture();
    let cases = [
        (0, 0, Some(MarkSet::empty())),
        (0, 1, Some(bold())),
        (3, 0, Some(bold())),
        (3, 1, Some(italic())),
        (3, 2, Some(code())),
        (3, 3, Some(MarkSet::empty())),
        (3, 4, Some(link())),
        (3, 5, Some(italic())),
        (7, 0, Some(code())),
        (8, 0, Some(code())),
        (10, 0, Some(link())),
        (11, 0, Some(link())),
        (12, 0, Some(italic())),
        (12, 1, Some(MarkSet::empty())),
        (12, 2, None),
    ];
    for (byte, ordinal, expected) in cases {
        for affinity in [CursorAffinity::Before, CursorAffinity::After] {
            let start = point(f.paragraph, byte, ordinal).with_affinity(affinity);
            for end in [start, point(f.paragraph, 12, 2)] {
                assert_eq!(
                    f.document.inherited_inline_range_marks(start, end).unwrap(),
                    expected,
                    "{start:?}..{end:?}"
                );
            }
        }
    }
    // Atom-only selection has zero text bytes, but a real right contributor.
    for (ordinal, expected) in [bold(), italic(), code(), MarkSet::empty(), link()]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            f.document
                .inherited_inline_range_marks(
                    point(f.paragraph, 3, ordinal),
                    point(f.paragraph, 3, ordinal + 1),
                )
                .unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn text_only_caret_retains_left_run_rules_but_range_helper_has_right_child_semantics() {
    let f = fixture(&[("中", bold()), ("🙂", link())], vec![]);
    for (byte, caret, range) in [
        (0, bold(), Some(bold())),
        (3, bold(), Some(link())),
        (7, link(), None),
    ] {
        let at = point(f.paragraph, byte, 0);
        assert_eq!(f.document.inherited_inline_marks(at).unwrap(), caret);
        assert_eq!(
            f.document.inherited_inline_range_marks(at, at).unwrap(),
            range
        );
    }
    let empty = fixture(&[], vec![]);
    let at = point(empty.paragraph, 0, 0);
    assert_eq!(
        empty.document.inherited_inline_marks(at).unwrap(),
        MarkSet::empty()
    );
    assert_eq!(
        empty.document.inherited_inline_range_marks(at, at).unwrap(),
        None
    );
}

#[test]
fn break_only_content_preserves_empty_marks_as_a_real_contributor() {
    let marks = [bold(), italic(), code(), MarkSet::empty(), link()];
    let f = fixture(
        &[],
        marks
            .iter()
            .cloned()
            .map(|marks| hard_break(0, marks))
            .collect(),
    );
    for ordinal in 0..=marks.len() {
        let at = point(f.paragraph, 0, ordinal);
        let expected_left = &marks[ordinal.saturating_sub(1)];
        assert_eq!(
            &f.document.inherited_inline_marks(at).unwrap(),
            expected_left
        );
        assert_eq!(
            f.document
                .inherited_inline_range_marks(at, point(f.paragraph, 0, marks.len()))
                .unwrap(),
            marks.get(ordinal).cloned()
        );
        let applied = apply_replace(&f, at, 0, "新🙂");
        assert_span_marks(
            applied.document(),
            f.paragraph,
            0,
            "新🙂".len(),
            expected_left,
        );
        assert_roundtrip(&f, &applied);
    }
    let unmarked = fixture(&[], vec![hard_break(0, MarkSet::empty())]);
    assert_eq!(
        unmarked
            .document
            .inherited_inline_range_marks(
                point(unmarked.paragraph, 0, 0),
                point(unmarked.paragraph, 0, 1)
            )
            .unwrap(),
        Some(MarkSet::empty())
    );
}

#[test]
fn marked_extensions_contribute_exact_marks_while_legacy_unmarked_extensions_are_transparent() {
    let f = fixture(
        &[("中", bold()), ("🙂", italic())],
        vec![
            extension(3, MarkSet::empty()),
            extension(3, rich_marks()),
            extension(3, MarkSet::empty()),
        ],
    );
    for (ordinal, left, right) in [
        (0, bold(), rich_marks()),
        (1, bold(), rich_marks()),
        (2, rich_marks(), italic()),
        (3, rich_marks(), italic()),
    ] {
        let at = point(f.paragraph, 3, ordinal);
        assert_eq!(f.document.inherited_inline_marks(at).unwrap(), left);
        assert_eq!(
            f.document
                .inherited_inline_range_marks(at, point(f.paragraph, 7, 0))
                .unwrap(),
            Some(right)
        );
        let applied = apply_replace(&f, at, 3, "新");
        assert_span_marks(applied.document(), f.paragraph, 3, 6, &left);
        assert_roundtrip(&f, &applied);
    }
    // Neither the extension's label nor its multiline fallback makes it a break.
    let only_legacy = fixture(&[], vec![extension(0, MarkSet::empty())]);
    for ordinal in 0..=1 {
        let at = point(only_legacy.paragraph, 0, ordinal);
        assert_eq!(
            only_legacy.document.inherited_inline_marks(at).unwrap(),
            MarkSet::empty()
        );
        assert_eq!(
            only_legacy
                .document
                .inherited_inline_range_marks(at, at)
                .unwrap(),
            None
        );
    }
    let edge = fixture(
        &[("中", link())],
        vec![
            extension(0, MarkSet::empty()),
            extension(3, MarkSet::empty()),
        ],
    );
    for (byte, ordinal) in [(0, 0), (0, 1), (3, 0), (3, 1)] {
        let at = point(edge.paragraph, byte, ordinal);
        assert_eq!(edge.document.inherited_inline_marks(at).unwrap(), link());
        assert_eq!(
            edge.document.inherited_inline_range_marks(at, at).unwrap(),
            if byte == 0 { Some(link()) } else { None }
        );
    }
}

#[test]
fn input_keeps_links_inclusive_and_does_not_apply_code_exclusion_or_attribute_cleanup() {
    for atom in [hard_break(0, rich_marks()), extension(0, rich_marks())] {
        let f = fixture(&[], vec![atom]);
        for ordinal in 0..=1 {
            let at = point(f.paragraph, 0, ordinal);
            assert_eq!(f.document.inherited_inline_marks(at).unwrap(), rich_marks());
            let applied = apply_replace(&f, at, 0, "新\n🙂");
            assert_span_marks(
                applied.document(),
                f.paragraph,
                0,
                "新\n🙂".len(),
                &rich_marks(),
            );
            assert_roundtrip(&f, &applied);
        }
    }
    let f = fixture(&[("中", link())], vec![]);
    let applied = apply_replace(&f, point(f.paragraph, 3, 0), 3, "🙂");
    assert_eq!(
        inline(applied.document(), f.paragraph).runs(),
        &[TextRun::new("中🙂", link()).unwrap()]
    );
    assert_roundtrip(&f, &applied);
}

#[test]
fn invalid_points_and_ranges_fail_without_clamping_or_cross_node_fallback() {
    let f = mixed_fixture();
    let start = point(f.paragraph, 0, 0);
    let end = point(f.paragraph, 12, 2);
    for invalid in [
        point(f.paragraph, 1, 0), // Inside 中.
        point(f.paragraph, 4, 0), // Inside 🙂.
        point(f.paragraph, 9, 0), // Inside the combining scalar.
        point(f.paragraph, 13, 0),
        point(f.paragraph, 0, 2),
        point(f.paragraph, 3, 6),
        point(f.paragraph, 7, 1),
        point(f.paragraph, 12, 3),
        point(f.document.root(), 0, 0),
        point(f.atoms[0], 0, 0),
        point(f.missing, 0, 0),
    ] {
        assert!(
            f.document.inherited_inline_marks(invalid).is_err(),
            "{invalid:?}"
        );
        assert!(
            f.document
                .inherited_inline_range_marks(invalid, end)
                .is_err()
        );
        assert!(
            f.document
                .inherited_inline_range_marks(start, invalid)
                .is_err()
        );
        assert!(
            f.document
                .inherited_inline_range_marks(invalid, invalid)
                .is_err()
        );
    }
    for (start, end) in [
        (point(f.paragraph, 7, 0), point(f.paragraph, 3, 5)),
        (point(f.paragraph, 3, 2), point(f.paragraph, 3, 1)),
        (point(f.paragraph, 0, 1), point(f.paragraph, 0, 0)),
        (point(f.paragraph, 0, 0), point(f.other, 0, 0)),
        (point(f.other, 0, 0), point(f.paragraph, 12, 2)),
    ] {
        assert!(
            f.document.inherited_inline_range_marks(start, end).is_err(),
            "{start:?}..{end:?}"
        );
    }
}

#[test]
fn replace_inline_text_uses_caret_marks_at_every_seam_and_moves_only_later_atoms() {
    let f = mixed_fixture();
    let original_text = text(&f.document, f.paragraph);
    let replacement = "新🙂";
    for (byte, ordinal, expected) in caret_cases() {
        for affinity in [CursorAffinity::Before, CursorAffinity::After] {
            let at = point(f.paragraph, byte, ordinal).with_affinity(affinity);
            let applied = apply_replace(&f, at, byte, replacement);
            assert_eq!(
                text(applied.document(), f.paragraph),
                format!(
                    "{}{}{}",
                    &original_text[..byte],
                    replacement,
                    &original_text[byte..]
                )
            );
            assert_span_marks(
                applied.document(),
                f.paragraph,
                byte,
                byte + replacement.len(),
                &expected,
            );
            let mut same_boundary_ordinal = 0;
            for (before, after) in inline(&f.document, f.paragraph)
                .atoms()
                .iter()
                .zip(inline(applied.document(), f.paragraph).atoms())
            {
                let old_byte = before.text_offset().as_usize();
                let shifted =
                    old_byte > byte || (old_byte == byte && same_boundary_ordinal >= ordinal);
                if old_byte == byte {
                    same_boundary_ordinal += 1;
                }
                assert_eq!(after.atom(), before.atom());
                assert_eq!(
                    after.text_offset(),
                    offset(old_byte + if shifted { replacement.len() } else { 0 })
                );
            }
            assert_roundtrip(&f, &applied);
        }
    }
}

#[test]
fn replacing_or_deleting_after_each_break_mark_restores_original_runs_without_mark_leakage() {
    for atom_marks in [
        bold(),
        italic(),
        code(),
        MarkSet::empty(),
        link(),
        rich_marks(),
    ] {
        let f = fixture(
            &[
                ("中", bold()),
                ("🙂", italic()),
                ("e\u{301}", MarkSet::empty()),
                ("\nZ", link()),
            ],
            vec![
                hard_break(3, atom_marks.clone()),
                extension(12, rich_marks()),
            ],
        );
        for end in [7, 10, 12] {
            for replacement in ["", "新", "新\n🙂"] {
                let applied = apply_replace(&f, point(f.paragraph, 3, 1), end, replacement);
                assert_span_marks(
                    applied.document(),
                    f.paragraph,
                    3,
                    3 + replacement.len(),
                    &atom_marks,
                );
                assert_roundtrip(&f, &applied);
            }
        }
    }
}

#[test]
fn deleting_leading_text_then_undoing_strips_marks_of_the_new_first_break() {
    // With the prefix gone, undo inserts before the break and inherits its
    // marks. Those may differ from the original prefix's inherited marks.
    for atom_marks in [italic(), code(), MarkSet::empty(), link(), rich_marks()] {
        for runs in [
            vec![("中", bold())],
            vec![("中", bold()), ("🙂", MarkSet::empty())],
        ] {
            let f = fixture(
                &runs,
                vec![
                    hard_break(3, atom_marks.clone()),
                    extension(3, rich_marks()),
                ],
            );
            let applied = apply_replace(&f, point(f.paragraph, 0, 0), 3, "");
            assert_eq!(
                applied
                    .document()
                    .inherited_inline_marks(point(f.paragraph, 0, 0))
                    .unwrap(),
                atom_marks
            );
            assert_roundtrip(&f, &applied);
        }
    }
}

#[test]
fn replacement_inherits_atom_marks_from_the_current_transaction_store() {
    let f = fixture(
        &[("中", bold()), ("🙂", italic())],
        vec![hard_break(3, MarkSet::empty())],
    );
    let atom = f.atoms[0];
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetInlineAtomMarks {
            atom,
            marks: rich_marks(),
        })
        .with_step(TransactionStep::ReplaceInlineText {
            at: point(f.paragraph, 3, 1),
            end: offset(7),
            replacement: "新🙂".into(),
        })
        .apply_with_changes(&f.document)
        .unwrap();
    assert_span_marks(applied.document(), f.paragraph, 3, 10, &rich_marks());
    assert_eq!(
        applied
            .document()
            .node(atom)
            .unwrap()
            .content()
            .as_inline_atom()
            .unwrap()
            .marks(),
        &rich_marks()
    );
    let undone = applied
        .inverse()
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(undone.document().store(), f.document.store());
    let redone = undone
        .inverse()
        .apply_with_changes(undone.document())
        .unwrap();
    assert_eq!(redone.document().store(), applied.document().store());
    let undone_again = redone.inverse().apply(redone.document()).unwrap();
    assert_eq!(undone_again.store(), f.document.store());
}

#[test]
fn invalid_replacements_do_not_publish_prior_atom_mark_changes() {
    let f = mixed_fixture();
    let original = f.document.store().clone();
    for (at, end) in [
        (point(f.paragraph, 1, 0), 3),
        (point(f.paragraph, 3, 5), 4),
        (point(f.paragraph, 3, 6), 3),
        (point(f.paragraph, 7, 0), 3),
        (point(f.paragraph, 3, 0), 7), // Would consume same-seam atoms.
        (point(f.paragraph, 7, 0), 13),
    ] {
        let attempted = Transaction::new(TransactionOrigin::UserInput)
            .with_step(TransactionStep::SetInlineAtomMarks {
                atom: f.atoms[0],
                marks: rich_marks(),
            })
            .with_step(TransactionStep::ReplaceInlineText {
                at,
                end: offset(end),
                replacement: "新🙂".into(),
            });
        assert!(attempted.apply(&f.document).is_err(), "{at:?}..{end}");
        assert_eq!(f.document.store(), &original);
    }
}
