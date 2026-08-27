//! Harness-internal fixture persistence: canonical snapshot <-> text format.
//!
//! Format is a harness convention for the P2 host-contract gate, not a
//! codec commitment. v2 preserves current-stage canonical semantics: node
//! kind / tree shape, inline run boundaries, [`MarkSet`] (including Link
//! attributes), and [`NodeAttrs`] actually present on a node.

use std::path::PathBuf;

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};

use xiaomu_runtime::persistence::{DocumentPersistence, PersistenceError};

mod format;

pub use format::parse_document;
use format::write_node;

#[cfg(test)]
pub use format::{escape_text, unescape_text};

/// File-backed fixture adapter: the on-disk format is harness-internal
/// (`v2`, one node per line, BEGIN/END nesting) and explicitly not a codec
/// commitment.
pub struct FixtureStore {
    path: PathBuf,
}

impl FixtureStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl DocumentPersistence for FixtureStore {
    fn save(&mut self, document: &XiaomuDocument) -> Result<(), PersistenceError> {
        let mut out = String::from("xiaomu-fixture-doc v2\n");
        write_node(document, document.root(), &mut out)?;
        std::fs::write(&self.path, out)
            .map_err(|error| PersistenceError(format!("{}: {error}", self.path.display())))
    }

    fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => parse_document(&text).map(Some).map_err(PersistenceError),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(PersistenceError(format!(
                "{}: {error}",
                self.path.display()
            ))),
        }
    }
}

/// Multi-block demo fixture exercising P2.5 rendering: heading, paragraphs,
/// a quote, and both list kinds.
pub fn demo_fixture() -> XiaomuDocument {
    let mut builder = NodeStoreBuilder::new();
    let leaf = |kind: NodeKind, text: &str, builder: &mut NodeStoreBuilder| {
        builder
            .insert(
                kind,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(text, MarkSet::empty()).unwrap()]).unwrap(),
                ),
            )
            .unwrap()
    };
    let heading = leaf(
        NodeKind::Heading(xiaomu_core::document::HeadingLevel::new(2).unwrap()),
        "Xiaomu multi-block 演示",
        &mut builder,
    );
    let intro = leaf(
        NodeKind::Paragraph,
        "多块文档：↑↓ 或鼠标在块间移动；Enter 拆块；普通段落 Tab 变列表；列表项 Tab / Shift-Tab 缩进与退出（有上一兄弟才能缩进）。",
        &mut builder,
    );
    let quoted = leaf(
        NodeKind::Paragraph,
        "引用块里的文字，视觉上有缩进和竖线。",
        &mut builder,
    );
    let quote = builder
        .insert(
            NodeKind::Quote,
            NodeAttrs::empty(),
            NodeContent::children([quoted]),
        )
        .unwrap();
    let item_a = leaf(
        NodeKind::Paragraph,
        "第一个待办（Tab 缩进 / Shift-Tab 取消）",
        &mut builder,
    );
    let item_b = leaf(NodeKind::Paragraph, "第二个待办", &mut builder);
    let item_a = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([item_a]),
        )
        .unwrap();
    let item_b = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([item_b]),
        )
        .unwrap();
    let todo = builder
        .insert(
            NodeKind::BulletList,
            NodeAttrs::empty(),
            NodeContent::children([item_a, item_b]),
        )
        .unwrap();
    let step = leaf(NodeKind::Paragraph, "有序列表的一步", &mut builder);
    let step_item = builder
        .insert(
            NodeKind::ListItem,
            NodeAttrs::empty(),
            NodeContent::children([step]),
        )
        .unwrap();
    let steps = builder
        .insert(
            NodeKind::OrderedList,
            NodeAttrs::empty(),
            NodeContent::children([step_item]),
        )
        .unwrap();
    let outro = leaf(
        NodeKind::Paragraph,
        "编辑后按 Ctrl+S（macOS ⌘S）保存到 store 文件；下次启动从这里恢复。",
        &mut builder,
    );
    let root = builder
        .insert(
            NodeKind::Document,
            NodeAttrs::empty(),
            NodeContent::children([heading, intro, quote, todo, steps, outro]),
        )
        .unwrap();
    XiaomuDocument::new(root, builder.finish()).expect("fixture document")
}

/// Collapsed selection at the start of the first inline-bearing block.
pub fn caret_at_first_block(document: &XiaomuDocument) -> TextSelection {
    let mut stack = vec![document.root()];
    while let Some(id) = stack.pop() {
        let Some(node) = document.node(id) else {
            continue;
        };
        match node.content() {
            NodeContent::Inline(inline) => {
                return TextSelection::collapsed(TextPoint::new(
                    id,
                    inline.offset_at(0).unwrap(),
                    CursorAffinity::Before,
                ));
            }
            NodeContent::Children(children) => {
                for child in children.iter().rev() {
                    stack.push(*child);
                }
            }
            _ => {}
        }
    }
    panic!("fixture document has no inline block");
}

/// Current-stage canonical semantics: kind, tree shape, inline runs / marks,
/// and node attrs. Identities are allocation-order dependent and ignored.
pub fn canonical_semantics_equal(a: &XiaomuDocument, b: &XiaomuDocument) -> bool {
    fn walk(a: &XiaomuDocument, b: &XiaomuDocument, ai: NodeId, bi: NodeId) -> bool {
        let (Some(an), Some(bn)) = (a.node(ai), b.node(bi)) else {
            return false;
        };
        if an.kind() != bn.kind() || an.attrs() != bn.attrs() {
            return false;
        }
        match (an.content(), bn.content()) {
            (NodeContent::Inline(x), NodeContent::Inline(y)) => x.runs() == y.runs(),
            (NodeContent::Children(x), NodeContent::Children(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(x, y)| walk(a, b, *x, *y))
            }
            (NodeContent::Atomic, NodeContent::Atomic) => true,
            _ => false,
        }
    }
    walk(a, b, a.root(), b.root())
}

/// Structural equality kept as an alias for older call sites.
#[allow(dead_code)]
pub fn structurally_equal(a: &XiaomuDocument, b: &XiaomuDocument) -> bool {
    canonical_semantics_equal(a, b)
}
