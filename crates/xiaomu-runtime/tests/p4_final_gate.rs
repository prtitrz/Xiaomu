//! P4.9 final integration gate: Unicode + inline atom + atomic block matrix.
//!
//! Closes the phase with the combinations the earlier slices built in
//! isolation: mixed CJK/emoji text with inline atoms next to atomic blocks,
//! atomic removal / clipboard / undo integrity, and multi-editor isolation.

use xiaomu_core::document::{
    AtomKind, ImageAttrs, ImageSource, InlineAtomContent, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, NodeGap};
use xiaomu_core::text::TextBuffer;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::{decode_metadata, encode_metadata};
use xiaomu_runtime::session::{
    DocumentPosition, DocumentSelection, DocumentSession, EditIntent, SessionOutcome,
};

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

fn offset_in(text: &str, byte: usize) -> xiaomu_core::text::TextOffset {
    TextBuffer::from_string(text.to_owned())
        .offset_at(byte)
        .unwrap()
}

/// Inserts one atom through the canonical transaction and returns the new id.
fn insert_atom(
    document: &mut XiaomuDocument,
    parent: NodeId,
    byte: usize,
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
    *document = Transaction::new(TransactionOrigin::Extension("p4-final-gate".into()))
        .with_step(TransactionStep::InsertInlineAtom {
            at: InlinePoint::new(parent, offset_in(&text, byte), 0, CursorAffinity::Before),
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

/// Builds `Document > [p(text), HorizontalRule, p("مرحبا tail")]` with one
/// mention atom anchored inside the first paragraph at `atom_byte`.
fn media_fixture(atom_byte: usize) -> (XiaomuDocument, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let first = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("会议📅纪要", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let last = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new("مرحبا tail", MarkSet::empty()).unwrap()])
                    .unwrap(),
            ),
        )
        .unwrap();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([first, rule, last]),
        )
        .unwrap();
    let mut document = XiaomuDocument::new(root, builder.finish()).unwrap();
    let atom = insert_atom(&mut document, first, atom_byte, "mention", "«@晓木»");
    (document, first, atom, rule, last)
}

fn caret(document: &XiaomuDocument, node: NodeId, byte: usize, ordinal: usize) -> DocumentPosition {
    DocumentPosition::Inline(InlinePoint::new(
        node,
        offset_in(&node_text(document, node), byte),
        ordinal,
        CursorAffinity::Before,
    ))
}

/// A stable structural fingerprint for equality checks across sessions.
fn fingerprint(document: &XiaomuDocument) -> String {
    fn walk(document: &XiaomuDocument, id: NodeId, depth: usize, out: &mut String) {
        let node = document.node(id).unwrap();
        out.push_str(&"  ".repeat(depth));
        match node.content() {
            NodeContent::Inline(inline) => {
                out.push_str(&format!(
                    "{:?} text={:?} atoms={:?}\n",
                    node.kind(),
                    inline
                        .runs()
                        .iter()
                        .map(|run| run.text().as_str())
                        .collect::<String>(),
                    inline
                        .atoms()
                        .iter()
                        .map(|placement| document
                            .node(placement.atom())
                            .unwrap()
                            .content()
                            .as_inline_atom()
                            .unwrap()
                            .fallback_text()
                            .to_owned())
                        .collect::<Vec<_>>(),
                ));
            }
            NodeContent::Children(children) => {
                out.push_str(&format!("{:?}\n", node.kind()));
                for &child in children {
                    walk(document, child, depth + 1, out);
                }
            }
            NodeContent::Atomic => {
                out.push_str(&format!(
                    "{:?} attrs={:?}\n",
                    node.kind(),
                    node.attrs().iter().collect::<Vec<_>>()
                ));
            }
            _ => out.push_str(&format!("{:?}\n", node.kind())),
        }
    }
    let mut out = String::new();
    walk(document, document.root(), 0, &mut out);
    out
}

#[test]
fn unicode_atom_atomic_matrix_survives_the_full_edit_cycle() {
    // "会议📅" is 10 UTF-8 bytes; the atom anchors between 📅 and 纪要.
    let (document, first, _atom, rule, last) = media_fixture(10);
    let mut session = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(caret(&document, first, 10, 1)),
    )
    .unwrap();

    // Typing after an atom keeps the atom anchored before the new text and
    // consumes exactly one caret unit across the CJK/emoji mix.
    assert_eq!(
        session
            .apply_intent(&EditIntent::InsertText {
                text: "✅".to_owned()
            })
            .unwrap(),
        SessionOutcome::DocumentChanged
    );
    let text = node_text(session.document(), first);
    assert_eq!(text, "会议📅✅纪要");
    let after_typing = fingerprint(session.document());
    let inline = session
        .document()
        .node(first)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    assert_eq!(
        inline.atoms()[0].text_offset(),
        offset_in(&text, 10),
        "atom anchor must not move"
    );

    // The atomic block removes as one logical change and undoes into a whole
    // node selection.
    session.set_atomic_selection(rule).unwrap();
    assert_eq!(
        session.apply_intent(&EditIntent::Backspace).unwrap(),
        SessionOutcome::DocumentChanged
    );
    let shrunk = session.document().clone();
    assert!(shrunk.node(rule).is_none(), "rule removed");
    match session.selection().focus() {
        DocumentPosition::Gap(gap) => {
            assert_eq!(gap.parent(), shrunk.root());
        }
        other => panic!("caret must converge to a gap after atomic removal: {other:?}"),
    }

    session.undo().unwrap();
    assert_eq!(
        fingerprint(session.document()),
        after_typing,
        "undo restores the pre-removal state, not the session origin"
    );
    assert_eq!(
        session.selection().focus(),
        DocumentPosition::Atomic(rule),
        "undo reinstates the atomic node selection"
    );
    session.undo().unwrap();
    assert_eq!(fingerprint(session.document()), fingerprint(&document));

    // The clipboard wire carries the atomic block losslessly.
    session.set_atomic_selection(rule).unwrap();
    let slice = session.clipboard_slice().unwrap().expect("atomic fragment");
    let metadata = encode_metadata(&slice).unwrap();
    let decoded = decode_metadata("", &metadata).unwrap();
    assert_eq!(decoded.roots().len(), 1);
    assert!(matches!(
        decoded.roots()[0].kind(),
        NodeKind::HorizontalRule
    ));

    // Pasting into the other paragraph inserts a sibling block after it.
    let mut target = DocumentSession::new(
        document.clone(),
        DocumentSelection::collapsed(caret(&document, last, 10, 0)),
    )
    .unwrap();
    target
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    let pasted = fingerprint(target.document());
    assert!(pasted.contains("HorizontalRule"), "rule pasted");
    assert_eq!(
        target.selection().focus(),
        caret(target.document(), last, 10, 0),
        "paste keeps the caret in place"
    );

    // The typed image command inserts an Image atomic block as a sibling of
    // the focused block and undoes cleanly.
    let image = ImageAttrs::new(
        ImageSource::ExternalUrl("https://example.invalid/cover.png".to_owned()),
        "封面".to_owned(),
        None,
        None,
        None,
    )
    .unwrap();
    target
        .apply_intent(&EditIntent::InsertImage {
            image: image.clone(),
        })
        .unwrap();
    let grew = target.document();
    let image_node = grew
        .node(grew.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .iter()
        .copied()
        .find(|id| matches!(grew.node(*id).unwrap().kind(), NodeKind::Image))
        .expect("image inserted");
    assert_eq!(
        ImageAttrs::from_attrs(grew.node(image_node).unwrap().attrs()).unwrap(),
        image,
        "canonical attrs carry the typed image semantics"
    );
    target.undo().unwrap();
    assert_eq!(fingerprint(target.document()), pasted);
}

#[test]
fn multi_editor_sessions_stay_isolated_with_atomic_blocks() {
    let (document_a, first_a, _, rule_a, _) = media_fixture(10);
    let (document_b, first_b, _, rule_b, _) = media_fixture(6);
    let mut editor_a = DocumentSession::new(
        document_a.clone(),
        DocumentSelection::collapsed(caret(&document_a, first_a, 0, 0)),
    )
    .unwrap();
    let editor_b = DocumentSession::new(
        document_b.clone(),
        DocumentSelection::collapsed(caret(&document_b, first_b, 0, 0)),
    )
    .unwrap();

    editor_a
        .apply_intent(&EditIntent::InsertText {
            text: "甲".to_owned(),
        })
        .unwrap();
    editor_a.set_atomic_selection(rule_a).unwrap();
    editor_a.apply_intent(&EditIntent::Backspace).unwrap();

    // Editor B is untouched: its own snapshot still has the rule and its
    // own fingerprint never moved.
    assert!(editor_b.document().node(rule_b).is_some());
    assert_eq!(fingerprint(editor_b.document()), fingerprint(&document_b));
    let edited = fingerprint(editor_a.document());
    assert!(edited.contains("甲会议📅纪要"));
    assert!(!edited.contains("HorizontalRule"), "rule removed in A");

    editor_a.undo().unwrap();
    editor_a.undo().unwrap();
    assert_eq!(fingerprint(editor_a.document()), fingerprint(&document_a));
    assert!(editor_a.document().node(rule_a).is_some());
    // B never changed across the whole exchange.
    assert_eq!(fingerprint(editor_b.document()), fingerprint(&document_b));
}

#[test]
fn gap_positions_map_through_atomic_removal_like_other_positions() {
    let (document, _, _, rule, _) = media_fixture(10);
    let gap = DocumentPosition::Gap(NodeGap::new(document.root(), 2));
    let DocumentPosition::Gap(node_gap) = gap else {
        panic!("gap position");
    };
    assert!(node_gap.validate(&document).is_ok());

    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::RemoveNode { node: rule })
        .apply_with_changes(&document)
        .unwrap();
    let mapped = DocumentSelection::collapsed(gap)
        .map_through(removed.changes(), &document)
        .unwrap()
        .focus();
    match mapped {
        DocumentPosition::Gap(gap) => {
            assert_eq!(gap.parent(), removed.document().root());
            assert_eq!(gap.index(), 1, "gap follows the removed rule");
        }
        other => panic!("gap must map to a gap: {other:?}"),
    }
}
