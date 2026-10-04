//! Explicit null is canonical data, never an absent key or removal request.

use xiaomu_core::Error;
use xiaomu_core::document::{
    AttrValue, ImageAttrs, ImageSource, InlineContent, NodeAttrs, NodeContent, NodeId, NodeKind,
    NodeStoreBuilder, TextRun, XiaomuDocument,
};
use xiaomu_core::mapping::{MapBias, MappedPosition};
use xiaomu_core::selection::{CursorAffinity, TextPoint};
use xiaomu_core::text::TextBuffer;
use xiaomu_core::transaction::{Transaction, TransactionOrigin, TransactionStep};

fn nullable_attrs() -> NodeAttrs {
    NodeAttrs::new(
        [
            ("textAlign".into(), AttrValue::Null),
            (
                "extension".into(),
                AttrValue::Object(
                    [(
                        "values".into(),
                        AttrValue::List(vec![
                            AttrValue::Null,
                            AttrValue::Bool(false),
                            AttrValue::Integer(0),
                            AttrValue::String("null".into()),
                            AttrValue::Object([("default".into(), AttrValue::Null)].into()),
                        ]),
                    )]
                    .into(),
                ),
            ),
        ]
        .into(),
    )
    .unwrap()
}

fn fixture(attrs: NodeAttrs) -> (XiaomuDocument, NodeId) {
    let mut builder = NodeStoreBuilder::new();
    let paragraph = builder
        .insert(
            NodeKind::Paragraph,
            attrs,
            NodeContent::Inline(
                InlineContent::new([TextRun::new("中🙂", Default::default()).unwrap()]).unwrap(),
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
    (
        XiaomuDocument::new(root, builder.finish()).unwrap(),
        paragraph,
    )
}

#[test]
fn null_is_distinct_from_missing_and_other_empty_values() {
    let attrs = nullable_attrs();
    assert_eq!(attrs.get("textAlign"), Some(&AttrValue::Null));
    assert_eq!(attrs.get("missing"), None);
    assert_ne!(attrs, NodeAttrs::empty());
    assert_eq!(attrs.len(), 2);
    assert_eq!(
        attrs.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["extension", "textAlign"]
    );
    for value in [
        AttrValue::Bool(false),
        AttrValue::Integer(0),
        AttrValue::String(String::new()),
        AttrValue::String("null".into()),
        AttrValue::List(Vec::new()),
        AttrValue::Object(Default::default()),
    ] {
        assert_ne!(AttrValue::Null, value);
    }
    let (document, paragraph) = fixture(attrs.clone());
    assert_eq!(document.version().as_u32(), 1);
    assert_eq!(document.node(paragraph).unwrap().attrs(), &attrs);
    assert!(document.validate().is_ok());
    assert_eq!(
        NodeAttrs::new([(" ".into(), AttrValue::Null)].into()),
        Err(Error::InvalidNodeAttrKey)
    );
}

#[test]
fn setting_null_and_removing_a_key_have_distinct_exact_inverses() {
    let (original, paragraph) = fixture(NodeAttrs::empty());
    let nullable = nullable_attrs();
    let applied = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: paragraph,
            attrs: nullable.clone(),
        })
        .apply_with_changes(&original)
        .unwrap();
    assert_eq!(
        applied.document().node(paragraph).unwrap().attrs(),
        &nullable
    );
    assert_eq!(
        original.node(paragraph).unwrap().attrs().get("textAlign"),
        None
    );
    assert_eq!(
        applied.inverse().apply(applied.document()).unwrap().store(),
        original.store()
    );

    let removed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: paragraph,
            attrs: NodeAttrs::empty(),
        })
        .apply_with_changes(applied.document())
        .unwrap();
    assert_eq!(
        removed
            .document()
            .node(paragraph)
            .unwrap()
            .attrs()
            .get("textAlign"),
        None
    );
    assert_eq!(
        removed.inverse().apply(removed.document()).unwrap().store(),
        applied.document().store()
    );
}

#[test]
fn split_join_and_their_inverses_preserve_nested_null_attrs_and_mapping() {
    let attrs = nullable_attrs();
    let (original, first) = fixture(attrs.clone());
    let text = TextBuffer::from("中🙂");
    let at = text.offset_at("中".len()).unwrap();
    let split = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SplitNode { node: first, at })
        .apply_with_changes(&original)
        .unwrap();
    let document = split.document();
    let second = document
        .node(document.root())
        .unwrap()
        .content()
        .as_children()
        .unwrap()[1];
    assert_eq!(document.node(first).unwrap().attrs(), &attrs);
    assert_eq!(document.node(second).unwrap().attrs(), &attrs);
    assert_eq!(
        split.changes().map_text_point(
            TextPoint::new(first, at, CursorAffinity::Before),
            MapBias::End
        ),
        MappedPosition::Mapped(TextPoint::new(
            second,
            text.offset_at(0).unwrap(),
            CursorAffinity::Before
        ))
    );
    assert_eq!(
        split.inverse().apply(document).unwrap().store(),
        original.store()
    );

    // Give the tail different attrs: join keeps the first node's attrs while
    // the inverse must restore the tail's exact null-versus-missing state.
    let changed = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::SetNodeAttrs {
            node: second,
            attrs: NodeAttrs::empty(),
        })
        .apply(document)
        .unwrap();
    let joined = Transaction::new(TransactionOrigin::UserInput)
        .with_step(TransactionStep::JoinNodes { first, second })
        .apply_with_changes(&changed)
        .unwrap();
    assert_eq!(joined.document().node(first).unwrap().attrs(), &attrs);
    assert!(joined.document().node(second).is_none());
    assert_eq!(
        joined.inverse().apply(joined.document()).unwrap().store(),
        changed.store()
    );
}

#[test]
fn null_does_not_relax_typed_image_requirements() {
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
    for key in ["src", "asset", "alt", "title", "width", "height"] {
        let mut values = valid
            .iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        values.insert(key.into(), AttrValue::Null);
        let attrs = NodeAttrs::new(values).unwrap();
        assert_eq!(
            ImageAttrs::from_attrs(&attrs),
            Err(Error::InvalidImageAttrs),
            "{key}"
        );
    }
    let mut values = valid
        .iter()
        .map(|(key, value)| (key.to_owned(), value.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    values.insert("extension".into(), AttrValue::Null);
    let attrs = NodeAttrs::new(values).unwrap();
    assert!(ImageAttrs::from_attrs(&attrs).is_ok());
    assert_eq!(attrs.get("extension"), Some(&AttrValue::Null));
}

#[test]
fn known_image_strings_reject_non_strings_in_either_source_configuration() {
    for source in [
        ImageSource::ExternalUrl("https://example.invalid/image.png".into()),
        ImageSource::AssetRef("host-image".into()),
    ] {
        let valid = ImageAttrs::new(source, "image".into(), None, None, None)
            .unwrap()
            .to_attrs()
            .unwrap();
        for key in ["src", "asset", "alt", "title"] {
            for invalid in [AttrValue::Null, AttrValue::Bool(false)] {
                let mut values = valid
                    .iter()
                    .map(|(key, value)| (key.to_owned(), value.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>();
                values.insert(key.into(), invalid);
                assert_eq!(
                    ImageAttrs::from_attrs(&NodeAttrs::new(values).unwrap()),
                    Err(Error::InvalidImageAttrs),
                    "{key}"
                );
            }
        }
    }
}
