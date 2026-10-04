//! Shared mixed-inline structural fixtures; every atom carries independent data.

use std::cell::RefCell;
use std::rc::Rc;

use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStore, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::{TextBuffer, TextOffset};
use xiaomu_runtime::session::{
    DocumentChangeListener, DocumentPosition, DocumentSelection, DocumentSession, SessionOutcome,
};

pub fn offset(raw: usize) -> TextOffset {
    TextBuffer::from_string("x".repeat(raw))
        .offset_at(raw)
        .unwrap()
}

pub fn point(node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(node, offset(raw), ordinal, CursorAffinity::After)
}

pub fn block(builder: &mut NodeStoreBuilder, text: &str, at: &[(usize, bool)]) -> NodeId {
    let placements: Vec<_> = at
        .iter()
        .enumerate()
        .map(|(index, &(raw, is_break))| {
            let kind = if is_break {
                AtomKind::hard_break()
            } else {
                // An extension with the same label must stay distinct.
                AtomKind::new("hardBreak").unwrap()
            };
            let attrs = if is_break {
                NodeAttrs::empty()
            } else {
                NodeAttrs::new(
                    [(
                        "opaque".into(),
                        AttrValue::List(vec![
                            AttrValue::Null,
                            AttrValue::String(format!("未知🙂-{index}")),
                        ]),
                    )]
                    .into(),
                )
                .unwrap()
            };
            let content = if is_break {
                InlineAtomContent::hard_break()
            } else {
                InlineAtomContent::new("@unknown🙂").unwrap()
            }
            .with_marks(MarkSet::new([Mark::Italic, Mark::Code]).unwrap());
            let atom = builder
                .insert(
                    NodeKind::InlineAtom(kind),
                    attrs,
                    NodeContent::InlineAtom(content),
                )
                .unwrap();
            InlineAtomPlacement::new(atom, offset(raw))
        })
        .collect();
    let runs = if text.is_empty() {
        Vec::new()
    } else {
        vec![TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()]
    };
    builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap()
}

pub fn container(builder: &mut NodeStoreBuilder, kind: NodeKind, children: &[NodeId]) -> NodeId {
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::children(children.to_vec()),
        )
        .unwrap()
}

pub fn finish(mut builder: NodeStoreBuilder, children: &[NodeId]) -> XiaomuDocument {
    let root = container(&mut builder, NodeKind::Document, children);
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

pub fn inline(document: &XiaomuDocument, node: NodeId) -> &InlineContent {
    document.node(node).unwrap().content().as_inline().unwrap()
}

pub fn children(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    document
        .node(node)
        .unwrap()
        .content()
        .as_children()
        .unwrap()
        .to_vec()
}

pub fn text(document: &XiaomuDocument, node: NodeId) -> String {
    inline(document, node)
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

pub fn atoms(document: &XiaomuDocument, node: NodeId) -> Vec<NodeId> {
    inline(document, node)
        .atoms()
        .iter()
        .map(|placement| placement.atom())
        .collect()
}

pub fn focus(session: &DocumentSession) -> InlinePoint {
    match session.selection().focus() {
        DocumentPosition::Inline(point) => point,
        _ => panic!("expected mixed-inline caret"),
    }
}

pub fn assert_payloads(before: &XiaomuDocument, after: &XiaomuDocument, ids: &[NodeId]) {
    for id in ids {
        assert_eq!(after.node(*id), before.node(*id), "atom payload {id:?}");
    }
}

pub type Events = Rc<RefCell<Vec<(NodeStore, DocumentSelection)>>>;
struct Listener(Events);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, document: &XiaomuDocument, selection: DocumentSelection) {
        document.validate().unwrap();
        selection.validate(document).unwrap();
        self.0
            .borrow_mut()
            .push((document.store().clone(), selection));
    }
}

pub fn listen(session: &mut DocumentSession) -> Events {
    let events = Events::default();
    session.add_listener(Box::new(Listener(events.clone())));
    events
}

pub fn round_trip(
    session: &mut DocumentSession,
    before: &XiaomuDocument,
    before_selection: DocumentSelection,
    events: &Events,
) {
    let after = session.document().clone();
    let after_selection = session.selection();
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0], (after.store().clone(), after_selection));
    assert_eq!(session.undo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), before.store());
    assert_eq!(session.selection(), before_selection);
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.history_depths(), (0, 1));
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(
        events.borrow()[1],
        (before.store().clone(), before_selection)
    );
    assert_eq!(session.redo().unwrap(), SessionOutcome::DocumentChanged);
    assert_eq!(session.document().store(), after.store());
    assert_eq!(session.selection(), after_selection);
    assert_eq!(session.stored_marks(), None);
    assert_eq!(session.history_depths(), (1, 0));
    assert_eq!(events.borrow().len(), 3);
    assert_eq!(events.borrow()[2], (after.store().clone(), after_selection));
}
