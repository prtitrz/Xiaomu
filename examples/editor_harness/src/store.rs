//! Harness-internal fixture persistence: canonical snapshot ⇄ text format.
//!
//! Format is a harness convention for the P2.6 host-contract gate, not a
//! codec commitment: one block per line, BEGIN-style container tags closed
//! by `end`, TAB-separated tag/text, minimal backslash escapes. Marks are
//! not serialized in this fixture.

use std::path::PathBuf;
use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};

use xiaomu_runtime::persistence::{DocumentPersistence, PersistenceError};

/// File-backed fixture adapter: the on-disk format is harness-internal
/// (`v1`, one block per line, BEGIN/END nesting) and explicitly not a codec
/// commitment. Marks are not serialized in this fixture.
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
        let mut out = String::from("xiaomu-fixture-doc v1\n");
        write_node(document, document.root(), &mut out);
        std::fs::write(&self.path, out)
            .map_err(|error| PersistenceError(format!("{}: {error}", self.path.display())))
    }

    fn load(&self) -> Option<XiaomuDocument> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        parse_document(&text).ok()
    }
}

fn write_node(document: &XiaomuDocument, id: NodeId, out: &mut String) {
    let Some(node) = document.node(id) else {
        return;
    };
    match (node.kind(), node.content()) {
        (NodeKind::Paragraph, NodeContent::Inline(inline)) => {
            out.push_str("p\t");
            out.push_str(&escape_text(&concatenate(inline)));
            out.push('\n');
        }
        (NodeKind::Heading(level), NodeContent::Inline(inline)) => {
            out.push_str(&format!("h{}\t", level.as_u8()));
            out.push_str(&escape_text(&concatenate(inline)));
            out.push('\n');
        }
        (_, NodeContent::Children(children)) => {
            match node.kind() {
                NodeKind::Quote => out.push_str("quote\n"),
                NodeKind::BulletList => out.push_str("ul\n"),
                NodeKind::OrderedList => out.push_str("ol\n"),
                NodeKind::ListItem => out.push_str("li\n"),
                _ => {}
            }
            for child in children {
                write_node(document, *child, out);
            }
            if !matches!(node.kind(), NodeKind::Document) {
                out.push_str("end\n");
            }
        }
        _ => {}
    }
}

fn concatenate(inline: &InlineContent) -> String {
    inline
        .runs()
        .iter()
        .map(|run| run.text().as_str())
        .collect()
}

pub fn escape_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
}

pub fn unescape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(character);
        }
    }
    out
}

pub fn parse_document(text: &str) -> Result<XiaomuDocument, String> {
    let mut lines = text.lines();
    match lines.next() {
        Some("xiaomu-fixture-doc v1") => {}
        _ => return Err("unknown fixture header".to_owned()),
    }

    enum Frame {
        Quote,
        BulletList,
        OrderedList,
        ListItem,
    }

    struct Builder {
        store: NodeStoreBuilder,
        roots: Vec<NodeId>,
        stack: Vec<(Frame, Vec<NodeId>)>,
    }

    impl Builder {
        fn push(&mut self, id: NodeId) {
            match self.stack.last_mut() {
                Some((_, children)) => children.push(id),
                None => self.roots.push(id),
            }
        }

        fn leaf(&mut self, kind: NodeKind, text: &str) -> Result<(), String> {
            let inline = InlineContent::new(if text.is_empty() {
                Vec::new()
            } else {
                vec![TextRun::new(text, MarkSet::empty()).map_err(|error| error.to_string())?]
            })
            .map_err(|error| error.to_string())?;
            let id = self
                .store
                .insert(kind, NodeAttrs::empty(), NodeContent::Inline(inline))
                .map_err(|error| error.to_string())?;
            self.push(id);
            Ok(())
        }

        fn finish(mut self) -> Result<XiaomuDocument, String> {
            if !self.stack.is_empty() {
                return Err("unclosed container".to_owned());
            }
            let root = self
                .store
                .insert(
                    NodeKind::Document,
                    NodeAttrs::empty(),
                    NodeContent::children(self.roots),
                )
                .map_err(|error| error.to_string())?;
            XiaomuDocument::new(root, self.store.finish()).map_err(|error| error.to_string())
        }
    }

    let mut builder = Builder {
        store: NodeStoreBuilder::new(),
        roots: Vec::new(),
        stack: Vec::new(),
    };

    for line in lines {
        let (tag, rest) = match line.split_once('\t') {
            Some((tag, rest)) => (tag, rest),
            None => (line, ""),
        };
        match tag {
            "p" => builder.leaf(NodeKind::Paragraph, &unescape_text(rest))?,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..]
                    .parse::<u8>()
                    .map_err(|_| format!("bad heading level: {tag}"))?;
                let kind = NodeKind::Heading(
                    xiaomu_core::document::HeadingLevel::new(level)
                        .map_err(|error| error.to_string())?,
                );
                builder.leaf(kind, &unescape_text(rest))?;
            }
            "quote" => builder.stack.push((Frame::Quote, Vec::new())),
            "ul" => builder.stack.push((Frame::BulletList, Vec::new())),
            "ol" => builder.stack.push((Frame::OrderedList, Vec::new())),
            "li" => builder.stack.push((Frame::ListItem, Vec::new())),
            "end" => {
                let (frame, children) = builder
                    .stack
                    .pop()
                    .ok_or_else(|| "unbalanced end".to_owned())?;
                let kind = match frame {
                    Frame::Quote => NodeKind::Quote,
                    Frame::BulletList => NodeKind::BulletList,
                    Frame::OrderedList => NodeKind::OrderedList,
                    Frame::ListItem => NodeKind::ListItem,
                };
                let id = builder
                    .store
                    .insert(kind, NodeAttrs::empty(), NodeContent::children(children))
                    .map_err(|error| error.to_string())?;
                builder.push(id);
            }
            other => return Err(format!("unknown line tag: {other}")),
        }
    }

    builder.finish()
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
        "多块文档：↑↓ 或鼠标在块间移动；Enter 拆块；Tab / Shift-Tab 缩进列表（首项不可再缩进）。",
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

/// Structural equality of two snapshots: same tree shape, kinds, and inline
/// text. Identities are allocation-order dependent and compared loosely.
#[allow(dead_code)]
pub fn structurally_equal(a: &XiaomuDocument, b: &XiaomuDocument) -> bool {
    fn walk(a: &XiaomuDocument, b: &XiaomuDocument, ai: NodeId, bi: NodeId) -> bool {
        let (Some(an), Some(bn)) = (a.node(ai), b.node(bi)) else {
            return false;
        };
        if an.kind() != bn.kind() {
            return false;
        }
        match (an.content(), bn.content()) {
            (NodeContent::Inline(x), NodeContent::Inline(y)) => concatenate(x) == concatenate(y),
            (NodeContent::Children(x), NodeContent::Children(y)) => {
                x.len() == y.len() && x.iter().zip(y.iter()).all(|(x, y)| walk(a, b, *x, *y))
            }
            (NodeContent::Atomic, NodeContent::Atomic) => true,
            _ => false,
        }
    }
    walk(a, b, a.root(), b.root())
}
