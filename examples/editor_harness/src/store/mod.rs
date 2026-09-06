//! Harness-internal fixture persistence: canonical snapshot <-> text format.
//!
//! Format is a harness convention for the P2 host-contract gate, not a
//! codec commitment. v3 preserves current-stage canonical semantics: node
//! kind / tree shape, inline run boundaries, [`MarkSet`] (including Link
//! attributes), [`NodeAttrs`] actually present on a node, and inline atom
//! placements with their atom nodes (kind / fallback / attrs).

use std::path::PathBuf;

use xiaomu_core::document::{
    AtomKind, InlineAtomContent, InlineAtomPlacement, InlineContent, MarkSet, NodeAttrs,
    NodeContent, NodeId, NodeKind, NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_core::text::TextBuffer;

use xiaomu_runtime::persistence::{DocumentPersistence, PersistenceError};

mod format;
mod marks_text;

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
        let mut out = String::from("xiaomu-fixture-doc v4\n");
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
    // Inline-atom demo: one mention chip anchored at the paragraph start.
    // The chip round-trips through the v3 fixture format and renders via the
    // harness demo renderer.
    let mention_atom = builder
        .insert(
            NodeKind::InlineAtom(AtomKind::new("mention").unwrap()),
            NodeAttrs::new(
                [(
                    "handle".to_owned(),
                    xiaomu_core::document::AttrValue::String("xiaomu".to_owned()),
                )]
                .into_iter()
                .collect(),
            )
            .unwrap(),
            NodeContent::InlineAtom(InlineAtomContent::new("@xiaomu").unwrap()),
        )
        .unwrap();
    let mention_text = " 是一个 inline atom chip：点击它会通过 capability seam 通知宿主。";
    let mention = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::with_atoms(
                    [TextRun::new(mention_text, MarkSet::empty()).unwrap()],
                    [InlineAtomPlacement::new(
                        mention_atom,
                        TextBuffer::from_string(mention_text.to_owned())
                            .offset_at(0)
                            .unwrap(),
                    )],
                )
                .unwrap(),
            ),
        )
        .unwrap();
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
    // Media demo (P4): an HR atomic block and an image block whose canonical
    // attrs keep an unknown extension tag, both encoded by fixture v4.
    let rule = builder
        .insert(
            NodeKind::HorizontalRule,
            NodeAttrs::empty(),
            NodeContent::Atomic,
        )
        .unwrap();
    let mut image_values = std::collections::BTreeMap::new();
    image_values.insert(
        "src".to_owned(),
        xiaomu_core::document::AttrValue::String("https://example.com/xiaomu-cover.png".to_owned()),
    );
    image_values.insert(
        "alt".to_owned(),
        xiaomu_core::document::AttrValue::String("晓木封面".to_owned()),
    );
    image_values.insert(
        "data-x-extension-tag".to_owned(),
        xiaomu_core::document::AttrValue::String("v1".to_owned()),
    );
    let image = builder
        .insert(
            NodeKind::Image,
            NodeAttrs::new(image_values).unwrap(),
            NodeContent::Atomic,
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
            NodeContent::children([
                heading, intro, mention, quote, rule, image, todo, steps, outro,
            ]),
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
            (NodeContent::Inline(x), NodeContent::Inline(y)) => {
                if x.runs() != y.runs() || x.atoms().len() != y.atoms().len() {
                    return false;
                }
                x.atoms().iter().zip(y.atoms().iter()).all(|(xa, ya)| {
                    xa.text_offset() == ya.text_offset() && walk(a, b, xa.atom(), ya.atom())
                })
            }
            (NodeContent::InlineAtom(x), NodeContent::InlineAtom(y)) => {
                x.fallback_text() == y.fallback_text()
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn multiline_fixture() -> XiaomuDocument {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new("alpha\nbeta", MarkSet::empty()).unwrap()])
                        .unwrap(),
                ),
            )
            .unwrap();
        let code = builder
            .insert(
                NodeKind::CodeBlock,
                NodeAttrs::empty(),
                NodeContent::Inline(
                    InlineContent::new([TextRun::new(
                        "fn main() {\n    println!(\"ok\");\n}",
                        MarkSet::empty(),
                    )
                    .unwrap()])
                    .unwrap(),
                ),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([paragraph, code]),
            )
            .unwrap();
        XiaomuDocument::new(root, builder.finish()).unwrap()
    }

    #[test]
    fn fixture_v2_round_trips_hard_breaks_and_code_newlines() {
        let document = multiline_fixture();
        let mut encoded = String::from("xiaomu-fixture-doc v2\n");
        write_node(&document, document.root(), &mut encoded).unwrap();

        assert!(encoded.contains("alpha\\nbeta"));
        assert!(encoded.contains("fn main() \\{\\n    println!"));
        let decoded = parse_document(&encoded).unwrap();
        assert!(canonical_semantics_equal(&document, &decoded));
    }

    #[test]
    fn fixture_text_escape_preserves_lf_round_trip() {
        let source = "a\nb\n";
        assert_eq!(unescape_text(&escape_text(source)), source);
    }

    #[test]
    fn fixture_v4_round_trips_inline_atom_chips_and_media() {
        let document = demo_fixture();
        let mut encoded = String::from("xiaomu-fixture-doc v4\n");
        write_node(&document, document.root(), &mut encoded).unwrap();

        // The mention chip serializes as an atom token plus its atom line,
        // and the media blocks encode as atomic lines with preserved attrs
        // (including the unknown extension tag).
        assert!(encoded.contains("{a#0}"));
        assert!(encoded.contains("atom\tmention\t@xiaomu"));
        assert!(encoded.contains("handle=s:xiaomu"));
        assert!(encoded.contains("hr\n"));
        assert!(encoded.contains("img\n"));
        assert!(encoded.contains("alt=s:晓木封面"));
        assert!(encoded.contains("data-x-extension-tag=s:v1"));

        let decoded = parse_document(&encoded).unwrap();
        assert!(canonical_semantics_equal(&document, &decoded));
    }

    #[test]
    fn fixture_still_refuses_custom_node_kinds() {
        let mut builder = NodeStoreBuilder::new();
        let custom = builder
            .insert(
                NodeKind::Custom("x-spoiler".to_owned()),
                NodeAttrs::empty(),
                NodeContent::children([]),
            )
            .unwrap();
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([custom]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();

        let error = write_node(&document, document.root(), &mut String::new()).unwrap_err();
        assert!(error.0.contains("x-spoiler"), "{error}");
    }

    #[test]
    fn fixture_v3_rejects_references_to_undefined_atoms() {
        let encoded = "xiaomu-fixture-doc v3\np\t{a#0}\t\n";
        assert!(parse_document(encoded).is_err());
    }
}
