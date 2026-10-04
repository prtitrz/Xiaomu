//! Read-only JSON preflight: no object may silently overwrite a repeated key.

use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

/// Validates recursively without constructing or rewriting a JSON value.
pub(super) struct UniqueFields;

impl<'de> Deserialize<'de> for UniqueFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueFieldsVisitor)
    }
}

struct UniqueFieldsVisitor;

impl<'de> Visitor<'de> for UniqueFieldsVisitor {
    type Value = UniqueFields;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E: Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_i64<E: Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_u64<E: Error>(self, _: u64) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_f64<E: Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_str<E: Error>(self, _: &str) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueFields)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<Self::Value, A::Error> {
        while values.next_element::<UniqueFields>()?.is_some() {}
        Ok(UniqueFields)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<Self::Value, A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = values.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
            values.next_value::<UniqueFields>()?;
        }
        Ok(UniqueFields)
    }
}
