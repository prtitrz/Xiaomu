//! P4.5 Inline Atom Integration Gate (P4A closeout).
//!
//! A realistic multi-block extension fixture -- several atom kinds, adjacent
//! same-boundary atoms, CJK / BiDi neighbors -- exercising the whole session
//! seam end-to-end: caret walking, seam typing, atom deletion, selection
//! replacement, IME commit boundaries, and undo/redo exactness.
//! Single-behavior seams are already pinned by `inline_atom_session.rs`; this
//! matrix proves they compose on realistic content.

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::session::{
    CaretMove, DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionError,
    SessionOutcome,
};

fn offset_in(text: &str, byte: usize) -> TextOffset {
    TextBuffer::from_string(text.to_owned())
        .offset_at(byte)
        .unwrap()
}

fn node_text(document: &XiaomuDocument, node: NodeId) -> String {
    document
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

/// Inserts one atom through the canonical transaction and returns the newly
/// allocated identity (found by diffing the parent's placements).
fn insert_atom(
    document: &mut XiaomuDocument,
    parent: NodeId,
    byte: usize,
    ordinal: usize,
    kind: &str,
    fallback: &str,
) -> NodeId {
    let before: Vec<NodeId> = document
        .node(parent)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .collect();
    let text = node_text(document, parent);
    *document = Transaction::new(TransactionOrigin::Extension("p4a-gate".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(
                parent,
                offset_in(&text, byte),
                ordinal,
                CursorAffinity::Before,
            ),
            kind: AtomKind::new(kind).unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new(fallback).unwrap(),
        })
        .apply(document)
        .unwrap();
    document
        .node(parent)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .find(|atom| !before.contains(atom))
        .unwrap()
}

struct ExtensionFixture {
    document: XiaomuDocument,
    /// Heading "周报".
    #[allow(dead_code)]
    heading: NodeId,
    /// Paragraph "A中B" with mention + reference anchored at byte 1
    /// (adjacent same-boundary atoms) and a cursor marker at byte 4.
    report: NodeId,
    report_atoms: [NodeId; 3],
    /// Paragraph "مرحبا" with one leading atom.
    bidi: NodeId,
    bidi_atom: NodeId,
}

/// Realistic multi-block extension fixture: a CJK heading, a report
/// paragraph mixing CJK scalars with adjacent mention/reference atoms and a
/// trailing cursor marker, and a BiDi paragraph with a leading atom.
fn full_fixture() -> ExtensionFixture {
    let mut builder = NodeStoreBuilder::new();
    let heading = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("周报", Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    let (report, _) = paragraph_only(&mut builder, "A中B");
    let (bidi, _) = paragraph_only(&mut builder, "مرحبا");
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, report, bidi]),
        )
        .unwrap();
    let mut document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let mention = insert_atom(&mut document, report, 1, 0, "mention", "«@alice»");
    let reference = insert_atom(&mut document, report, 1, 1, "reference", "«#42»");
    let marker = insert_atom(&mut document, report, 4, 0, "cursor", "▸");
    let bidi_atom = insert_atom(&mut document, bidi, 0, 0, "mention", "«@bob»");
    ExtensionFixture {
        document,
        heading,
        report,
        report_atoms: [mention, reference, marker],
        bidi,
        bidi_atom,
    }
}

fn paragraph_only(builder: &mut NodeStoreBuilder, text: &str) -> (NodeId, ()) {
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(text, Default::default()).unwrap()]).unwrap(),
            ),
        )
        .unwrap();
    (paragraph, ())
}

fn inline_point(
    document: &XiaomuDocument,
    node: NodeId,
    byte: usize,
    ordinal: usize,
) -> InlinePoint {
    InlinePoint::new(
        node,
        offset_in(&node_text(document, node), byte),
        ordinal,
        CursorAffinity::Before,
    )
}

fn run_session(
    document: XiaomuDocument,
    node: NodeId,
    byte: usize,
    ordinal: usize,
) -> DocumentSession {
    let point = inline_point(&document, node, byte, ordinal);
    DocumentSession::new(
        document,
        DocumentSelection::collapsed(DocumentPosition::Inline(point)),
    )
    .unwrap()
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

fn text_of(session: &DocumentSession, node: NodeId) -> String {
    node_text(session.document(), node)
}

/// `(atom, anchor byte)` pairs in canonical placement order.
fn atoms_of(session: &DocumentSession, node: NodeId) -> Vec<(NodeId, usize)> {
    session
        .document()
        .node(node)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .atoms()
        .iter()
        .map(|placement| (placement.atom(), placement.text_offset().as_usize()))
        .collect()
}

fn report_atoms(session: &DocumentSession, fixture: &ExtensionFixture) -> Vec<(NodeId, usize)> {
    fixture
        .report_atoms
        .iter()
        .filter_map(|atom| {
            atoms_of(session, fixture.report)
                .into_iter()
                .find(|(candidate, _)| candidate == atom)
        })
        .collect()
}

#[test]
fn unicode_walk_crosses_cjk_scalars_and_adjacent_atom_seams() {
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);

    // "A" | «@alice» | «#42» | "中" | ▸ | "B": one caret unit each.
    let expected = [
        (1, 0), // before «@alice»
        (1, 1), // between the two adjacent atoms
        (1, 2), // after both, before 中
        (4, 0), // after 中, before ▸
        (4, 1), // after ▸, before B
        (5, 0), // paragraph end
    ];
    for (byte, ordinal) in expected {
        let after = moved(&mut session, CaretMove::Forward);
        assert_eq!(
            (after.text_offset().as_usize(), after.atom_index()),
            (byte, ordinal),
        );
    }

    // Backward re-walks the same units in reverse order (the caret is
    // already at the last walked position).
    for (byte, ordinal) in expected[..expected.len() - 1].iter().rev() {
        let after = moved(&mut session, CaretMove::Backward);
        assert_eq!(
            (after.text_offset().as_usize(), after.atom_index()),
            (*byte, *ordinal),
        );
    }
    let after = moved(&mut session, CaretMove::Backward);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (0, 0),);
}

#[test]
fn to_end_and_to_start_bracket_the_adjacent_seams() {
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);

    let after = moved(&mut session, CaretMove::ToEnd);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (5, 0),);
    let after = moved(&mut session, CaretMove::ToStart);
    assert_eq!((after.text_offset().as_usize(), after.atom_index()), (0, 0),);
}

#[test]
fn typing_into_adjacent_seam_gaps_reanchors_atoms_in_order() {
    // ordinal 0: insertion before both atoms moves them after the text.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 1, 0);
    session
        .apply_intent(&EditIntent::InsertText {
            text: "X".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "AX中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 2),
            (fixture.report_atoms[1], 2),
            (fixture.report_atoms[2], 5)
        ],
    );
    // Undo restores the exact canonical placement.
    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 4)
        ],
    );

    // ordinal 1: insertion between the atoms splits the seam in two.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 1, 1);
    session
        .apply_intent(&EditIntent::InsertText {
            text: "X".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "AX中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 2),
            (fixture.report_atoms[2], 5)
        ],
    );
    session.undo().unwrap();
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 4)
        ],
    );

    // ordinal 2: insertion after both atoms keeps them anchored at 1.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 1, 2);
    session
        .apply_intent(&EditIntent::InsertText {
            text: "X".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "AX中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 5)
        ],
    );
}

#[test]
fn backspace_and_delete_strip_exactly_one_caret_unit() {
    // Backspace right after the cursor marker removes only that atom.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 4, 1);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![(fixture.report_atoms[0], 1), (fixture.report_atoms[1], 1)],
    );
    // Undo restores the removed atom; redo re-removes it.
    session.undo().unwrap();
    assert_eq!(report_atoms(&session, &fixture).len(), 3);
    session.redo().unwrap();
    assert_eq!(report_atoms(&session, &fixture).len(), 2);

    // Backspace before the CJK scalar deletes the whole scalar, not one
    // byte, and shifts the cursor marker accordingly.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 4, 0);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, fixture.report), "AB");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 1)
        ],
    );

    // Delete before the adjacent seam removes only the first atom.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 1, 0);
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![(fixture.report_atoms[1], 1), (fixture.report_atoms[2], 4)],
    );
}

#[test]
fn selection_spanning_cjk_and_adjacent_atoms_replaces_as_one_region() {
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);
    session
        .set_inline_selection(
            inline_point(session.document(), fixture.report, 0, 0),
            inline_point(session.document(), fixture.report, 4, 0),
        )
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText {
            text: "好".to_owned(),
        })
        .unwrap();

    // "A" + both adjacent atoms + "中" are replaced as one region; the
    // cursor marker at the end gap survives and re-anchors to the insert.
    assert_eq!(text_of(&session, fixture.report), "好B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![(fixture.report_atoms[2], 3)],
    );

    // One undo restores text and all three atom anchors exactly.
    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中B");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 4)
        ],
    );
}

#[test]
fn selection_of_adjacent_atoms_alone_replaces_without_touching_text() {
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 1, 0);
    session
        .set_inline_selection(
            inline_point(session.document(), fixture.report, 1, 0),
            inline_point(session.document(), fixture.report, 1, 2),
        )
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText {
            text: "群".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "A群中B");
    // The cursor marker shifts by the inserted UTF-8 length (3 bytes).
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![(fixture.report_atoms[2], 7)],
    );
}

#[test]
fn ime_commit_boundary_matrix_keeps_boundary_atoms_and_spans_fail_closed() {
    // A commit range starting exactly at the adjacent seam keeps both atoms
    // (they sit before the start gap) and replaces only "中".
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);
    let range = TextBuffer::from_string(text_of(&session, fixture.report))
        .range(offset_in("A中B", 1), offset_in("A中B", 4))
        .unwrap();
    session
        .apply_intent(&EditIntent::CommitComposition {
            range,
            text: "你好😀".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "A你好😀B");
    // 你好😀 is 10 bytes; "中" was 3, so trailing anchors shift by +7.
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 11)
        ],
    );
    // IME commits own one isolated history entry.
    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中B");

    // A commit range whose start gap is after the cursor marker keeps the
    // marker anchored while the replaced scalar shifts nothing else; the
    // range must not span the byte-1 anchors, which sit strictly inside.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);
    let range = TextBuffer::from_string(text_of(&session, fixture.report))
        .range(offset_in("A中B", 4), offset_in("A中B", 5))
        .unwrap();
    session
        .apply_intent(&EditIntent::CommitComposition {
            range,
            text: "尾".to_owned(),
        })
        .unwrap();
    assert_eq!(text_of(&session, fixture.report), "A中尾");
    assert_eq!(
        report_atoms(&session, &fixture),
        vec![
            (fixture.report_atoms[0], 1),
            (fixture.report_atoms[1], 1),
            (fixture.report_atoms[2], 4)
        ],
    );

    // A range spanning the byte-1 anchors strictly inside it is rejected
    // atomically: text-only IME contracts cannot address the seam.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.report, 0, 0);
    let range = TextBuffer::from_string(text_of(&session, fixture.report))
        .range(offset_in("A中B", 0), offset_in("A中B", 4))
        .unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::CommitComposition {
            range,
            text: "X".to_owned(),
        }),
        Err(SessionError::Core(xiaomu_core::Error::InvalidTransaction)),
    );
    assert_eq!(text_of(&session, fixture.report), "A中B");
    assert_eq!(report_atoms(&session, &fixture).len(), 3);
}

#[test]
fn bidi_paragraph_edits_keep_the_leading_atom_and_scalar_boundaries() {
    // Typing after the leading atom keeps it anchored at byte 0.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.bidi, 0, 1);
    session
        .apply_intent(&EditIntent::InsertText {
            text: "↩".to_owned(),
        })
        .unwrap();
    assert_eq!(
        text_of(&session, fixture.bidi),
        "\u{21a9}\u{645}\u{631}\u{62d}\u{628}\u{627}"
    );
    assert_eq!(
        atoms_of(&session, fixture.bidi),
        vec![(fixture.bidi_atom, 0)],
    );

    // Backspace right after the inserted scalar deletes that scalar, not
    // the atom or a BiDi letter.
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, fixture.bidi), "مرحبا");
    assert_eq!(
        atoms_of(&session, fixture.bidi),
        vec![(fixture.bidi_atom, 0)],
    );
    session.undo().unwrap();
    assert_eq!(text_of(&session, fixture.bidi), "↩مرحبا");

    // Backspace inside the BiDi word deletes exactly one scalar per step.
    let fixture = full_fixture();
    let mut session = run_session(fixture.document.clone(), fixture.bidi, 2, 0);
    session.apply_intent(&EditIntent::Backspace).unwrap();
    assert_eq!(text_of(&session, fixture.bidi).chars().count(), 4);
    assert_eq!(
        atoms_of(&session, fixture.bidi),
        vec![(fixture.bidi_atom, 0)],
    );
}
