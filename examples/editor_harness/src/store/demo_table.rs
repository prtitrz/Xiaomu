//! Table fixture shared by the interactive harness and persistence gate.
use xiaomu_core::document::{
    AtomKind, AttrValue, InlineAtomContent, InlineAtomPlacement, InlineContent, Mark, MarkSet,
    NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
};
use xiaomu_core::text::TextOffset;

fn attrs(label: &str) -> NodeAttrs {
    NodeAttrs::new([("data-fixture".into(), AttrValue::String(label.into()))].into()).unwrap()
}

pub(super) fn build(builder: &mut NodeStoreBuilder) -> NodeId {
    let mut table_rows = Vec::new();
    for (r, labels) in [
        ["需求", "状态"],
        [
            "中文 👩‍💻é אבג\n表格编辑",
            "Alt+单击选单格；跨格拖动或 Shift+单击扩选；Alt+Shift+方向键扩选。",
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let mut cells = Vec::new();
        for (c, text) in labels.into_iter().enumerate() {
            let inline = if r == 0 && c == 0 {
                let atom = builder
                    .insert(
                        NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
                        attrs("table-chip"),
                        NodeContent::InlineAtom(InlineAtomContent::new("@xiaomu").unwrap()),
                    )
                    .unwrap();
                InlineContent::with_atoms(
                    [TextRun::new(text, MarkSet::new([Mark::Bold]).unwrap()).unwrap()],
                    [InlineAtomPlacement::new(atom, TextOffset::ZERO)],
                )
                .unwrap()
            } else {
                InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap()
            };
            let mut block = builder
                .insert(
                    NodeKind::Paragraph,
                    attrs(&format!("text-{r}-{c}")),
                    NodeContent::Inline(inline),
                )
                .unwrap();
            if r == 1 && c == 0 {
                block = builder
                    .insert(
                        NodeKind::Quote,
                        attrs("table-quote"),
                        NodeContent::children([block]),
                    )
                    .unwrap();
            }
            cells.push(
                builder
                    .insert(
                        NodeKind::TableCell,
                        attrs(&format!("cell-{r}-{c}")),
                        NodeContent::children([block]),
                    )
                    .unwrap(),
            );
        }
        table_rows.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    attrs(&format!("row-{r}")),
                    NodeContent::children(cells),
                )
                .unwrap(),
        );
    }
    builder
        .insert(
            NodeKind::Table,
            attrs("table"),
            NodeContent::children(table_rows),
        )
        .unwrap()
}
