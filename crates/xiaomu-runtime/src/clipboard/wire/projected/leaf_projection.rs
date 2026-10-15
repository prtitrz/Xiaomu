//! Strict, bounded descriptor wire; keys always mean extension AtomKinds.

use std::fmt;

use serde::{Deserialize, Serialize, de};
use xiaomu_core::document::AtomKind;

use crate::clipboard::{ClipboardLeafKinds, ClipboardTextProjection};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LeafProjection {
    kind: Version,
    #[serde(deserialize_with = "atom_kinds")]
    atom_kinds: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Version {
    TextBetweenLfWithAtomsV1,
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Name;
        impl de::Visitor<'_> for Name {
            type Value = Version;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("the text_between_lf_with_atoms_v1 string")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<Version, E> {
                match name {
                    "text_between_lf_with_atoms_v1" => Ok(Version::TextBetweenLfWithAtomsV1),
                    _ => Err(E::custom("unknown clipboard leaf projection")),
                }
            }
        }
        // A derived unit-enum reader also accepts an externally tagged object.
        // The descriptor version is deliberately a string, never that shape.
        deserializer.deserialize_str(Name)
    }
}

impl LeafProjection {
    pub(super) fn from_kinds(kinds: &ClipboardLeafKinds) -> Self {
        Self {
            kind: Version::TextBetweenLfWithAtomsV1,
            atom_kinds: kinds
                .atom_kinds()
                .iter()
                .map(|kind| kind.as_str().to_owned())
                .collect(),
        }
    }

    pub(super) fn into_projection(self) -> Option<ClipboardTextProjection> {
        // Wire descriptors are canonical, not normalized after untrusted input.
        if self.atom_kinds.windows(2).any(|pair| pair[0] >= pair[1]) {
            return None;
        }
        let kinds = self
            .atom_kinds
            .into_iter()
            .map(AtomKind::new)
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        ClipboardTextProjection::text_between_lf_with_atoms(kinds).ok()
    }
}

fn atom_kinds<'de, D: de::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    struct Kinds;
    impl<'de> de::Visitor<'de> for Kinds {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("1–32 exact extension keys, each at most 128 UTF-8 bytes")
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut keys = Vec::new();
            while let Some(key) = seq.next_element_seed(Key)? {
                if keys.len() == 32 {
                    return Err(de::Error::custom("too many clipboard leaf kinds"));
                }
                keys.push(key);
            }
            if keys.is_empty() {
                return Err(de::Error::custom("empty clipboard leaf kinds"));
            }
            Ok(keys)
        }
    }
    deserializer.deserialize_seq(Kinds)
}

struct Key;
impl<'de> de::DeserializeSeed<'de> for Key {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl de::Visitor<'_> for Key {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a nonempty extension key of at most 128 UTF-8 bytes")
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<String, E> {
        if key.trim().is_empty() || key.len() > 128 {
            return Err(E::custom("invalid clipboard leaf kind"));
        }
        Ok(key.to_owned())
    }
}
