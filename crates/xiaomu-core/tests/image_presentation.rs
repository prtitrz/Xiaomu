//! Presentation is borrowed and nullable; construction and raw data stay strict.

use std::collections::BTreeMap;
use xiaomu_core::Error;
use xiaomu_core::document::{
    AttrValue, ImageAttrs, ImagePresentationAttrs, ImageSource, ImageSourceRef, NodeAttrs,
};

fn source_attrs(key: &str) -> BTreeMap<String, AttrValue> {
    BTreeMap::from([(key.into(), AttrValue::String("  opaque:图片  ".into()))])
}

fn insert_state(values: &mut BTreeMap<String, AttrValue>, key: &str, state: u32) {
    match state {
        0 => {}
        1 => {
            values.insert(key.into(), AttrValue::Null);
        }
        2 => {
            let value = match key {
                "width" => AttrValue::Integer(240),
                "height" => AttrValue::Integer(120),
                _ => AttrValue::String(format!("{key} 中文👩‍💻\n\"")),
            };
            values.insert(key.into(), value);
        }
        _ => unreachable!(),
    }
}

#[test]
fn both_sources_accept_all_81_metadata_states_without_changing_raw_attrs() {
    for key in ["asset", "src"] {
        for states in 0..81_u32 {
            let mut values = source_attrs(key);
            for (index, name) in ["alt", "title", "width", "height"].iter().enumerate() {
                insert_state(&mut values, name, states / 3_u32.pow(index as u32) % 3);
            }
            let raw = NodeAttrs::new(values).unwrap();
            let before = raw.clone();
            let view = ImagePresentationAttrs::read(&raw).unwrap();
            let expected_source = match key {
                "asset" => ImageSourceRef::AssetRef("  opaque:图片  "),
                _ => ImageSourceRef::ExternalUrl("  opaque:图片  "),
            };
            assert_eq!(view.source(), expected_source);
            let AttrValue::String(source) = raw.get(key).unwrap() else {
                unreachable!();
            };
            assert!(std::ptr::eq(view.source().value(), source.as_str()));
            for (name, resolved) in [("alt", view.alt()), ("title", view.title())] {
                match raw.get(name) {
                    Some(AttrValue::String(value)) => {
                        assert_eq!(resolved, Some(value.as_str()));
                        assert!(std::ptr::eq(resolved.unwrap(), value.as_str()));
                    }
                    None | Some(AttrValue::Null) => assert_eq!(resolved, None),
                    _ => unreachable!(),
                }
            }
            assert_eq!(view.width(), (states / 9 % 3 == 2).then_some(240));
            assert_eq!(view.height(), (states / 27 % 3 == 2).then_some(120));
            assert_eq!(raw, before, "{key}: {states}");
        }
    }
}

#[test]
fn empty_whitespace_and_unicode_metadata_are_borrowed_verbatim() {
    for key in ["asset", "src"] {
        for text in ["", " \t\n", "中文👩‍💻\n\"quoted\""] {
            let mut values = source_attrs(key);
            for field in ["alt", "title"] {
                values.insert(field.into(), AttrValue::String(text.into()));
            }
            let raw = NodeAttrs::new(values).unwrap();
            let before = raw.clone();
            let view = ImagePresentationAttrs::read(&raw).unwrap();
            assert_eq!(view.alt(), Some(text));
            assert_eq!(view.title(), Some(text));
            assert_eq!(raw, before);
        }
    }
}

fn assert_invalid(values: BTreeMap<String, AttrValue>) {
    let raw = NodeAttrs::new(values).unwrap();
    let before = raw.clone();
    assert_eq!(
        ImagePresentationAttrs::read(&raw),
        Err(Error::InvalidImageAttrs),
        "{raw:?}"
    );
    assert_eq!(raw, before);
}

#[test]
fn missing_null_empty_wrong_type_and_ambiguous_sources_are_rejected() {
    assert_invalid(BTreeMap::new());
    let invalid = [
        AttrValue::Null,
        AttrValue::String(String::new()),
        AttrValue::String(" \t\n".into()),
        AttrValue::Integer(1),
        AttrValue::Bool(false),
        AttrValue::List(vec![]),
        AttrValue::Object(BTreeMap::new()),
    ];
    for (key, other) in [("asset", "src"), ("src", "asset")] {
        for value in &invalid {
            assert_invalid(BTreeMap::from([(key.into(), value.clone())]));
            let mut ambiguous = source_attrs(key);
            ambiguous.insert(other.into(), value.clone());
            assert_invalid(ambiguous);
        }
        let mut ambiguous = source_attrs(key);
        ambiguous.insert(other.into(), AttrValue::String("other".into()));
        assert_invalid(ambiguous);
    }
}

#[test]
fn malformed_metadata_is_rejected_without_coercion_or_raw_changes() {
    for source in ["asset", "src"] {
        for key in ["alt", "title", "width", "height"] {
            let mut invalid = vec![
                AttrValue::Bool(false),
                AttrValue::List(vec![AttrValue::Null]),
                AttrValue::Object(BTreeMap::new()),
            ];
            if matches!(key, "alt" | "title") {
                invalid.push(AttrValue::Integer(240));
            } else {
                invalid.extend([
                    AttrValue::Integer(0),
                    AttrValue::Integer(-1),
                    AttrValue::Integer(i64::MIN),
                    AttrValue::Integer(i64::from(u32::MAX) + 1),
                    AttrValue::Integer(i64::MAX),
                    AttrValue::String("240".into()),
                    AttrValue::String("auto".into()),
                    AttrValue::String(String::new()),
                ]);
            }
            for value in invalid {
                let mut values = source_attrs(source);
                values.insert(key.into(), value);
                assert_invalid(values);
            }
        }
    }
}

#[test]
fn dimensions_are_independent_positive_u32_hints_and_extensions_stay_raw() {
    for key in ["width", "height"] {
        for dimension in [1, u32::MAX] {
            let mut values = source_attrs("asset");
            values.insert(key.into(), AttrValue::Integer(i64::from(dimension)));
            values.insert(
                "extension".into(),
                AttrValue::Object(BTreeMap::from([(
                    "nested".into(),
                    AttrValue::List(vec![AttrValue::Null, AttrValue::String("".into())]),
                )])),
            );
            let raw = NodeAttrs::new(values).unwrap();
            let before = raw.clone();
            let view = ImagePresentationAttrs::read(&raw).unwrap();
            assert_eq!(view.width(), (key == "width").then_some(dimension));
            assert_eq!(view.height(), (key == "height").then_some(dimension));
            assert_eq!(raw, before);
        }
    }
}

#[test]
fn presentation_does_not_relax_the_old_typed_constructor_or_reader() {
    for source in [
        ImageSource::AssetRef("asset:sample".into()),
        ImageSource::ExternalUrl("https://example.invalid/image.png".into()),
    ] {
        for alt in ["", " \t\n"] {
            assert_eq!(
                ImageAttrs::new(source.clone(), alt.into(), None, None, None),
                Err(Error::InvalidImageAttrs)
            );
        }
        let strict = ImageAttrs::new(source, "image".into(), None, None, None).unwrap();
        let strict_raw = strict.to_attrs().unwrap();
        assert_eq!(ImageAttrs::from_attrs(&strict_raw).unwrap(), strict);
        for key in ["alt", "title", "width", "height"] {
            let mut values = strict_raw
                .iter()
                .map(|(key, value)| (key.to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            values.insert(key.into(), AttrValue::Null);
            let raw = NodeAttrs::new(values).unwrap();
            let before = raw.clone();
            assert!(ImagePresentationAttrs::read(&raw).is_ok());
            assert_eq!(ImageAttrs::from_attrs(&raw), Err(Error::InvalidImageAttrs));
            assert_eq!(raw, before);
        }
    }
}
