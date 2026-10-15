//! Framed provenance and deterministic plain-text binding.

use super::{ClipboardMetadataError, ClipboardSlice, FORMAT, WireNode, strict_json};
use crate::clipboard::{ClipboardCellRangeRoot, ClipboardSourceBoundary, ClipboardTextProjection};
use serde::{Deserialize, Serialize};

mod leaf_projection;
use leaf_projection::LeafProjection;

const PREFIX: &str = "xiaomu.clipboard.v14\n";
const LEAF_PREFIX: &str = "xiaomu.clipboard.v15\n";
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

/// Decodes metadata while retaining the rejection boundary of framed native data.
///
/// Prefix classification precedes JSON parsing, duplicate-key checks and size
/// checks. Every recognized-frame failure rejects, including unknown versions,
/// missing text (frontends must check that separately), malformed/oversized JSON
/// and byte-mismatched plain text. Historical unframed v4-v13 behavior is unchanged.
#[must_use]
pub fn decode_metadata_checked(plain_text: &str, metadata: &str) -> ClipboardMetadataDecode {
    let framed = metadata
        .strip_prefix(PREFIX)
        .map(|body| (body, 14))
        .or_else(|| metadata.strip_prefix(LEAF_PREFIX).map(|body| (body, 15)));
    if let Some((body, version)) = framed {
        if metadata.len() > crate::clipboard::export_budget::MAX_BYTES {
            return ClipboardMetadataDecode::RejectedNative;
        }
        return decode(plain_text, body, version).map_or(
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

#[derive(Serialize)]
#[serde(untagged)]
enum Projection {
    Legacy(LegacyProjection),
    Leaf(LeafProjection),
}

impl<'de> Deserialize<'de> for Projection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Projection;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a known text projection string or bounded leaf descriptor")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Projection, E> {
                match value {
                    "text_between_lf_v1" => {
                        Ok(Projection::Legacy(LegacyProjection::TextBetweenLfV1))
                    }
                    _ => Err(E::custom("unknown clipboard text projection")),
                }
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Projection, A::Error> {
                LeafProjection::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(Projection::Leaf)
            }
        }
        // Do not use untagged Deserialize: it buffers the entire descriptor
        // before its per-key/count checks. Dispatch directly to bounded readers.
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyProjection {
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
    crate::clipboard::export_budget::roots(slice.roots(), slice.text_projection().as_ref())
        .map_err(|()| ClipboardMetadataError::resource_limit())?;
    let source = slice
        .source_boundary()
        .ok_or_else(ClipboardMetadataError::invalid)?;
    let roots = slice
        .roots()
        .iter()
        .map(WireNode::from_node)
        .collect::<Result<Vec<_>, _>>()?;
    let (version, prefix, text_projection) = match slice.text_projection() {
        None => (14, PREFIX, None),
        Some(ClipboardTextProjection::TextBetweenLfV1) => (
            14,
            PREFIX,
            Some(Projection::Legacy(LegacyProjection::TextBetweenLfV1)),
        ),
        Some(ClipboardTextProjection::TextBetweenLfWithAtomsV1(kinds)) => (
            15,
            LEAF_PREFIX,
            Some(Projection::Leaf(LeafProjection::from_kinds(&kinds))),
        ),
    };
    let body = serde_json::to_string(&Envelope {
        format: FORMAT.to_owned(),
        version,
        roots,
        closed: slice.is_closed(),
        source_boundary: Boundary::from_source(source),
        text_projection,
    })
    .map_err(|_| ClipboardMetadataError::serialization())?;
    if body.len() > crate::clipboard::export_budget::MAX_BYTES - prefix.len()
        || !strict_json::validate(&body)
    {
        return Err(ClipboardMetadataError::resource_limit());
    }
    // An encoder may not emit metadata with a stale derived body/provenance.
    if decode(slice.plain_text(), &body, version).as_ref() != Some(slice) {
        return Err(ClipboardMetadataError::invalid());
    }
    Ok(format!("{prefix}{body}"))
}

fn decode(plain_text: &str, body: &str, version: u32) -> Option<ClipboardSlice> {
    if plain_text.len() > crate::clipboard::export_budget::MAX_BYTES || !strict_json::validate(body)
    {
        return None;
    }
    // Derived struct readers can accept tuple arrays. The new envelope is an
    // object only; historical v14 decoding remains unchanged.
    if version == 15 && !body.trim_start().starts_with('{') {
        return None;
    }
    let envelope: Envelope = serde_json::from_str(body).ok()?;
    if envelope.format != FORMAT || envelope.version != version {
        return None;
    }
    let source = envelope.source_boundary.into_source(envelope.closed)?;
    let projection = match (version, envelope.text_projection) {
        (14, None) => None,
        (14, Some(Projection::Legacy(LegacyProjection::TextBetweenLfV1))) => {
            Some(ClipboardTextProjection::TextBetweenLfV1)
        }
        (15, Some(Projection::Leaf(leaf))) => Some(leaf.into_projection()?),
        _ => return None,
    };
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
