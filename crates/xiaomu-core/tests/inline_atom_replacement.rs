//! Atom-aware mixed-inline text replacement through the public API.
//!
//! These tests exercise the `ReplaceInlineText` contract end to end: seam
//! insertion on both sides of an atom, fail-closed replacement regions,
//! pure-deletion seam ordinal merging, mark-preserving inverses, and the
//! composed remove-atom-then-replace mapping the runtime editing layer uses.

use xiaomu_core::{
    Error,
    document::{
        AtomKind, InlineAtomContent, InlineContent, Mark, MarkKind, MarkSet, NodeAttrs,
        NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    mapping::{MapBias, MappedPosition},
    selection::{CursorAffinity, InlinePoint},
    text::TextOffset,
    transaction::{Transaction, TransactionOrigin, TransactionStep},
};

fn text_fixture(text: &str) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
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
    )
}

/// Wrapper so the fixtures below can copy the crate-internal test style with
/// plain type aliases; `NodeId` is just the public node id.
fn byte_offset(raw: usize) -> TextOffset {
    const SCRATCH: &str = "0000000000000000000000000000000000000000";
    xiaomu_core::text::TextBuffer::from_string(SCRATCH.to_owned())
        .offset_at(raw)
        .unwrap()
}

fn gap(
    document: &XiaomuDocument,
    paragraph: NodeId,
    char_index: usize,
    atom_index: usize,
) -> InlinePoint {
    let offset = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .offset_at(char_index)
        .unwrap();
    InlinePoint::new(paragraph, offset, atom_index, CursorAffinity::Before)
}

fn insert_atom_at(
    document: &XiaomuDocument,
    paragraph: NodeId,
    char_index: usize,
    atom_index: usize,
    fallback: &str,
) -> xiaomu_core::transaction::AppliedTransaction {
    Transaction::new(TransactionOrigin::Extension("atom-test".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at: gap(document, paragraph, char_index, atom_index),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply_with_changes(document)
        .unwrap()
}

fn replace_inline(at: InlinePoint, end: TextOffset, replacement: &str) -> TransactionStep {
    TransactionStep::ReplaceInlineText {
        at,
        end,
        replacement: replacement.to_owned(),
    }
}

fn placements_of(document: &XiaomuDocument, paragraph: NodeId) -> Vec<(NodeId, TextOffset)> {
    document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| (placement.atom(), placement.text_offset()))
        .collect()
}

fn text_of(document: &XiaomuDocument, paragraph: NodeId) -> String {
    document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

fn first_atom(document: &XiaomuDocument, paragraph: NodeId) -> NodeId {
    document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()[0]
        .atom()
}

#[test]
fn seam_insertion_matches_design_example_on_both_sides() {
    let (document, paragraph) = text_fixture("ab");
    let applied = insert_atom_at(&document, paragraph, 1, 0, "@A");
    let with_atom = applied.document().clone();
    let atom = first_atom(&with_atom, paragraph);

    // (1, 0) + X → A X [atom] B
    let before = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_inline(
            gap(&with_atom, paragraph, 1, 0),
            byte_offset(1),
            "X",
        ))
        .apply_with_changes(&with_atom)
        .unwrap();
    assert_eq!(text_of(before.document(), paragraph), "aXb");
    assert_eq!(
        placements_of(before.document(), paragraph),
        [(atom, byte_offset(2))]
    );
    let undone = before.inverse().apply(before.document()).unwrap();
    assert_eq!(undone.store(), with_atom.store());

    // (1, 1) + X → A [atom] X B
    let after = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_inline(
            gap(&with_atom, paragraph, 1, 1),
            byte_offset(1),
            "X",
        ))
        .apply_with_changes(&with_atom)
        .unwrap();
    assert_eq!(text_of(after.document(), paragraph), "aXb");
    assert_eq!(
        placements_of(after.document(), paragraph),
        [(atom, byte_offset(1))]
    );
    let undone = after.inverse().apply(after.document()).unwrap();
    assert_eq!(undone.store(), with_atom.store());
}

#[test]
fn replacement_after_seam_atom_preserves_identity_and_inverts() {
    let (document, paragraph) = text_fixture("abc");
    let applied = insert_atom_at(&document, paragraph, 1, 0, "@A");
    let with_atom = applied.document().clone();
    let atom = first_atom(&with_atom, paragraph);

    let replaced = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_inline(
            gap(&with_atom, paragraph, 1, 1),
            byte_offset(2),
            "XY",
        ))
        .apply_with_changes(&with_atom)
        .unwrap();
    assert_eq!(text_of(replaced.document(), paragraph), "aXYc");
    assert_eq!(
        placements_of(replaced.document(), paragraph),
        [(atom, byte_offset(1))]
    );

    let undone = replaced.inverse().apply(replaced.document()).unwrap();
    assert_eq!(undone.store(), with_atom.store());
    assert_eq!(undone.root(), with_atom.root());
}

#[test]
fn replacement_containing_seam_atom_fails_closed_atomically() {
    let (document, paragraph) = text_fixture("ab");
    let applied = insert_atom_at(&document, paragraph, 1, 0, "@A");
    let with_atom = applied.document().clone();

    // The region starts before the seam atom, so the atom would fall inside
    // the replaced span; the step refuses instead of deleting it.
    let rejected = Transaction::new(TransactionOrigin::UserInput).with_step(replace_inline(
        gap(&with_atom, paragraph, 1, 0),
        byte_offset(2),
        "X",
    ));
    assert!(matches!(
        rejected.apply(&with_atom),
        Err(Error::InvalidTransaction)
    ));
    assert_eq!(with_atom.node_count(), 3);
}

#[test]
fn pure_deletion_merges_seam_ordinals_and_inverts() {
    let (document, paragraph) = text_fixture("abc");
    let first = insert_atom_at(&document, paragraph, 1, 0, "@A");
    let second = insert_atom_at(first.document(), paragraph, 2, 0, "@B");
    let with_atoms = second.document().clone();
    let atoms = placements_of(&with_atoms, paragraph);

    // Delete "b" starting after the atom anchored at offset 1: the atom
    // anchored at offset 2 survives and merges behind it.
    let deleted = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_inline(
            gap(&with_atoms, paragraph, 1, 1),
            byte_offset(2),
            "",
        ))
        .apply_with_changes(&with_atoms)
        .unwrap();
    assert_eq!(text_of(deleted.document(), paragraph), "ac");
    assert_eq!(
        placements_of(deleted.document(), paragraph),
        [(atoms[0].0, byte_offset(1)), (atoms[1].0, byte_offset(1))]
    );

    let undone = deleted.inverse().apply(deleted.document()).unwrap();
    assert_eq!(undone.store(), with_atoms.store());
}

#[test]
fn marked_replacement_round_trips_through_atom_aware_inverse() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([
                    TextRun::new("ab", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
                    TextRun::new("cd", MarkSet::empty()).unwrap(),
                ])
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
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();

    let applied = insert_atom_at(&document, paragraph, 2, 0, "@A");
    let with_atom = applied.document().clone();

    // Replace bold "b" with "XY" starting before the atom at offset 2: the
    // replacement inherits bold, and the atom at the end boundary shifts by
    // the length delta.
    let replaced = Transaction::new(TransactionOrigin::UserInput)
        .with_step(replace_inline(
            gap(&with_atom, paragraph, 1, 0),
            byte_offset(2),
            "XY",
        ))
        .apply_with_changes(&with_atom)
        .unwrap();
    let inline = replaced
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(
        inline
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<Vec<_>>(),
        ["aXY", "cd"]
    );
    assert!(inline.runs()[0].marks().contains(MarkKind::Bold));
    assert_eq!(inline.atoms()[0].text_offset(), byte_offset(3));

    let undone = replaced.inverse().apply(replaced.document()).unwrap();
    assert_eq!(undone.store(), with_atom.store());
}

#[test]
fn composed_remove_atom_then_replacement_maps_the_edited_gap() {
    let (document, paragraph) = text_fixture("ab");
    let applied = insert_atom_at(&document, paragraph, 1, 0, "@A");
    let with_atom = applied.document().clone();
    let atom = first_atom(&with_atom, paragraph);

    // Runtime-style decomposition of "select across the atom and type": an
    // explicit atom removal, then the atom-aware replacement.
    let composed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveInlineAtom { atom })
        .with_step(replace_inline(
            gap(&with_atom, paragraph, 1, 0),
            byte_offset(2),
            "X",
        ))
        .apply_with_changes(&with_atom)
        .unwrap();
    assert_eq!(text_of(composed.document(), paragraph), "aX");
    assert!(composed.document().node(atom).is_none());

    // A caret that sat after the removed atom maps past the replacement.
    let old_focus = InlinePoint::new(paragraph, byte_offset(1), 1, CursorAffinity::Before);
    assert_eq!(
        composed.changes().map_inline_point(old_focus, MapBias::End),
        MappedPosition::Mapped(InlinePoint::new(
            paragraph,
            byte_offset(2),
            0,
            CursorAffinity::Before,
        ))
    );

    let undone = composed.inverse().apply(composed.document()).unwrap();
    assert_eq!(undone.store(), with_atom.store());
}
