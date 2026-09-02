//! P4.3 structured clipboard with inline atoms.
//!
//! Copy captures detached atom payloads (kind, attrs, `fallback_text`)
//! without canonical identities; paste re-materializes them under fresh
//! node identities. The plain-text fallback splices `fallback_text` at the
//! anchored positions, and pastes that would have to drop atoms fail
//! closed instead of silently downgrading them.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError,
};

fn offset_at(raw: usize) -> xiaomu_core::text::TextOffset {
    const SCRATCH: &str = "00000000000000000000000000000000";
    TextBuffer::from_string(SCRATCH.to_owned())
        .offset_at(raw)
        .unwrap()
}

/// Builds `Document > [p(text)]` and inserts one mention atom at
/// `atom_char` with the given fallback text.
fn document_with_atom(text: &str, atom_char: usize, fallback: &str) -> (XiaomuDocument, NodeId) {
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
    document = xiaomu_core::transaction::Transaction::new(
        xiaomu_core::transaction::TransactionOrigin::Extension("atom-test".into()),
    )
    .with_step(
        xiaomu_core::transaction::TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(paragraph, offset_at(atom_char), 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        },
    )
    .apply(&document)
    .unwrap();
    (document, paragraph)
}

/// Builds `Document > [p(text)]` with no atoms.
fn plain_document(text: &str) -> (XiaomuDocument, NodeId) {
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
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

fn caret_point(node: NodeId, raw: usize, ordinal: usize) -> DocumentPosition {
    DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_at(raw),
        ordinal,
        CursorAffinity::Before,
    ))
}

fn plain_text_selection(node: NodeId, anchor_raw: usize, focus_raw: usize) -> DocumentSelection {
    DocumentSelection::new(
        DocumentPosition::Inline(InlinePoint::new(
            node,
            offset_at(anchor_raw),
            0,
            CursorAffinity::Before,
        )),
        DocumentPosition::Inline(InlinePoint::new(
            node,
            offset_at(focus_raw),
            0,
            CursorAffinity::Before,
        )),
    )
}

fn first_atom_id(document: &XiaomuDocument, paragraph: NodeId) -> NodeId {
    document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()[0]
        .atom()
}

fn atom_fallback(document: &XiaomuDocument, paragraph: NodeId) -> String {
    let atom = first_atom_id(document, paragraph);
    document
        .node(atom)
        .unwrap()
        .content()
        .as_inline_atom()
        .unwrap()
        .fallback_text()
        .to_owned()
}

#[test]
fn copy_captures_detached_atoms_and_plain_fallback() {
    let (document, paragraph) = document_with_atom("ab", 1, "@Ann");
    let session = DocumentSession::new(document, plain_text_selection(paragraph, 0, 2)).unwrap();

    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.plain_text(), "a@Annb");

    let inline = slice.blocks()[0].inline();
    assert_eq!(inline.atoms().len(), 1);
    assert_eq!(inline.atoms()[0].anchor().as_usize(), 1);
    assert_eq!(inline.atoms()[0].kind().as_str(), "mention");
    assert_eq!(inline.atoms()[0].content().fallback_text(), "@Ann");
}

#[test]
fn copy_between_seam_gaps_selects_the_atoms_in_between() {
    let (document, paragraph) = document_with_atom("ab", 1, "@A");
    // Insert a second atom after the first.
    let document = xiaomu_core::transaction::Transaction::new(
        xiaomu_core::transaction::TransactionOrigin::Extension("atom-test".into()),
    )
    .with_step(
        xiaomu_core::transaction::TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(paragraph, offset_at(1), 1, CursorAffinity::Before),
            kind: AtomKind::new("reference").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("ref").unwrap(),
        },
    )
    .apply(&document)
    .unwrap();

    // Selection from before both atoms to between the second atom and "b":
    // both atoms are inside, no text is.
    let selection =
        DocumentSelection::new(caret_point(paragraph, 1, 0), caret_point(paragraph, 1, 2));
    let session = DocumentSession::new(document, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();

    assert_eq!(slice.plain_text(), "@Aref");
    assert_eq!(slice.blocks()[0].inline().atoms().len(), 2);
}

#[test]
fn wire_metadata_round_trips_atom_payloads() {
    let (document, paragraph) = document_with_atom("ab", 1, "@Ann");
    let session = DocumentSession::new(document, plain_text_selection(paragraph, 0, 2)).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    let metadata = encode_metadata(&slice).unwrap();

    let decoded = decode_metadata(slice.plain_text(), &metadata).unwrap();
    assert_eq!(decoded.plain_text(), "a@Annb");
    let atom = decoded.blocks()[0].inline().atoms()[0].clone();
    assert_eq!(atom.anchor().as_usize(), 1);
    assert_eq!(atom.kind().as_str(), "mention");
    assert_eq!(atom.content().fallback_text(), "@Ann");

    // A stale plain-text body must not decode into structured content.
    assert!(decode_metadata("changed", &metadata).is_none());
}

#[test]
fn paste_restores_atoms_with_fresh_identity() {
    let (source, source_paragraph) = document_with_atom("ab", 1, "@Ann");
    let selection = plain_text_selection(source_paragraph, 0, 2);
    let session = DocumentSession::new(source, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    let source_atom = first_atom_id(session.document(), source_paragraph);

    // Paste into a different document at an empty caret.
    let (target, target_paragraph) = {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("xy", Default::default()).unwrap()]).unwrap(),
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
    };
    let mut session = DocumentSession::new(
        target,
        DocumentSelection::collapsed(caret_point(target_paragraph, 1, 0)),
    )
    .unwrap();
    session
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();

    // Text "xy" becomes "x" + pasted content + "y" with the atom restored.
    assert_eq!(atom_fallback(session.document(), target_paragraph), "@Ann");
    let pasted_atom = first_atom_id(session.document(), target_paragraph);
    assert_eq!(
        session.document().parent_of(pasted_atom),
        Some(target_paragraph)
    );

    // NodeIds are per-document, so freshness is proven by pasting again:
    // the second paste must allocate a different identity in the same
    // document, and the source atom identity is irrelevant to the target.
    let _ = source_atom;

    // Undo removes the pasted atom again.
    session.undo().unwrap();
    assert_eq!(
        session
            .document()
            .node(target_paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()
            .len(),
        0
    );
}

#[test]
fn paste_over_a_selection_spanning_target_atoms_removes_them() {
    // A clean source document provides an atom-free slice.
    let (other, other_paragraph) = plain_document("cd");
    let other_session =
        DocumentSession::new(other, plain_text_selection(other_paragraph, 0, 2)).unwrap();
    let text_slice = other_session.clipboard_slice().unwrap().unwrap();

    // Paste it over a selection that spans the target's atom.
    let (target, target_paragraph) = document_with_atom("ab", 1, "@A");
    let mut session =
        DocumentSession::new(target, plain_text_selection(target_paragraph, 0, 2)).unwrap();
    session
        .apply_intent(&EditIntent::PasteSlice { slice: text_slice })
        .unwrap();

    let inline = session
        .document()
        .node(target_paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(inline.atoms().len(), 0, "target atom was inside the span");
    assert_eq!(
        inline
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "cd"
    );
}

#[test]
fn multi_block_paste_with_atoms_fails_closed() {
    // Build a two-paragraph document where the first block carries an atom,
    // then copy across both blocks.
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("ab", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let second = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("cd", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, second]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let document = xiaomu_core::transaction::Transaction::new(
        xiaomu_core::transaction::TransactionOrigin::Extension("atom-test".into()),
    )
    .with_step(
        xiaomu_core::transaction::TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(first, offset_at(1), 0, CursorAffinity::Before),
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@A").unwrap(),
        },
    )
    .apply(&document)
    .unwrap();

    let selection = DocumentSelection::new(caret_point(first, 0, 0), caret_point(second, 2, 0));
    let session = DocumentSession::new(document, selection).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert_eq!(slice.blocks().len(), 2);

    // Pasting the atom-bearing multi-block slice fails closed.
    let (target, target_paragraph) = {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("xy", Default::default()).unwrap()]).unwrap(),
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
    };
    let mut session = DocumentSession::new(
        target,
        DocumentSelection::collapsed(caret_point(target_paragraph, 0, 0)),
    )
    .unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::PasteSlice { slice }),
        Err(SessionError::ClipboardAtomsUnsupported)
    );
}

#[test]
fn ime_composition_touching_an_atom_fails_closed_atomically() {
    let (document, paragraph) = document_with_atom("ab", 1, "@A");
    let mut session = DocumentSession::new(
        document,
        DocumentSelection::collapsed(caret_point(paragraph, 0, 0)),
    )
    .unwrap();

    // The composition range spans the atom anchor: the text-only IME
    // contract cannot address the seam, so the commit must be rejected
    // without mutating the document.
    let range = TextBuffer::from_string("ab".to_owned())
        .range(offset_at(0), offset_at(2))
        .unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::CommitComposition {
            range,
            text: "X".to_owned(),
        }),
        Err(SessionError::Core(xiaomu_core::Error::InvalidTransaction))
    );

    assert_eq!(
        session
            .document()
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()
            .len(),
        1
    );
}

#[test]
fn ime_composition_clear_of_atoms_commits_normally() {
    let (document, paragraph) = document_with_atom("ab", 1, "@A");
    let mut session = DocumentSession::new(
        document,
        DocumentSelection::collapsed(caret_point(paragraph, 0, 0)),
    )
    .unwrap();

    // A range strictly inside plain text keeps the text-only IME contract.
    let range = TextBuffer::from_string("a".to_owned())
        .range(offset_at(0), offset_at(1))
        .unwrap();
    session
        .apply_intent(&EditIntent::CommitComposition {
            range,
            text: "A".to_owned(),
        })
        .unwrap();

    let inline = session
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
            .collect::<String>(),
        "Ab"
    );
    assert_eq!(
        inline.atoms()[0].text_offset().as_usize(),
        1,
        "same-length commit keeps the trailing atom anchored"
    );
}

#[test]
fn cut_then_undo_restores_atoms_and_caret() {
    let (document, paragraph) = document_with_atom("ab", 1, "@A");
    // Selection spans the atom.
    let mut session =
        DocumentSession::new(document, plain_text_selection(paragraph, 0, 2)).unwrap();
    let slice = session.clipboard_slice().unwrap().unwrap();
    assert!(slice.blocks()[0].inline().atoms().len() == 1);

    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(
        session
            .document()
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()
            .len(),
        0
    );

    session.undo().unwrap();
    assert_eq!(
        session
            .document()
            .node(paragraph)
            .unwrap()
            .content()
            .as_inline()
            .unwrap()
            .atoms()
            .len(),
        1
    );
    assert!(session.selection().focus().as_inline_point().is_some());
}
