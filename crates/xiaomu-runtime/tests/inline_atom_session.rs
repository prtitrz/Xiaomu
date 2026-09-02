//! P4.3 Runtime mixed-inline session semantics.
//!
//! The session stores full `(text_offset, atom_index)` caret positions and
//! moves one caret unit per step: an inline atom is an indivisible unit at
//! its anchor boundary (ADR 0005). Selection endpoints at atom seams survive
//! atom transactions through `ChangeMap::map_inline_point`.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::CaretMove;
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

fn offset_at(raw: usize) -> xiaomu_core::text::TextOffset {
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from_string(SCRATCH.to_owned())
        .offset_at(raw)
        .unwrap()
}

fn document_with(text: &str, atoms_at: &[(usize, &str)]) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap(),
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
    let mut document = XiaomuDocument::new(root, builder.finish()).unwrap();

    for &(char_index, fallback) in atoms_at {
        let offset = offset_at(char_index);
        document = Transaction::new(TransactionOrigin::Extension("atom-test".into()))
            .with_step(TransactionStep::InsertInlineAtom {
                at: InlinePoint::new(paragraph, offset, 0, CursorAffinity::Before),
                kind: AtomKind::new("mention").unwrap(),
                attrs: NodeAttrs::empty(),
                content: InlineAtomContent::new(fallback).unwrap(),
            })
            .apply(&document)
            .unwrap();
    }
    (document, paragraph)
}

fn seam_point(node: NodeId, raw: usize, ordinal: usize) -> DocumentPosition {
    DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_at(raw),
        ordinal,
        CursorAffinity::Before,
    ))
}

fn caret(session: &DocumentSession) -> InlinePoint {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => point,
        DocumentPosition::Gap(_) => panic!("caret must stay on inline text"),
    }
}

fn moved(session: &mut DocumentSession, direction: CaretMove) -> InlinePoint {
    let outcome = session
        .apply_intent(&EditIntent::MoveCaret {
            caret_move: direction,
            extend_selection: false,
        })
        .unwrap();
    assert_eq!(outcome, SessionOutcome::SelectionChanged);
    caret(session)
}

#[test]
fn caret_crosses_a_single_atom_one_unit_per_step() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let start = DocumentSelection::collapsed(seam_point(paragraph, 1, 0));
    let mut session = DocumentSession::new(document, start).unwrap();

    // (1,0) → (1,1): across the atom.
    let after = moved(&mut session, CaretMove::Forward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (1, 1));

    // (1,1) → (2,0): across the following scalar.
    let after = moved(&mut session, CaretMove::Forward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (2, 0));

    // Backward re-walks the same units in reverse.
    let after = moved(&mut session, CaretMove::Backward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (1, 1));
    let after = moved(&mut session, CaretMove::Backward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (1, 0));
    let after = moved(&mut session, CaretMove::Backward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (0, 0));
}

#[test]
fn caret_crosses_adjacent_atoms_one_unit_per_step() {
    let (document, paragraph) = document_with("ab", &[(1, "@A"), (1, "@B")]);
    let start = DocumentSelection::collapsed(seam_point(paragraph, 1, 0));
    let mut session = DocumentSession::new(document, start).unwrap();

    for expected_ordinal in [1usize, 2] {
        let after = moved(&mut session, CaretMove::Forward);
        assert_eq!(
            (after.text_offset().as_usize(), after.atom_index()),
            (1, expected_ordinal)
        );
    }
    let after = moved(&mut session, CaretMove::Forward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (2, 0));

    // Backward from past the seam: the caret lands after every atom
    // anchored at the boundary it entered from the right.
    let after = moved(&mut session, CaretMove::Backward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (1, 2));
}

#[test]
fn to_end_lands_after_trailing_atoms() {
    let (document, paragraph) = document_with("ab", &[(2, "@A")]);
    let start = DocumentSelection::collapsed(seam_point(paragraph, 0, 0));
    let mut session = DocumentSession::new(document, start).unwrap();

    let after = moved(&mut session, CaretMove::ToEnd);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (2, 1));

    // ToStart from the seam keeps the leading-gap ordinal zero.
    let after = moved(&mut session, CaretMove::ToStart);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (0, 0));
}

#[test]
fn seam_caret_maps_through_atom_removal() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let atom = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()[0]
        .atom();
    let start = DocumentSelection::collapsed(seam_point(paragraph, 1, 1));
    let mut session = DocumentSession::new(document, start).unwrap();

    let transaction = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveInlineAtom { atom });
    session.apply(&transaction).unwrap();

    // The caret gap collapses onto the surviving seam gap.
    assert_eq!(caret(&session).atom_index(), 0);
    assert_eq!(caret(&session).text_offset(), offset_at(1));
}

#[test]
fn seam_caret_maps_through_atom_insertion_at_the_same_gap() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    // Caret between the two future gaps: insertion at (1, 0) shifts it.
    let start = DocumentSelection::collapsed(seam_point(paragraph, 1, 0));
    let mut session = DocumentSession::new(document, start).unwrap();

    let transaction = Transaction::new(TransactionOrigin::Extension("atom-test".into())).with_step(
        TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(paragraph, offset_at(1), 0, CursorAffinity::Before),
            kind: AtomKind::new("reference").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("ref").unwrap(),
        },
    );
    session.apply(&transaction).unwrap();

    assert_eq!(
        caret(&session),
        InlinePoint::new(paragraph, offset_at(1), 0, CursorAffinity::Before)
    );
}

#[test]
fn seam_selection_rejects_ordinals_beyond_the_document() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let invalid = DocumentSelection::collapsed(seam_point(paragraph, 1, 2));
    assert!(DocumentSession::new(document, invalid).is_err());
}
