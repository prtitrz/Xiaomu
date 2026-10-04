//! Preflight for the owned cell-attribute expansion caused by splitting.
//!
//! These limits are separate from logical-grid budgets. They account for the
//! projected output cells' owned key/string bytes, attribute values and their
//! inline storage. They exclude tree nodes, BTree allocation overhead, allocator
//! capacity, and transient/inverse copies; this is not a process-memory sandbox.

use std::mem::size_of;

use crate::document::{AttrValue, NodeAttrs};
use crate::{Error, Result};

const MAX_OUTPUT_ATTR_BYTES: usize = 64 * 1024 * 1024;
const MAX_OUTPUT_ATTR_VALUES: usize = 1_000_000;
const MAX_ATTR_DEPTH: usize = 64;

pub(super) fn check_split_attrs(attrs: &NodeAttrs, cells: usize) -> Result<()> {
    let mut budget = AttributeExpansion {
        bytes: 0,
        values: 0,
        copies: cells,
    };
    budget.charge(size_of::<NodeAttrs>(), 0)?;
    for (key, value) in attrs.iter() {
        budget.key(key)?;
        if key == "colwidth" && matches!(value, AttrValue::List(_)) {
            // Each output cell gets exactly one checked integer width. Never
            // copy or repeatedly walk the full input list for this projection.
            budget.charge(
                size_of::<AttrValue>()
                    .checked_mul(2)
                    .ok_or(Error::TableResourceLimit)?,
                2,
            )?;
        } else {
            budget.value(value, 0)?;
        }
    }
    Ok(())
}

struct AttributeExpansion {
    bytes: usize,
    values: usize,
    copies: usize,
}

impl AttributeExpansion {
    fn charge(&mut self, bytes: usize, values: usize) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or(Error::TableResourceLimit)?;
        self.values = self
            .values
            .checked_add(values)
            .ok_or(Error::TableResourceLimit)?;
        let bytes = self
            .bytes
            .checked_mul(self.copies)
            .ok_or(Error::TableResourceLimit)?;
        let values = self
            .values
            .checked_mul(self.copies)
            .ok_or(Error::TableResourceLimit)?;
        if bytes > MAX_OUTPUT_ATTR_BYTES || values > MAX_OUTPUT_ATTR_VALUES {
            return Err(Error::TableResourceLimit);
        }
        Ok(())
    }

    fn key(&mut self, key: &str) -> Result<()> {
        self.charge(
            size_of::<String>()
                .checked_add(key.len())
                .ok_or(Error::TableResourceLimit)?,
            0,
        )
    }

    fn value(&mut self, value: &AttrValue, depth: usize) -> Result<()> {
        if depth >= MAX_ATTR_DEPTH {
            return Err(Error::TableResourceLimit);
        }
        self.charge(size_of::<AttrValue>(), 1)?;
        match value {
            AttrValue::String(value) => self.charge(value.len(), 0)?,
            AttrValue::List(values) => {
                for value in values {
                    self.value(value, depth + 1)?;
                }
            }
            AttrValue::Object(values) => {
                for (key, value) in values {
                    self.key(key)?;
                    self.value(value, depth + 1)?;
                }
            }
            AttrValue::Null | AttrValue::Bool(_) | AttrValue::Integer(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn attrs(value: AttrValue) -> NodeAttrs {
        NodeAttrs::new(BTreeMap::from([("opaque".to_owned(), value)])).unwrap()
    }

    #[test]
    fn rejects_multiplicative_string_expansion_without_allocating_outputs() {
        let source = attrs(AttrValue::String("x".repeat(16 * 1024)));
        assert_eq!(
            check_split_attrs(&source, 10_000),
            Err(Error::TableResourceLimit)
        );
        assert!(check_split_attrs(&source, 2).is_ok());
    }

    #[test]
    fn counts_nested_keys_values_and_limits_depth() {
        let source = attrs(AttrValue::Object(BTreeMap::from([(
            "k".into(),
            AttrValue::List(vec![AttrValue::Null; 10]),
        )])));
        assert_eq!(
            check_split_attrs(&source, 100_000),
            Err(Error::TableResourceLimit)
        );
        let mut nested = AttrValue::Null;
        for _ in 0..MAX_ATTR_DEPTH {
            nested = AttrValue::List(vec![nested]);
        }
        assert_eq!(
            check_split_attrs(&attrs(nested), 2),
            Err(Error::TableResourceLimit)
        );
    }

    #[test]
    fn width_projection_counts_only_the_single_output_column() {
        let source = NodeAttrs::new(BTreeMap::from([
            ("colspan".into(), AttrValue::Integer(10_000)),
            (
                "colwidth".into(),
                AttrValue::List(vec![AttrValue::Integer(0); 10_000]),
            ),
        ]))
        .unwrap();
        assert!(check_split_attrs(&source, 10_000).is_ok());
    }

    #[test]
    fn projected_size_arithmetic_is_checked() {
        assert_eq!(
            check_split_attrs(&NodeAttrs::empty(), usize::MAX),
            Err(Error::TableResourceLimit)
        );
    }
}
