//! Typed canonical semantics for `NodeKind::Image` atomic blocks (P4.7).
//!
//! An image node stores only stable, serializable, frontend-neutral
//! semantics. Pixels, platform texture handles, host file objects, and host
//! absolute paths never enter the canonical document: the host owns import,
//! persistence, and caching behind an opaque [`ImageSource::AssetRef`], and
//! only codec-imported external URLs travel as [`ImageSource::ExternalUrl`].

use crate::document::{AttrValue, NodeAttrs};
use crate::{Error, Result};

/// Canonical attribute key carrying an external image URL.
pub const IMAGE_ATTR_SRC: &str = "src";
/// Canonical attribute key carrying an opaque host asset reference.
pub const IMAGE_ATTR_ASSET: &str = "asset";
/// Canonical attribute key carrying the required alternative text.
pub const IMAGE_ATTR_ALT: &str = "alt";
/// Canonical attribute key carrying an optional title.
pub const IMAGE_ATTR_TITLE: &str = "title";
/// Canonical attribute key carrying the optional intrinsic width in pixels.
pub const IMAGE_ATTR_WIDTH: &str = "width";
/// Canonical attribute key carrying the optional intrinsic height in pixels.
pub const IMAGE_ATTR_HEIGHT: &str = "height";

/// The frontend-neutral source of one image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// Opaque host-managed reference. The string is stable and portable, but
    /// only the host can resolve it to bytes via the asset capability seam.
    AssetRef(String),
    /// Absolute external URL imported by a codec or the host.
    ExternalUrl(String),
}

impl ImageSource {
    /// Returns the reference or URL string.
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::AssetRef(value) | Self::ExternalUrl(value) => value,
        }
    }
}

/// Typed view of the canonical image attrs on an Image node.
///
/// Construction validates the contract: exactly one source, non-empty
/// alternative text, and positive intrinsic dimensions when present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageAttrs {
    source: ImageSource,
    alt: String,
    title: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

impl ImageAttrs {
    /// Creates validated image attrs.
    ///
    /// The asset reference and external URL must be non-empty; the
    /// alternative text must be non-empty after trimming.
    pub fn new(
        source: ImageSource,
        alt: String,
        title: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<Self> {
        if source.value().trim().is_empty() {
            return Err(Error::InvalidImageAttrs);
        }
        if alt.trim().is_empty() {
            return Err(Error::InvalidImageAttrs);
        }
        if width.is_some_and(|width| width == 0) || height.is_some_and(|height| height == 0) {
            return Err(Error::InvalidImageAttrs);
        }
        Ok(Self {
            source,
            alt,
            title,
            width,
            height,
        })
    }

    /// Returns the image source.
    #[must_use]
    pub const fn source(&self) -> &ImageSource {
        &self.source
    }

    /// Returns the alternative text.
    #[must_use]
    pub fn alt(&self) -> &str {
        &self.alt
    }

    /// Returns the optional title.
    #[must_use]
    pub const fn title(&self) -> Option<&String> {
        self.title.as_ref()
    }

    /// Returns the optional intrinsic width in pixels.
    #[must_use]
    pub const fn width(&self) -> Option<u32> {
        self.width
    }

    /// Returns the optional intrinsic height in pixels.
    #[must_use]
    pub const fn height(&self) -> Option<u32> {
        self.height
    }

    /// Reads typed image attrs from canonical node attrs.
    ///
    /// Exactly one of the source keys must be present; dimensions must be
    /// positive integers. Unknown keys are ignored here and preserved in the
    /// generic attribute container.
    pub fn from_attrs(attrs: &NodeAttrs) -> Result<Self> {
        let src = string_attr(attrs, IMAGE_ATTR_SRC);
        let asset = string_attr(attrs, IMAGE_ATTR_ASSET);
        let source = match (src, asset) {
            (Some(url), None) => ImageSource::ExternalUrl(url),
            (None, Some(asset_ref)) => ImageSource::AssetRef(asset_ref),
            _ => return Err(Error::InvalidImageAttrs),
        };
        let alt = string_attr(attrs, IMAGE_ATTR_ALT).ok_or(Error::InvalidImageAttrs)?;
        let title = string_attr(attrs, IMAGE_ATTR_TITLE);
        let width = dimension_attr(attrs, IMAGE_ATTR_WIDTH)?;
        let height = dimension_attr(attrs, IMAGE_ATTR_HEIGHT)?;
        Self::new(source, alt, title, width, height)
    }

    /// Builds canonical node attrs carrying these image semantics.
    pub fn to_attrs(&self) -> Result<NodeAttrs> {
        let mut values = std::collections::BTreeMap::new();
        match &self.source {
            ImageSource::ExternalUrl(url) => {
                values.insert(IMAGE_ATTR_SRC.to_owned(), AttrValue::String(url.clone()));
            }
            ImageSource::AssetRef(asset_ref) => {
                values.insert(
                    IMAGE_ATTR_ASSET.to_owned(),
                    AttrValue::String(asset_ref.clone()),
                );
            }
        }
        values.insert(
            IMAGE_ATTR_ALT.to_owned(),
            AttrValue::String(self.alt.clone()),
        );
        if let Some(title) = &self.title {
            values.insert(
                IMAGE_ATTR_TITLE.to_owned(),
                AttrValue::String(title.clone()),
            );
        }
        if let Some(width) = self.width {
            values.insert(
                IMAGE_ATTR_WIDTH.to_owned(),
                AttrValue::Integer(i64::from(width)),
            );
        }
        if let Some(height) = self.height {
            values.insert(
                IMAGE_ATTR_HEIGHT.to_owned(),
                AttrValue::Integer(i64::from(height)),
            );
        }
        NodeAttrs::new(values)
    }
}

fn string_attr(attrs: &NodeAttrs, key: &str) -> Option<String> {
    match attrs.get(key) {
        Some(AttrValue::String(value)) => Some(value.clone()),
        _ => None,
    }
}

fn dimension_attr(attrs: &NodeAttrs, key: &str) -> Result<Option<u32>> {
    match attrs.get(key) {
        None => Ok(None),
        Some(AttrValue::Integer(raw)) if *raw > 0 && *raw <= i64::from(u32::MAX) => {
            Ok(Some(*raw as u32))
        }
        _ => Err(Error::InvalidImageAttrs),
    }
}
