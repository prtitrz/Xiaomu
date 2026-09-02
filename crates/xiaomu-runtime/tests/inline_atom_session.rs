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

fn session_with_caret(
    document: XiaomuDocument,
    paragraph: NodeId,
    raw: usize,
    ordinal: usize,
) -> DocumentSession {
    let start = DocumentSelection::collapsed(seam_point(paragraph, raw, ordinal));
    DocumentSession::new(document, start).unwrap()
}

fn typed(session: &mut DocumentSession, text: &str) {
    session
        .apply_intent(&EditIntent::InsertText {
            text: text.to_owned(),
        })
        .unwrap();
}

fn text_of(session: &DocumentSession, paragraph: NodeId) -> String {
    session
        .document()
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

fn atom_count(session: &DocumentSession, paragraph: NodeId) -> usize {
    session
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .len()
}

#[test]
fn typing_at_seam_lands_on_the_chosen_side_of_the_atom() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);

    // (1,0) + X → A X [atom] B, caret after the typed text.
    let mut session = session_with_caret(document.clone(), paragraph, 1, 0);
    typed(&mut session, "X");
    assert_eq!(text_of(&session, paragraph), "aXb");
    assert_eq!(atom_count(&session, paragraph), 1);
    assert_eq!(
        caret(&session).text_offset().as_usize(),
        2,
        "caret lands after the typed text, before the shifted atom"
    );

    // (1,1) + X → A [atom] X B.
    let mut session = session_with_caret(document, paragraph, 1, 1);
    typed(&mut session, "X");
    assert_eq!(text_of(&session, paragraph), "aXb");
    let atom_offset = session
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()[0]
        .text_offset()
        .as_usize();
    assert_eq!(atom_offset, 1, "the atom keeps its anchor");
    assert_eq!(caret(&session).text_offset().as_usize(), 2);
}

#[test]
fn typing_between_adjacent_atoms_splits_the_seam() {
    let (document, paragraph) = document_with("ab", &[(1, "@A"), (1, "@B")]);
    let mut session = session_with_caret(document, paragraph, 1, 1);
    typed(&mut session, "X");

    assert_eq!(text_of(&session, paragraph), "aXb");
    let inline = session
        .document()
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let offsets: Vec<usize> = inline
        .atoms()
        .iter()
        .map(|placement| placement.text_offset().as_usize())
        .collect();
    assert_eq!(offsets, [1, 2], "later seam atoms move after the text");
    assert_eq!(caret(&session).text_offset().as_usize(), 2);
}

#[test]
fn backspace_after_an_atom_removes_the_atom() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let mut session = session_with_caret(document, paragraph, 1, 1);
    session.apply_intent(&EditIntent::Backspace).unwrap();

    assert_eq!(text_of(&session, paragraph), "ab");
    assert_eq!(atom_count(&session, paragraph), 0);
    assert_eq!(
        (
            caret(&session).text_offset().as_usize(),
            caret(&session).atom_index()
        ),
        (1, 0)
    );

    // Undo restores the atom and keeps a valid caret.
    session.undo().unwrap();
    assert_eq!(atom_count(&session, paragraph), 1);
    assert_eq!(
        session.selection().focus(),
        seam_point(paragraph, 1, 1),
        "undo reinstates the seam caret"
    );
    session.redo().unwrap();
    assert_eq!(atom_count(&session, paragraph), 0);
}

#[test]
fn backspace_before_an_atom_deletes_only_the_previous_scalar() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let mut session = session_with_caret(document, paragraph, 1, 0);
    session.apply_intent(&EditIntent::Backspace).unwrap();

    assert_eq!(text_of(&session, paragraph), "b");
    assert_eq!(atom_count(&session, paragraph), 1);
    // The caret stays before the atom, which shifted to the boundary.
    assert_eq!(
        (
            caret(&session).text_offset().as_usize(),
            caret(&session).atom_index()
        ),
        (0, 0)
    );
}

#[test]
fn backspace_past_seam_atoms_lands_after_them() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    // Caret after "b": Backspace deletes "b"; the caret must land after the
    // atom, not before it.
    let mut session = session_with_caret(document, paragraph, 2, 0);
    session.apply_intent(&EditIntent::Backspace).unwrap();

    assert_eq!(text_of(&session, paragraph), "a");
    assert_eq!(atom_count(&session, paragraph), 1);
    assert_eq!(
        (
            caret(&session).text_offset().as_usize(),
            caret(&session).atom_index()
        ),
        (1, 1)
    );
}

#[test]
fn delete_removes_the_following_atom_or_scalar_atomically() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);

    // Delete before the atom removes the atom itself.
    let mut session = session_with_caret(document.clone(), paragraph, 1, 0);
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(text_of(&session, paragraph), "ab");
    assert_eq!(atom_count(&session, paragraph), 0);
    assert_eq!(caret(&session).atom_index(), 0);

    // Delete after the atom removes the following scalar and keeps the atom.
    let mut session = session_with_caret(document, paragraph, 1, 1);
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(text_of(&session, paragraph), "a");
    assert_eq!(atom_count(&session, paragraph), 1);
    assert_eq!(
        (
            caret(&session).text_offset().as_usize(),
            caret(&session).atom_index()
        ),
        (1, 1)
    );
}

#[test]
fn typing_over_a_selection_spanning_atoms_deletes_them_explicitly() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let selection =
        DocumentSelection::new(seam_point(paragraph, 1, 0), seam_point(paragraph, 2, 0));
    let mut session = DocumentSession::new(document, selection).unwrap();
    typed(&mut session, "X");

    assert_eq!(text_of(&session, paragraph), "aX");
    assert_eq!(atom_count(&session, paragraph), 0);
    assert_eq!(caret(&session).text_offset().as_usize(), 2);

    // The undo of the composed removal+replacement restores everything.
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "ab");
    assert_eq!(atom_count(&session, paragraph), 1);
}

#[test]
fn selecting_an_atom_alone_and_typing_replaces_it() {
    let (document, paragraph) = document_with("ab", &[(1, "@A"), (1, "@B")]);
    // Selection from before @A to between @B and "b": both atoms inside.
    let selection =
        DocumentSelection::new(seam_point(paragraph, 1, 0), seam_point(paragraph, 1, 2));
    let mut session = DocumentSession::new(document, selection).unwrap();
    typed(&mut session, "X");

    assert_eq!(text_of(&session, paragraph), "aXb");
    assert_eq!(atom_count(&session, paragraph), 0);
}

#[test]
fn typing_at_seam_coalesces_into_the_typing_group() {
    let (document, paragraph) = document_with("ab", &[(1, "@A")]);
    let mut session = session_with_caret(document, paragraph, 1, 0);
    typed(&mut session, "X");
    typed(&mut session, "Y");
    // One undo reverts both keystrokes of the typing group.
    session.undo().unwrap();
    assert_eq!(text_of(&session, paragraph), "ab");
    assert_eq!(atom_count(&session, paragraph), 1);
}
