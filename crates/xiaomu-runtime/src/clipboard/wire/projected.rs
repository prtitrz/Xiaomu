//! Framed v14 provenance and deterministic plain-text binding.

use super::{ClipboardMetadataError, ClipboardSlice, FORMAT, WireNode, strict_json};
use crate::clipboard::{ClipboardCellRangeRoot, ClipboardSourceBoundary, ClipboardTextProjection};
use serde::{Deserialize, Serialize};

const PREFIX: &str = "xiaomu.clipboard.v14\n";
const FAMILY: &str = "xiaomu.clipboard.";

/// Classified native metadata decoding for frontends with fail-closed routing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardMetadataDecode {
    /// Valid structure whose recomputed text exactly matches the platform body.
    Valid(ClipboardSlice),
    /// Foreign content or a failed historical unframed envelope; old fallback applies.
    ForeignOrLegacyFallback,
    /// Recognized framed native content is invalid; no text or image fallback is safe.
    RejectedNative,
}

/// Decodes metadata while retaining the rejection boundary of framed v14 data.
///
/// Prefix classification precedes JSON parsing, duplicate-key checks and size
/// checks. Every recognized-frame failure rejects, including unknown versions,
/// missing text (frontends must check that separately), malformed/oversized JSON
/// and byte-mismatched plain text. Historical unframed v4-v13 behavior is unchanged.
#[must_use]
pub fn decode_metadata_checked(plain_text: &str, metadata: &str) -> ClipboardMetadataDecode {
    if let Some(body) = metadata.strip_prefix(PREFIX) {
        if metadata.len() > crate::clipboard::export_budget::MAX_BYTES {
            return ClipboardMetadataDecode::RejectedNative;
        }
        return decode(plain_text, body).map_or(
            ClipboardMetadataDecode::RejectedNative,
            ClipboardMetadataDecode::Valid,
        );
    }
    if metadata.starts_with(FAMILY) {
        return ClipboardMetadataDecode::RejectedNative;
    }
    super::decode_legacy(plain_text, metadata).map_or(
        ClipboardMetadataDecode::ForeignOrLegacyFallback,
        ClipboardMetadataDecode::Valid,
    )
}

/// Compatibility decoder returning only valid native fragments.
///
/// Frontends must use [`decode_metadata_checked`] when deciding whether plain
/// text/image fallback is permitted; `None` deliberately loses that distinction.
#[must_use]
pub fn decode_metadata(plain_text: &str, metadata: &str) -> Option<ClipboardSlice> {
    match decode_metadata_checked(plain_text, metadata) {
        ClipboardMetadataDecode::Valid(slice) => Some(slice),
        ClipboardMetadataDecode::ForeignOrLegacyFallback
        | ClipboardMetadataDecode::RejectedNative => None,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    roots: Vec<WireNode>,
    closed: bool,
    source_boundary: Boundary,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "projection"
    )]
    text_projection: Option<Projection>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Boundary {
    Open {},
    WholeRoots {
        open_start: u8,
        open_end: u8,
    },
    CellRange {
        root_form: RootForm,
        open_start: u8,
        open_end: u8,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RootForm {
    Rows,
    Table,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Projection {
    TextBetweenLfV1,
}

fn projection<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Projection>, D::Error> {
    // Missing is the legacy plain algorithm. Explicit null is malformed.
    Projection::deserialize(deserializer).map(Some)
}

impl Boundary {
    fn from_source(source: ClipboardSourceBoundary) -> Self {
        match source {
            ClipboardSourceBoundary::Open => Self::Open {},
            ClipboardSourceBoundary::WholeRoots => Self::WholeRoots {
                open_start: 0,
                open_end: 0,
            },
            ClipboardSourceBoundary::CellRange { root_form } => Self::CellRange {
                root_form: match root_form {
                    ClipboardCellRangeRoot::Rows => RootForm::Rows,
                    ClipboardCellRangeRoot::Table => RootForm::Table,
                },
                open_start: 1,
                open_end: 1,
            },
        }
    }

    fn into_source(self, closed: bool) -> Option<ClipboardSourceBoundary> {
        match self {
            Self::Open {} if !closed => Some(ClipboardSourceBoundary::Open),
            Self::WholeRoots {
                open_start: 0,
                open_end: 0,
            } if closed => Some(ClipboardSourceBoundary::WholeRoots),
            Self::CellRange {
                root_form,
                open_start: 1,
                open_end: 1,
            } if !closed => Some(ClipboardSourceBoundary::CellRange {
                root_form: match root_form {
                    RootForm::Rows => ClipboardCellRangeRoot::Rows,
                    RootForm::Table => ClipboardCellRangeRoot::Table,
                },
            }),
            _ => None,
        }
    }
}

pub(super) fn encode(slice: &ClipboardSlice) -> Result<String, ClipboardMetadataError> {
    // Borrowed budget before WireNode clones or plain-text recomputation.
    crate::clipboard::export_budget::roots(slice.roots())
        .map_err(|()| ClipboardMetadataError::resource_limit())?;
    let source = slice
        .source_boundary()
        .ok_or_else(ClipboardMetadataError::invalid)?;
    let roots = slice
        .roots()
        .iter()
        .map(WireNode::from_node)
        .collect::<Result<Vec<_>, _>>()?;
    let body = serde_json::to_string(&Envelope {
        format: FORMAT.to_owned(),
        version: 14,
        roots,
        closed: slice.is_closed(),
        source_boundary: Boundary::from_source(source),
        text_projection: slice.text_projection().map(|_| Projection::TextBetweenLfV1),
    })
    .map_err(|_| ClipboardMetadataError::serialization())?;
    if body.len() > crate::clipboard::export_budget::MAX_BYTES - PREFIX.len()
        || !strict_json::validate(&body)
    {
        return Err(ClipboardMetadataError::resource_limit());
    }
    // An encoder may not emit metadata with a stale derived body/provenance.
    if decode(slice.plain_text(), &body).as_ref() != Some(slice) {
        return Err(ClipboardMetadataError::invalid());
    }
    Ok(format!("{PREFIX}{body}"))
}

fn decode(plain_text: &str, body: &str) -> Option<ClipboardSlice> {
    if plain_text.len() > crate::clipboard::export_budget::MAX_BYTES || !strict_json::validate(body)
    {
        return None;
    }
    let envelope: Envelope = serde_json::from_str(body).ok()?;
    if envelope.format != FORMAT || envelope.version != 14 {
        return None;
    }
    let source = envelope.source_boundary.into_source(envelope.closed)?;
    let projection = envelope
        .text_projection
        .map(|_| ClipboardTextProjection::TextBetweenLfV1);
    // Strict JSON preflight above bounds all strings, values and nesting before
    // typed DTO normalization. Borrowed root preflight then precedes tree clone.
    let roots = envelope
        .roots
        .into_iter()
        .map(WireNode::into_node)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let slice = ClipboardSlice::from_export_roots(roots, source, projection).ok()?;
    (slice.plain_text() == plain_text).then_some(slice)
}
