//! Read-only image presentation over preserved canonical attributes.

use crate::document::{
    AttrValue, IMAGE_ATTR_ALT, IMAGE_ATTR_ASSET, IMAGE_ATTR_HEIGHT, IMAGE_ATTR_SRC,
    IMAGE_ATTR_TITLE, IMAGE_ATTR_WIDTH, NodeAttrs,
};
use crate::{Error, Result};

/// An image source borrowed from canonical attributes, without resolving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSourceRef<'a> {
    /// Opaque host-managed reference; only the host can resolve its meaning.
    AssetRef(&'a str),
    /// External URL supplied by a codec or host; reading does not fetch it.
    ExternalUrl(&'a str),
}

impl<'a> ImageSourceRef<'a> {
    /// Returns the original reference or URL without trimming or allocating.
    #[must_use]
    pub const fn value(self) -> &'a str {
        match self {
            Self::AssetRef(value) | Self::ExternalUrl(value) => value,
        }
    }
}

/// Borrowed, validated display values for an existing image's raw attributes.
///
/// Missing and explicit null metadata have the same presentation but remain
/// distinct in the canonical [`NodeAttrs`]. Strings, including empty strings,
/// are borrowed unchanged. This view is not a persistence representation:
/// never reconstruct canonical attributes from it. Host codecs and edit
/// policies still decide which raw attributes may be imported or saved.
/// The strict [`super::ImageAttrs`] construction contract remains separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePresentationAttrs<'a> {
    source: ImageSourceRef<'a>,
    alt: Option<&'a str>,
    title: Option<&'a str>,
    width: Option<u32>,
    height: Option<u32>,
}

impl<'a> ImagePresentationAttrs<'a> {
    /// Reads image display values without modifying or normalizing `attrs`.
    ///
    /// Exactly one of `asset` and `src` must exist and contain a string that
    /// is non-empty after trimming. Even a null second source is ambiguous.
    /// Alternative text and title accept missing, null, or string values;
    /// dimensions accept missing, null, or positive integers fitting `u32`.
    /// Invalid known values return [`Error::InvalidImageAttrs`]. Unknown
    /// attributes are ignored by this view and preserved in the raw map.
    pub fn read(attrs: &'a NodeAttrs) -> Result<Self> {
        let source = match (attrs.get(IMAGE_ATTR_ASSET), attrs.get(IMAGE_ATTR_SRC)) {
            (Some(AttrValue::String(value)), None) if !value.trim().is_empty() => {
                ImageSourceRef::AssetRef(value)
            }
            (None, Some(AttrValue::String(value))) if !value.trim().is_empty() => {
                ImageSourceRef::ExternalUrl(value)
            }
            _ => return Err(Error::InvalidImageAttrs),
        };
        Ok(Self {
            source,
            alt: nullable_string(attrs, IMAGE_ATTR_ALT)?,
            title: nullable_string(attrs, IMAGE_ATTR_TITLE)?,
            width: nullable_dimension(attrs, IMAGE_ATTR_WIDTH)?,
            height: nullable_dimension(attrs, IMAGE_ATTR_HEIGHT)?,
        })
    }

    /// Returns the borrowed source; this does not grant permission to fetch it.
    #[must_use]
    pub const fn source(&self) -> ImageSourceRef<'a> {
        self.source
    }

    /// Returns alternative text, preserving `Some("")` separately from `None`.
    #[must_use]
    pub const fn alt(&self) -> Option<&'a str> {
        self.alt
    }

    /// Returns the title verbatim; missing and null both display as `None`.
    #[must_use]
    pub const fn title(&self) -> Option<&'a str> {
        self.title
    }

    /// Returns an optional positive pixel-width hint, not a layout instruction.
    #[must_use]
    pub const fn width(&self) -> Option<u32> {
        self.width
    }

    /// Returns an optional positive pixel-height hint, not a layout instruction.
    #[must_use]
    pub const fn height(&self) -> Option<u32> {
        self.height
    }
}

fn nullable_string<'a>(attrs: &'a NodeAttrs, key: &str) -> Result<Option<&'a str>> {
    match attrs.get(key) {
        None | Some(AttrValue::Null) => Ok(None),
        Some(AttrValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(Error::InvalidImageAttrs),
    }
}

fn nullable_dimension(attrs: &NodeAttrs, key: &str) -> Result<Option<u32>> {
    match attrs.get(key) {
        None | Some(AttrValue::Null) => Ok(None),
        Some(AttrValue::Integer(value)) if *value > 0 && *value <= i64::from(u32::MAX) => {
            Ok(Some(*value as u32))
        }
        Some(_) => Err(Error::InvalidImageAttrs),
    }
}
