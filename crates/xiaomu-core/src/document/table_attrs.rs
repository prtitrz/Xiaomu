//! Preservation-first interpretation of the three geometry attributes.

use crate::{Error, Result};

use super::{AttrValue, NodeAttrs};

/// Presence of a known table attribute, without default insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableAttribute<T> {
    /// The attribute key is absent.
    Missing,
    /// The key is present with an explicit null value.
    Null,
    /// A value of the expected type is present.
    Value(T),
}

/// Borrowed, validated nonnegative column widths.
///
/// Zero means an unspecified width. Core does not interpret widths as CSS or
/// reconcile conflicting width hints from different rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableColumnWidths<'a>(&'a [AttrValue]);

impl<'a> TableColumnWidths<'a> {
    /// Returns the number of column width entries.
    #[must_use]
    pub fn len(self) -> usize {
        self.0.len()
    }

    /// Returns whether there are no entries.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// Iterates widths in logical column order, retaining unspecified zeros.
    pub fn iter(self) -> impl ExactSizeIterator<Item = usize> + 'a {
        self.0.iter().map(|value| match value {
            AttrValue::Integer(value) => {
                usize::try_from(*value).expect("column width was checked by TableCellAttrs::read")
            }
            _ => unreachable!("column width was checked by TableCellAttrs::read"),
        })
    }
}

/// Read-only typed view of cell geometry; original attrs remain authoritative.
///
/// Reading never inserts defaults, rewrites nulls, or drops unrelated attrs
/// such as alignment, background, or extension metadata. Explicit null spans
/// can be inspected here but are rejected by [`Self::validate_geometry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCellAttrs<'a> {
    colspan: TableAttribute<usize>,
    rowspan: TableAttribute<usize>,
    colwidth: TableAttribute<TableColumnWidths<'a>>,
}

impl<'a> TableCellAttrs<'a> {
    /// Reads known fields without copying or modifying the attribute map.
    ///
    /// Non-positive/non-integer spans and non-integer/negative widths return
    /// [`Error::InvalidTableAttrs`]. Missing and null remain distinct states.
    pub fn read(attrs: &'a NodeAttrs) -> Result<Self> {
        let colspan = read_span(attrs.get("colspan"))?;
        let rowspan = read_span(attrs.get("rowspan"))?;
        let colwidth = match attrs.get("colwidth") {
            None => TableAttribute::Missing,
            Some(AttrValue::Null) => TableAttribute::Null,
            Some(AttrValue::List(values)) => {
                if let Ok(span) = effective_span(colspan)
                    && values.len() != span
                {
                    return Err(Error::InvalidTableAttrs);
                }
                if !values.iter().all(|value| {
                    matches!(value, AttrValue::Integer(value) if usize::try_from(*value).is_ok())
                }) {
                    return Err(Error::InvalidTableAttrs);
                }
                TableAttribute::Value(TableColumnWidths(values))
            }
            Some(_) => return Err(Error::InvalidTableAttrs),
        };
        Ok(Self {
            colspan,
            rowspan,
            colwidth,
        })
    }

    /// Returns the exact column-span presence and parsed positive value.
    #[must_use]
    pub const fn colspan(self) -> TableAttribute<usize> {
        self.colspan
    }

    /// Returns the exact row-span presence and parsed positive value.
    #[must_use]
    pub const fn rowspan(self) -> TableAttribute<usize> {
        self.rowspan
    }

    /// Returns width-list presence without collapsing null into missing.
    #[must_use]
    pub const fn colwidth(self) -> TableAttribute<TableColumnWidths<'a>> {
        self.colwidth
    }

    /// Returns the effective column span: missing is one; null is invalid.
    pub fn effective_colspan(self) -> Result<usize> {
        effective_span(self.colspan)
    }

    /// Returns the effective row span: missing is one; null is invalid.
    pub fn effective_rowspan(self) -> Result<usize> {
        effective_span(self.rowspan)
    }

    /// Checks effective spans and the width-list length, without normalizing.
    pub fn validate_geometry(self) -> Result<()> {
        let colspan = self.effective_colspan()?;
        self.effective_rowspan()?;
        if let TableAttribute::Value(widths) = self.colwidth
            && widths.len() != colspan
        {
            return Err(Error::InvalidTableAttrs);
        }
        Ok(())
    }
}

fn read_span(value: Option<&AttrValue>) -> Result<TableAttribute<usize>> {
    match value {
        None => Ok(TableAttribute::Missing),
        Some(AttrValue::Null) => Ok(TableAttribute::Null),
        Some(AttrValue::Integer(value)) if *value > 0 => usize::try_from(*value)
            .map(TableAttribute::Value)
            .map_err(|_| Error::InvalidTableAttrs),
        Some(_) => Err(Error::InvalidTableAttrs),
    }
}

fn effective_span(span: TableAttribute<usize>) -> Result<usize> {
    match span {
        TableAttribute::Missing => Ok(1),
        TableAttribute::Null => Err(Error::InvalidTableAttrs),
        TableAttribute::Value(value) => Ok(value),
    }
}
