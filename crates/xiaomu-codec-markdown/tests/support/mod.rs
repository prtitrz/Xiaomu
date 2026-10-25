use xiaomu_core::document::{
    InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};

pub fn run(text: &str, marks: impl IntoIterator<Item = Mark>) -> TextRun {
    TextRun::new(text, MarkSet::new(marks).unwrap()).unwrap()
}

pub fn document(kind: NodeKind, runs: Vec<TextRun>, containers: &[NodeKind]) -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let mut child = builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap();
    for container in containers {
        child = builder
            .insert(
                container.clone(),
                NodeAttrs::empty(),
                NodeContent::children([child]),
            )
            .unwrap();
    }
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([child]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).unwrap()
}

// Compare all semantic fields and wrapper structure, deliberately excluding IDs.
pub fn assert_semantics(left: &XiaomuDocument, right: &XiaomuDocument) {
    let mut stack = vec![(left.root(), right.root())];
    while let Some((a, b)) = stack.pop() {
        let a = left.node(a).unwrap();
        let b = right.node(b).unwrap();
        assert_eq!(a.kind(), b.kind());
        assert_eq!(a.attrs(), b.attrs());
        match (a.content(), b.content()) {
            (NodeContent::Children(a), NodeContent::Children(b)) => {
                assert_eq!(a.len(), b.len());
                stack.extend(a.iter().copied().zip(b.iter().copied()));
            }
            (a, b) => assert_eq!(a, b),
        }
    }
}
