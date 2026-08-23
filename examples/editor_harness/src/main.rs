//! Xiaomu editor harness: real-machine verification entry point.
//!
//! P1.3 opens a single-paragraph editor window. The manual gate checklist
//! lives in `docs/phases/p1-single-block-input/progress.md`.

use xiaomu_core::document::{
    InlineContent, MarkSet, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, TextRun,
    XiaomuDocument,
};
use xiaomu_core::selection::{CursorAffinity, TextPoint, TextSelection};
use xiaomu_gpui::editor::run_single_block_editor;

fn main() {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            NodeAttrs::empty(),
            NodeContent::Inline(
                InlineContent::new([TextRun::new(
                    "你好，Xiaomu！直接输入试试：←→ 移动，Shift 选择，Home/End，⌘Z/Ctrl+Z 撤销。",
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
            NodeContent::children([paragraph]),
        )
        .unwrap();
    let document = XiaomuDocument::new(root, builder.finish()).expect("fixture document");

    let inline = document
        .node(paragraph)
        .unwrap()
        .content()
        .as_inline()
        .unwrap();
    let selection = TextSelection::collapsed(TextPoint::new(
        paragraph,
        inline.offset_at(0).unwrap(),
        CursorAffinity::Before,
    ));

    if let Err(error) = run_single_block_editor(document, paragraph, selection) {
        eprintln!("xiaomu: failed to start editor: {error}");
        std::process::exit(1);
    }
}
