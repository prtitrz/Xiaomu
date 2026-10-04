//! Task containers are never silently exported as ordinary Markdown lists.

use xiaomu_codec_markdown::{MarkdownCodecError, to_markdown};
use xiaomu_core::document::{
    AttrValue, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder, XiaomuDocument,
};

#[test]
fn direct_and_nested_task_lists_explicitly_reject_markdown_export() {
    for nested in [false, true] {
        let mut builder = NodeStoreBuilder::new();
        let paragraph = builder
            .insert(
                NodeKind::Paragraph,
                NodeAttrs::empty(),
                NodeContent::empty_inline(),
            )
            .unwrap();
        let item = builder
            .insert(
                NodeKind::TaskItem,
                NodeAttrs::new([("checked".into(), AttrValue::Bool(true))].into()).unwrap(),
                NodeContent::children([paragraph]),
            )
            .unwrap();
        let list = builder
            .insert(
                NodeKind::TaskList,
                NodeAttrs::empty(),
                NodeContent::children([item]),
            )
            .unwrap();
        let root_child = if nested {
            builder
                .insert(
                    NodeKind::Quote,
                    NodeAttrs::empty(),
                    NodeContent::children([list]),
                )
                .unwrap()
        } else {
            list
        };
        let root = builder
            .insert(
                NodeKind::Document,
                NodeAttrs::empty(),
                NodeContent::children([root_child]),
            )
            .unwrap();
        let document = XiaomuDocument::new(root, builder.finish()).unwrap();
        let before = document.store().clone();
        assert_eq!(
            to_markdown(&document),
            Err(MarkdownCodecError::UnsupportedNodeKind {
                kind: "TaskList".into()
            })
        );
        assert_eq!(document.store(), &before);
    }
}
