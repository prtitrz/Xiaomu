//! The default fixture keeps the table visible near the top for the P5 gate.
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
};
use xiaomu_core::text::TextBuffer;

pub(super) fn append(builder: &mut NodeStoreBuilder) -> NodeId {
    fn leaf(builder: &mut NodeStoreBuilder, text: &str) -> NodeId {
        builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    }
    fn group(
        builder: &mut NodeStoreBuilder,
        kind: NodeKind,
        children: Vec<NodeId>,
        tag: &str,
    ) -> NodeId {
        builder
            .insert(
                kind,
                NodeAttrs::new(
                    [(
                        "extension-tag".to_owned(),
                        AttrValue::String(tag.to_owned()),
                    )]
                    .into_iter()
                    .collect(),
                )
                .unwrap(),
                NodeContent::children(children),
            )
            .unwrap()
    }
    let a = leaf(
        builder,
        "表格输入：中文🙂 e\u{301}。Ctrl+Shift+Space 选格，Shift+方向键扩展；也可拖动左上角小方块。",
    );
    let a = group(builder, NodeKind::TableCell, vec![a], "a1");
    let b = leaf(
        builder,
        "右列：Tab / Shift-Tab 换格；↑↓ 保持列。末格 Tab 新增行。",
    );
    let b = group(builder, NodeKind::TableCell, vec![b], "b1");
    let row1 = group(builder, NodeKind::TableRow, vec![a, b], "row-1");
    let quote = leaf(builder, "格内引用 / IME 候选框应跟随光标。");
    let quote = group(builder, NodeKind::Quote, vec![quote], "quote");
    let atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            NodeAttrs::empty(),
            NodeContent::InlineAtom(InlineAtomContent::new("@xiaomu").unwrap()),
        )
        .unwrap();
    let text = "同格 chip 与正文";
    let p = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(text, MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(
                        atom,
                        TextBuffer::from_string(text.to_owned())
                            .offset_at(0)
                            .unwrap(),
                    )],
                )
                .unwrap(),
            ),
        )
        .unwrap();
    let a2 = group(builder, NodeKind::TableCell, vec![quote, p], "a2");
    let nested = leaf(builder, "嵌套表格");
    let nested = group(builder, NodeKind::TableCell, vec![nested], "nested-cell");
    let nested = group(builder, NodeKind::TableRow, vec![nested], "nested-row");
    let nested = group(builder, NodeKind::Table, vec![nested], "nested-table");
    let tail = leaf(builder, "嵌套表格之后");
    let b2 = group(builder, NodeKind::TableCell, vec![nested, tail], "b2");
    let row2 = group(builder, NodeKind::TableRow, vec![a2, b2], "row-2");
    group(builder, NodeKind::Table, vec![row1, row2], "table-v5")
}
