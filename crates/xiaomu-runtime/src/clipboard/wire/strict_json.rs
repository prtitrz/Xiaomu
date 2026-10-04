//! Streaming preflight for duplicate keys and bounded untrusted metadata.
//!
//! These resource limits apply to every envelope version. They do not change
//! the meaning or encoding of historical schemas; oversized payloads now fail
//! closed instead of consuming unbounded decoder resources.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserializer;
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_VALUES: usize = 100_000;
const MAX_DEPTH: usize = 128;

/// Reads the original bytes without building a `Value` tree or rewriting any
/// objects before the typed decoder sees them. Keys are compared after JSON
/// escape decoding, so escaped spellings cannot hide duplicates.
pub(super) fn validate(metadata: &str) -> bool {
    if metadata.len() > MAX_BYTES {
        return false;
    }
    let mut remaining = MAX_VALUES;
    let mut deserializer = serde_json::Deserializer::from_str(metadata);
    CheckedValue {
        remaining: &mut remaining,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .is_ok()
        && deserializer.end().is_ok()
}

struct CheckedValue<'a> {
    remaining: &'a mut usize,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for CheckedValue<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        if *self.remaining == 0 || self.depth > MAX_DEPTH {
            return Err(D::Error::custom(
                "clipboard metadata resource limit exceeded",
            ));
        }
        *self.remaining -= 1;
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for CheckedValue<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON without duplicate object keys")
    }

    fn visit_bool<E: Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        while values
            .next_element_seed(CheckedValue {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut values: A) -> Result<(), A::Error> {
        let mut keys = BTreeSet::new();
        while let Some(key) = values.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
            values.next_value_seed(CheckedValue {
                remaining: self.remaining,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
}
