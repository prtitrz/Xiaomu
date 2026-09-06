//! Host asset capability seam (P4.7).
//!
//! Xiaomu asks the host to resolve one opaque [`AssetRef`]; the host decides
//! how bytes are stored, fetched, cached, and permitted. The contract is
//! frontend-neutral: no async runtime, no file API, and no GPUI image types
//! cross this seam. Resolve results are delivered through an [`AssetSink`]
//! callback and must be re-validated by the consumer (node identity and
//! [`ResolvedAsset::revision`]) before entering a view cache, because the
//! document may have moved on while the host worked.

use std::rc::Rc;

/// An opaque, stable reference to one host-managed asset.
///
/// The value is host-defined; Xiaomu treats it as an uninterpreted key and
/// never derives storage paths or identities from it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetRef(String);

impl AssetRef {
    /// Creates a validated asset reference.
    ///
    /// The reference must be non-empty after trimming; any further meaning
    /// is host policy.
    pub fn new(value: String) -> Result<Self, AssetError> {
        if value.trim().is_empty() {
            return Err(AssetError::InvalidRef);
        }
        Ok(Self(value))
    }

    /// Returns the opaque reference value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.0
    }
}

/// Host-neutral failure to resolve one asset reference.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssetError {
    /// The reference is empty or otherwise malformed.
    InvalidRef,
    /// The host has no asset for the reference.
    NotFound,
    /// The host denied access to the asset.
    PermissionDenied,
    /// The asset is temporarily unavailable (offline, fetch in flight...).
    Unavailable,
}

/// The declared encoding of one resolved asset payload.
///
/// The host knows what it stored; the frontend uses this to pick its
/// decoder. The set stays intentionally small until hosts need more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetFormat {
    /// PNG-encoded raster data.
    Png,
    /// JPEG-encoded raster data.
    Jpeg,
}

/// The resolved payload for one asset reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAsset {
    asset_ref: AssetRef,
    revision: u64,
    format: AssetFormat,
    bytes: Vec<u8>,
}

impl ResolvedAsset {
    /// Builds one resolved asset payload.
    pub fn new(asset_ref: AssetRef, revision: u64, format: AssetFormat, bytes: Vec<u8>) -> Self {
        Self {
            asset_ref,
            revision,
            format,
            bytes,
        }
    }

    /// Returns the declared payload encoding.
    #[must_use]
    pub const fn format(&self) -> AssetFormat {
        self.format
    }

    /// Returns the reference this payload resolves.
    #[must_use]
    pub const fn asset_ref(&self) -> &AssetRef {
        &self.asset_ref
    }

    /// Returns the host-assigned source revision.
    ///
    /// Consumers compare this against their cache to drop stale resolves.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the renderable bytes.
    ///
    /// The bytes are opaque here: decoding belongs to the frontend, which
    /// knows its own render resource types.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Receives one resolve outcome from the host.
///
/// The callback never mutates the canonical document; consumers hand the
/// payload to their own view cache after stale checks.
pub trait AssetSink: 'static {
    /// Delivers the resolve outcome exactly once per request.
    fn resolved(self: Rc<Self>, result: Result<ResolvedAsset, AssetError>);
}

/// The host-owned capability that turns references into bytes.
///
/// Implementations may resolve synchronously or asynchronously; the only
/// contract is that each request drives exactly one [`AssetSink::resolved`]
/// call.
pub trait AssetService: 'static {
    /// Requests the bytes for one opaque asset reference.
    fn resolve(&self, asset_ref: AssetRef, sink: Rc<dyn AssetSink>);
}
