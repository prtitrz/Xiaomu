//! Independent atom marks remain identity-preserving and exactly reversible.

use std::collections::BTreeMap;

use xiaomu_core::{
    Error,
    document::{
        AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, LinkAttributes,
        LinkMark, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder,
        StringAttribute, TextRun, TextStyleAttributes, TextStyleMark, XiaomuDocument,
    },
    mapping::{MapBias, MappedPosition},
    selection::{CursorAffinity, InlinePoint},
    text::TextBuffer,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};

fn marks() -> MarkSet {
    MarkSet::new([
        Mark::Code,
        Mark::Bold,
        Mark::Link(LinkMark::from_attributes(
            LinkAttributes::default()
                .with_href("原值🙂".into())
                .with_target(StringAttribute::Null)
                .with_rel("".into()),
        )),
        Mark::TextStyle(TextStyleMark::from_attributes(
            TextStyleAttributes::default()
                .with_color(StringAttribute::Null)
                .with_font_family("宋体, serif".into())
                .with_font_size("calc(1em + 2px)".into()),
        )),
    ])
    .unwrap()
}

fn fixture() -> (XiaomuDocument, NodeId, [NodeId; 2]) {
    let mut builder = NodeStoreBuilder::new();
    let hard_break = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::hard_break()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::hard_break()),
        )
        .unwrap();
    let extension = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("hardBreak").unwrap()),
            NodeAttrs::new(BTreeMap::from([("future".into(), AttrValue::Null)])).unwrap(),
            NodeContent::InlineAtom(InlineAtomContent::new("@张🙂\r\n").unwrap()),
        )
        .unwrap();
    let text = TextBuffer::from_string("中\n🙂".into());
    let seam = text.offset_at("中".len()).unwrap();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(text, MarkSet::new([Mark::Italic]).unwrap()).unwrap()],
                    [hard_break, extension].map(|atom| InlineAtomPlacement::new(atom, seam)),
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([paragraph]),
        )
        .unwrap();
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
        [hard_break, extension],
    )
}

fn set(atom: NodeId, marks: MarkSet) -> TransactionStep {
    TransactionStep::SetInlineAtomMarks { atom, marks }
}

#[test]
fn set_atom_marks_preserves_payload_placement_and_maps_all_seam_gaps_identically() {
    let (document, paragraph, atoms) = fixture();
    for atom in atoms {
        let applied = Transaction::new(TransactionOrigin::UserInput)
            .with_step(set(atom, marks()))
            .apply_with_changes(&document)
            .unwrap();
        let next = applied.document();
        let old = document.node(atom).unwrap();
        let new = next.node(atom).unwrap();
        assert_eq!(old.id(), new.id());
        assert_eq!(old.kind(), new.kind());
        assert_eq!(old.attrs(), new.attrs());
        assert_eq!(
            old.content().as_inline_atom().unwrap().fallback_text(),
            new.content().as_inline_atom().unwrap().fallback_text()
        );
        assert_eq!(new.content().as_inline_atom().unwrap().marks(), &marks());
        assert_eq!(next.node(paragraph), document.node(paragraph));
        assert_eq!(next.node_count(), document.node_count());
        assert_eq!(next.parent_of(atom), Some(paragraph));
        assert!(applied.changes().steps().is_empty());
        let inline = document
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        for ordinal in 0..=2 {
            let point = InlinePoint::new(
                paragraph,
                inline.offset_at(3).unwrap(),
                ordinal,
                CursorAffinity::After,
            );
            for bias in [MapBias::Start, MapBias::End] {
                assert_eq!(
                    applied.changes().map_inline_point(point, bias),
                    MappedPosition::Mapped(point)
                );
            }
        }
        assert_eq!(applied.inverse().steps(), &[set(atom, MarkSet::empty())]);
        let undone = applied.inverse().apply_with_changes(next).unwrap();
        assert_eq!(undone.document().store(), document.store());
        let redone = undone.inverse().apply(undone.document()).unwrap();
        assert_eq!(redone.store(), next.store());
    }
}

#[test]
fn multiple_atom_mark_replacements_restore_each_exact_intermediate_value() {
    let (document, _, [first, second]) = fixture();
    let italic = MarkSet::new([Mark::Italic]).unwrap();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(set(first, marks()))
        .with_step(set(second, italic.clone()))
        .with_step(set(first, MarkSet::empty()))
        .apply_with_changes(&document)
        .unwrap();
    assert_eq!(
        applied.inverse().steps(),
        &[
            set(first, marks()),
            set(second, MarkSet::empty()),
            set(first, MarkSet::empty()),
        ]
    );
    assert_eq!(
        applied
            .document()
            .node(second)
            .unwrap()
            .content()
            .as_inline_atom()
            .unwrap()
            .marks(),
        &italic
    );
    let undone = applied.inverse().apply(applied.document()).unwrap();
    assert_eq!(undone.store(), document.store());
}

#[test]
fn non_atom_and_removed_atom_targets_reject_without_partial_publication() {
    let (document, paragraph, [first, _]) = fixture();
    for target in [paragraph, document.root()] {
        let attempted = Transaction::new(TransactionOrigin::UserInput)
            .with_step(set(first, marks()))
            .with_step(set(target, marks()));
        assert!(matches!(
            attempted.apply(&document),
            Err(Error::InvalidTransaction)
        ));
        assert!(
            document
                .node(first)
                .unwrap()
                .content()
                .as_inline_atom()
                .unwrap()
                .marks()
                .is_empty()
        );
    }
    let attempted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveInlineAtom { atom: first })
        .with_step(set(first, marks()));
    assert!(matches!(
        attempted.apply(&document),
        Err(Error::UnknownNode)
    ));
    assert!(document.node(first).is_some());
}

#[test]
fn later_hard_break_invariant_failure_discards_prior_atom_mark_changes() {
    let (document, _, [hard_break, extension]) = fixture();
    let before = document.clone();
    let attempted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(set(extension, marks()))
        .with_step(set(hard_break, marks()))
        .with_step(TransactionStep::SetNodeAttrs {
            node: hard_break,
            attrs: NodeAttrs::new(BTreeMap::from([("bad".into(), AttrValue::Null)])).unwrap(),
        });
    assert!(matches!(
        attempted.apply(&document),
        Err(Error::InvalidHardBreak)
    ));
    assert_eq!(document.store(), before.store());
    assert_eq!(document.revision(), before.revision());
}
