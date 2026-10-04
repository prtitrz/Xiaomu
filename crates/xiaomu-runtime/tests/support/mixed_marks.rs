//! Shared deterministic mixed-inline fixtures for mark-range contract tests.

use std::{cell::Cell, rc::Rc};

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, InlinePoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_runtime::session::{DocumentChangeListener, DocumentSelection, DocumentSession};

pub fn marks(values: impl IntoIterator<Item = Mark>) -> MarkSet {
    MarkSet::new(values).unwrap()
}

pub fn block(
    builder: &mut NodeStoreBuilder,
    text: &str,
    text_marks: MarkSet,
    atoms: &[(usize, AtomKind, MarkSet)],
) -> (NodeId, Vec<NodeId>) {
    let buffer = TextBuffer::from_string(text.to_owned());
    let mut identities = Vec::new();
    let mut placements = Vec::new();
    for (raw, kind, marks) in atoms {
        let content = if kind.is_hard_break() {
            InlineAtomContent::hard_break()
        } else {
            InlineAtomContent::new("@张🙂").unwrap()
        };
        let atom = builder
            .insert(
                NodeKind::InlineAtom(kind.clone()),
                NodeAttrs::empty(),
                NodeContent::InlineAtom(content.with_marks(marks.clone())),
            )
            .unwrap();
        identities.push(atom);
        placements.push(InlineAtomPlacement::new(
            atom,
            buffer.offset_at(*raw).unwrap(),
        ));
    }
    let runs = if text.is_empty() {
        vec![]
    } else {
        vec![TextRun::new(text, text_marks).unwrap()]
    };
    let node = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::with_atoms(runs, placements).unwrap()),
        )
        .unwrap();
    (node, identities)
}

pub fn finish(mut builder: NodeStoreBuilder, children: Vec<NodeId>) -> XiaomuDocument {
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

pub fn fixture(
    text: &str,
    text_marks: MarkSet,
    atoms: &[(usize, AtomKind, MarkSet)],
) -> (XiaomuDocument, NodeId, Vec<NodeId>) {
    let mut builder = NodeStoreBuilder::new();
    let (node, atoms) = block(&mut builder, text, text_marks, atoms);
    (finish(builder, vec![node]), node, atoms)
}

pub fn inline(document: &XiaomuDocument, node: NodeId) -> &InlineContent {
    document.node(node).unwrap().content().as_inline().unwrap()
}

pub fn point(document: &XiaomuDocument, node: NodeId, raw: usize, ordinal: usize) -> InlinePoint {
    InlinePoint::new(
        node,
        inline(document, node).offset_at(raw).unwrap(),
        ordinal,
        CursorAffinity::Before,
    )
}

pub fn atom_marks(document: &XiaomuDocument, atom: NodeId) -> &MarkSet {
    document
        .node(atom)
        .unwrap()
        .content()
        .as_inline_atom()
        .unwrap()
        .marks()
}

pub fn text_marks(document: &XiaomuDocument, node: NodeId, raw: usize) -> &MarkSet {
    let mut cursor = 0;
    for run in inline(document, node).runs() {
        cursor += run.len_bytes();
        if raw < cursor {
            return run.marks();
        }
    }
    panic!("offset must address a text scalar");
}

pub type Counts = Rc<Cell<(usize, usize)>>;

struct Listener(Counts);
impl DocumentChangeListener for Listener {
    fn document_changed(&mut self, _: &XiaomuDocument, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents + 1, selections));
    }
    fn selection_changed(&mut self, _: DocumentSelection) {
        let (documents, selections) = self.0.get();
        self.0.set((documents, selections + 1));
    }
}

pub fn listen(session: &mut DocumentSession) -> Counts {
    let counts = Rc::new(Cell::new((0, 0)));
    session.add_listener(Box::new(Listener(counts.clone())));
    counts
}
