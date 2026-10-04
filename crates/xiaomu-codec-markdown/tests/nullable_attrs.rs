//! Export must reject typed null image fields, never silently omit them.

use xiaomu_codec_markdown::{MarkdownCodecError, to_markdown};
use xiaomu_core::Error;
use xiaomu_core::document::{
    AttrValue, ImageAttrs, ImageSource, NodeAttrs, NodeContent, NodeKind, NodeStoreBuilder,
    XiaomuDocument,
};

#[test]
fn image_null_and_other_non_string_fields_fail_export_without_mutation() {
    for key in ["title", "asset", "src", "alt"] {
        for invalid in [AttrValue::Null, AttrValue::Bool(false)] {
            let valid = ImageAttrs::new(
                ImageSource::ExternalUrl("https://example.invalid/image.png".into()),
                "image".into(),
                None,
                None,
                None,
            )
            .unwrap()
            .to_attrs()
            .unwrap();
            let mut values = valid
                .iter()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect::<std::collections::BTreeMap<_, _>>();
            values.insert(key.into(), invalid.clone());
            let mut builder = NodeStoreBuilder::new();
            let image = builder
                .insert(
                    NodeKind::Image,
                    NodeAttrs::new(values).unwrap(),
                    NodeContent::Atomic,
                )
                .unwrap();
            let root = builder
                .insert(
                    NodeKind::Document,
                    NodeAttrs::empty(),
                    NodeContent::children([image]),
                )
                .unwrap();
            let document = XiaomuDocument::new(root, builder.finish()).unwrap();
            assert_eq!(
                to_markdown(&document),
                Err(MarkdownCodecError::InvalidDocument(
                    Error::InvalidImageAttrs
                )),
                "{key}"
            );
            assert_eq!(
                document.node(image).unwrap().attrs().get(key),
                Some(&invalid)
            );
        }
    }
}
