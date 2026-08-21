//! Extensible canonical node attributes.

use std::collections::BTreeMap;

use crate::{Error, Result};

/// Preservation-friendly value used by canonical node attributes.
///
/// P0 intentionally excludes floating-point values so equality and
/// deterministic comparisons remain straightforward. Codecs may map external
/// numeric formats into one of the supported canonical representations.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttrValue {
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Integer(i64),
    /// UTF-8 string value.
    String(String),
    /// Ordered list of attribute values.
    List(Vec<AttrValue>),
    /// Deterministically ordered object value.
    Object(BTreeMap<String, AttrValue>),
}

/// Immutable canonical attributes attached to one document node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeAttrs {
    values: BTreeMap<String, AttrValue>,
}

impl NodeAttrs {
    /// Creates validated attributes from a deterministic map.
    ///
    /// Attribute keys must be non-empty after trimming. Unknown keys are
    /// preserved; semantic interpretation belongs to node kinds, extensions,
    /// or codecs rather than this generic container.
    pub fn new(values: BTreeMap<String, AttrValue>) -> Result<Self> {
        if values.keys().any(|key| key.trim().is_empty()) {
            return Err(Error::InvalidNodeAttrKey);
        }
        Ok(Self { values })
    }

    /// Returns an empty attribute set.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Returns an attribute by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&AttrValue> {
        self.values.get(key)
    }

    /// Returns attributes in deterministic key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, &AttrValue)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// Returns the number of attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether no attributes are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
