//! TextStyle participates in ordinary typing, exact reconstruction and history.

use std::cell::Cell;
use std::rc::Rc;
use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineContent, Mark, MarkKind, MarkSet, NodeAttrs, NodeContent,
    NodeId, NodeKind, NodeStoreBuilder, StringAttribute, TextRun, TextStyleAttributes,
    TextStyleMark, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint, TextPoint};
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};
use xiaomu_runtime::clipboard::ClipboardSlice;
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentSelection, DocumentSession, EditIntent, PolicyError,
    SessionPolicy,
};

fn style(color: StringAttribute, family: StringAttribute, size: StringAttribute) -> Mark {
    Mark::TextStyle(TextStyleMark::from_attributes(
        TextStyleAttributes::default()
            .with_color(color)
            .with_font_family(family)
            .with_font_size(size),
    ))
}

fn red() -> Mark {
    style(
        "red".into(),
        StringAttribute::Null,
        StringAttribute::Missing,
    )
}
fn run(text: &str, marks: Vec<Mark>) -> TextRun {
    TextRun::new(text, MarkSet::new(marks).unwrap()).unwrap()
}

fn document(blocks: Vec<Vec<TextRun>>) -> (XiaomuDocument, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let nodes: Vec<_> = blocks
        .into_iter()
        .map(|runs| {
            builder
                .insert(
                    NodeKind::Paragraph,
                    NodeAttrs::empty(),
                    NodeContent::Inline(InlineContent::new(runs).unwrap()),
                )
                .unwrap()
        })
        .collect();
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(nodes.iter().copied()),
        )
        .unwrap();
    (XiaomuDocument::new(root, builder.finish()).unwrap(), nodes)
}

fn inline(doc: &XiaomuDocument, node: NodeId) -> &InlineContent {
    doc.node(node).unwrap().content().as_inline().unwrap()
}
fn point(doc: &XiaomuDocument, node: NodeId, byte: usize) -> TextPoint {
    TextPoint::new(
        node,
        inline(doc, node).offset_at(byte).unwrap(),
        CursorAffinity::Before,
    )
}
fn marks_at(doc: &XiaomuDocument, node: NodeId, byte: usize) -> MarkSet {
    let mut end = 0;
    for run in inline(doc, node).runs() {
        end += run.len_bytes();
        if byte < end {
            return run.marks().clone();
        }
    }
    panic!("byte outside text");
}
fn copied(runs: Vec<TextRun>) -> ClipboardSlice {
    let (doc, nodes) = document(vec![runs]);
    let node = nodes[0];
    let selection = DocumentSelection::new(
        point(&doc, node, 0),
        point(&doc, node, inline(&doc, node).len_bytes()),
    );
    DocumentSession::new(doc, selection)
        .unwrap()
        .clipboard_slice()
        .unwrap()
        .unwrap()
}

#[test]
fn collapsed_set_and_remove_keep_exact_pending_marks_and_typing_groups() {
    let (doc, nodes) = document(vec![vec![run("tail", vec![red(), Mark::Code])]]);
    let node = nodes[0];
    let mut session = DocumentSession::new(
        doc.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
    )
    .unwrap();
    let mark = style(StringAttribute::Null, "Noto Sans SC".into(), "".into());
    session
        .apply_intent(&EditIntent::SetMark { mark: mark.clone() })
        .unwrap();
    assert_eq!(
        session.stored_marks().unwrap().as_slice(),
        &[Mark::Code, mark.clone()]
    );
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    session
        .apply_intent(&EditIntent::SetMark { mark: mark.clone() })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(
        marks_at(session.document(), node, 0).as_slice(),
        &[Mark::Code, mark]
    );
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::TextStyle,
        })
        .unwrap();
    session
        .apply_intent(&EditIntent::RemoveMark {
            kind: MarkKind::Code,
        })
        .unwrap();
    assert_eq!(session.stored_marks(), Some(&MarkSet::empty()));
    session
        .apply_intent(&EditIntent::InsertText { text: "X".into() })
        .unwrap();
    assert_eq!(marks_at(session.document(), node, 2), MarkSet::empty());
    assert!(marks_at(session.document(), node, 3).contains(MarkKind::TextStyle));
}

#[test]
fn range_set_replaces_full_value_once_and_history_restores_original_states() {
    let old2 = style(
        StringAttribute::Missing,
        "serif".into(),
        StringAttribute::Null,
    );
    let (doc, nodes) = document(vec![vec![
        run("ab", vec![red(), Mark::Bold]),
        run("中", vec![old2]),
    ]]);
    let node = nodes[0];
    let selection = DocumentSelection::new(point(&doc, node, 5), point(&doc, node, 1));
    let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
    let exact = style(
        StringAttribute::Missing,
        StringAttribute::Missing,
        StringAttribute::Missing,
    );
    session
        .apply_intent(&EditIntent::SetMark {
            mark: exact.clone(),
        })
        .unwrap();
    assert_eq!(session.selection(), selection);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(
        marks_at(session.document(), node, 1).as_slice(),
        &[Mark::Bold, exact.clone()]
    );
    assert_eq!(marks_at(session.document(), node, 2).as_slice(), &[exact]);
    let changed = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), changed.store());
}

#[test]
fn structured_paste_rebuilds_exact_mark_runs_instead_of_leaking_destination_style() {
    let slice = copied(vec![
        run("P", vec![]),
        run(
            "中",
            vec![style(StringAttribute::Null, "".into(), "32px".into())],
        ),
    ]);
    let (doc, nodes) = document(vec![vec![run("ab", vec![red()])]]);
    let node = nodes[0];
    let at = point(&doc, node, 1);
    let mut session = DocumentSession::new(doc.clone(), DocumentSelection::collapsed(at)).unwrap();
    session
        .apply_intent(&EditIntent::PasteSlice { slice })
        .unwrap();
    assert_eq!(marks_at(session.document(), node, 1), MarkSet::empty());
    assert_eq!(
        marks_at(session.document(), node, 2).as_slice(),
        &[style(StringAttribute::Null, "".into(), "32px".into())]
    );
    assert_eq!(marks_at(session.document(), node, 5).as_slice(), &[red()]);
    assert_eq!(session.history_depths(), (1, 0));
    let pasted = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), pasted.store());
}

#[test]
fn cross_block_atom_reconstruction_does_not_style_the_plain_suffix() {
    let (doc, nodes) = document(vec![vec![run("aP", vec![red()])], vec![run("Qz", vec![])]]);
    let [first, second] = [nodes[0], nodes[1]];
    let seam = InlinePoint::new(
        second,
        inline(&doc, second).offset_at(1).unwrap(),
        0,
        CursorAffinity::Before,
    );
    let doc = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::InsertInlineAtom {
            at: seam,
            kind: AtomKind::new("mention").unwrap(),
            attrs: NodeAttrs::empty(),
            content: InlineAtomContent::new("@A").unwrap(),
        })
        .apply(&doc)
        .unwrap();
    let selection = DocumentSelection::new(InlinePoint::from(point(&doc, first, 1)), seam);
    let mut session = DocumentSession::new(doc.clone(), selection).unwrap();
    session.apply_intent(&EditIntent::Delete).unwrap();
    assert_eq!(
        inline(session.document(), first)
            .runs()
            .iter()
            .map(|run| run.text().as_str())
            .collect::<String>(),
        "az"
    );
    assert_eq!(marks_at(session.document(), first, 0).as_slice(), &[red()]);
    assert_eq!(marks_at(session.document(), first, 1), MarkSet::empty());
    assert_eq!(inline(session.document(), first).atoms().len(), 1);
    let changed = session.document().clone();
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
    session.redo().unwrap();
    assert_eq!(session.document().store(), changed.store());
}

struct RejectSize;
impl SessionPolicy for RejectSize {
    fn validate_document(&self, doc: &XiaomuDocument) -> Result<(), PolicyError> {
        for node in doc.store().iter() {
            if let Some(inline) = node.content().as_inline() {
                for run in inline.runs() {
                    if run.marks().as_slice().iter().any(|mark| matches!(mark, Mark::TextStyle(style) if matches!(style.attributes().font_size(), StringAttribute::Value(_)))) {
                        return Err(PolicyError::new("size rendering unavailable"));
                    }
                }
            }
        }
        Ok(())
    }
}

struct Changes(Rc<Cell<usize>>);
impl DocumentChangeListener for Changes {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn rejected_text_style_paste_preserves_pending_marks_history_selection_and_listener() {
    let (doc, nodes) = document(vec![vec![]]);
    let node = nodes[0];
    let mut session = DocumentSession::new_with_policy(
        doc.clone(),
        DocumentSelection::collapsed(TextPoint::at_start_of(node)),
        Box::new(RejectSize),
    )
    .unwrap();
    let changes = Rc::new(Cell::new(0));
    session.add_listener(Box::new(Changes(changes.clone())));
    session
        .apply_intent(&EditIntent::SetMark { mark: red() })
        .unwrap();
    session
        .apply_intent(&EditIntent::InsertText { text: "a".into() })
        .unwrap();
    let before = session.document().clone();
    let selection = session.selection();
    let pending = session.stored_marks().cloned();
    let count = changes.get();
    let slice = copied(vec![run(
        "bad",
        vec![style(
            StringAttribute::Null,
            StringAttribute::Null,
            "48px".into(),
        )],
    )]);
    assert!(
        session
            .apply_intent(&EditIntent::PasteSlice { slice })
            .is_err()
    );
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.document().revision(), before.revision());
    assert_eq!(session.selection(), selection);
    assert_eq!(session.stored_marks(), pending.as_ref());
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(changes.get(), count);
    session
        .apply_intent(&EditIntent::InsertText { text: "b".into() })
        .unwrap();
    assert_eq!(session.history_depths(), (1, 0));
    session.undo().unwrap();
    assert_eq!(session.document().store(), doc.store());
}
