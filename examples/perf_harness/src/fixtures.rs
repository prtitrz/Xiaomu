//! Fixed long-document, rich/span/nested-table and multi-editor workloads.
use xiaomu_core::{
    document::{
        AttrValue, InlineContent, Mark, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind,
        NodeStoreBuilder, TextRun, XiaomuDocument,
    },
    selection::InlinePoint,
};
use xiaomu_runtime::session::{DocumentSelection, DocumentSession};

/// Fixed names accepted by the measurement CLI; each runs in a fresh process.
pub const SCENARIOS: &[&str] = &[
    "long-1k",
    "long-10k",
    "code-128k",
    "table-20x20",
    "table-50x20",
    "multi-4x1k",
    "multi-4x10k",
];

/// One immutable starting document and independent editor count.
pub struct Fixture {
    /// Valid canonical snapshot, shared only until sessions first edit.
    pub document: XiaomuDocument,
    /// Last editable block, so lookups cannot use an accidental first-node fast path.
    pub target: NodeId,
    /// Number of independent sessions/views to mount.
    pub editors: usize,
    /// Outer complex table when present, used for structural-edit measurements.
    pub table: Option<NodeId>,
}

impl Fixture {
    /// Constructs independent sessions with a caret at the target's beginning.
    pub fn sessions(&self) -> Vec<DocumentSession> {
        (0..self.editors)
            .map(|_| {
                DocumentSession::new(
                    self.document.clone(),
                    DocumentSelection::collapsed(InlinePoint::at_start_of(self.target)),
                )
                .unwrap()
            })
            .collect()
    }
}

fn text(builder: &mut NodeStoreBuilder, kind: NodeKind, content: &str, rich: bool) -> NodeId {
    let runs = if rich {
        vec![
            TextRun::new("bold ", MarkSet::new([Mark::Bold]).unwrap()).unwrap(),
            TextRun::new(content, MarkSet::empty()).unwrap(),
        ]
    } else {
        vec![TextRun::new(content, MarkSet::empty()).unwrap()]
    };
    builder
        .insert(
            kind,
            NodeAttrs::empty(),
            NodeContent::Inline(InlineContent::new(runs).unwrap()),
        )
        .unwrap()
}

fn table(
    builder: &mut NodeStoreBuilder,
    rows: usize,
    columns: usize,
    nested: bool,
) -> (NodeId, NodeId) {
    let mut row_ids = vec![];
    let mut target = None;
    for row in 0..rows {
        let mut cells = vec![];
        let mut col = 0;
        while col < columns {
            // Alternating 2-column origins; vertical spans on the first two rows.
            if row == 1 && col == 0 {
                col += 2;
                continue;
            }
            let span = if col % 4 == 0 && col + 1 < columns {
                2
            } else {
                1
            };
            let leaf = text(
                builder,
                NodeKind::Paragraph,
                &format!("r{row} c{col} 中文 e\u{301} 🌲"),
                true,
            );
            target = Some(leaf);
            let mut children = vec![leaf];
            if nested && row % 10 == 0 && col == 0 {
                let (inner, _) = table(builder, 2, 4, false);
                children.push(inner);
            }
            let attrs = NodeAttrs::new(
                [
                    ("colspan".into(), AttrValue::Integer(span as i64)),
                    (
                        "rowspan".into(),
                        AttrValue::Integer(if row == 0 && col == 0 { 2 } else { 1 }),
                    ),
                ]
                .into(),
            )
            .unwrap();
            cells.push(
                builder
                    .insert(
                        if row == 0 {
                            NodeKind::TableHeader
                        } else {
                            NodeKind::TableCell
                        },
                        attrs,
                        NodeContent::children(children),
                    )
                    .unwrap(),
            );
            col += span;
        }
        row_ids.push(
            builder
                .insert(
                    NodeKind::TableRow,
                    NodeAttrs::empty(),
                    NodeContent::children(cells),
                )
                .unwrap(),
        );
    }
    (
        builder
            .insert(
                NodeKind::Table,
                NodeAttrs::empty(),
                NodeContent::children(row_ids),
            )
            .unwrap(),
        target.unwrap(),
    )
}

/// Builds an exact named workload; invalid names are rejected instead of falling back.
pub fn build(name: &str) -> Fixture {
    assert!(SCENARIOS.contains(&name), "unknown scenario {name}");
    let mut builder = NodeStoreBuilder::new();
    let (children, target, editors, outer_table) = if name.starts_with("table-") {
        let (table, target) = table(
            &mut builder,
            if name == "table-20x20" { 20 } else { 50 },
            20,
            true,
        );
        (vec![table], target, 1, Some(table))
    } else if name == "code-128k" {
        let body = "let value = 42; // 中文 e\u{301}\n".repeat(4096);
        let leaf = text(&mut builder, NodeKind::CodeBlock, &body, false);
        (vec![leaf], leaf, 1, None)
    } else {
        let count = if name.ends_with("10k") { 10_000 } else { 1_000 };
        let children: Vec<_> = (0..count)
            .map(|i| {
                text(
                    &mut builder,
                    NodeKind::Paragraph,
                    &format!("Paragraph {i}: 中文 e\u{301} 🌲 — reproducible long document text."),
                    false,
                )
            })
            .collect();
        let target = *children.last().unwrap();
        (
            children,
            target,
            if name.starts_with("multi-") { 4 } else { 1 },
            None,
        )
    };
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children(children),
        )
        .unwrap();
    Fixture {
        document: XiaomuDocument::new(root, builder.finish()).unwrap(),
        target,
        editors,
        table: outer_table,
    }
}

/// Concatenates exact text at the measured leaf for correctness checks outside timers.
pub fn leaf_text(document: &XiaomuDocument, target: NodeId) -> String {
    document
        .node(target)
        .unwrap()
        .content()
        .as_inline()
        .unwrap()
        .runs()
        .iter()
        .map(|r| r.text().as_str())
        .collect()
}
