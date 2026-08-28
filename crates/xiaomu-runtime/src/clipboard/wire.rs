//! Versioned Xiaomu clipboard metadata codec.
//!
//! The system clipboard keeps ordinary plain text as its interoperable body.
//! Xiaomu metadata is an additional, versioned JSON value carried by the
//! frontend transport. Core canonical types do not derive or depend on serde;
//! this module converts them through private wire DTOs instead.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use xiaomu_core::document::{
    AttrValue, HeadingLevel, InlineContent, LinkMark, Mark, MarkSet, NodeAttrs, NodeKind, TextRun,
};

use super::{ClipboardBlock, ClipboardSlice};

const FORMAT: &str = "xiaomu.clipboard";
const VERSION: u32 = 1;

/// Failure to encode a Xiaomu structured clipboard slice.
///
/// Decoding intentionally uses an `Option` instead: platform clipboard
/// metadata is untrusted and foreign/obsolete values should quietly fall back
/// to their plain-text body rather than becoming an editor error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardMetadataError {
    message: &'static str,
}

impl ClipboardMetadataError {
    const fn unsupported() -> Self {
        Self {
            message: "clipboard slice contains a value unsupported by metadata v1",
        }
    }

    const fn invalid() -> Self {
        Self {
            message: "clipboard metadata value is invalid",
        }
    }

    const fn serialization() -> Self {
        Self {
            message: "clipboard metadata could not be serialized",
        }
    }
}

impl fmt::Display for ClipboardMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ClipboardMetadataError {}

/// Encodes `slice` into Xiaomu's versioned clipboard metadata JSON.
///
/// The plain-text fallback is deliberately not duplicated in the metadata;
/// callers put [`ClipboardSlice::plain_text`] in the platform text flavor.
pub fn encode_metadata(slice: &ClipboardSlice) -> Result<String, ClipboardMetadataError> {
    let blocks = slice
        .blocks()
        .iter()
        .map(WireBlock::from_block)
        .collect::<Result<Vec<_>, _>>()?;
    serde_json::to_string(&WireEnvelope {
        format: FORMAT.to_owned(),
        version: VERSION,
        blocks,
    })
    .map_err(|_| ClipboardMetadataError::serialization())
}

/// Decodes Xiaomu metadata when it matches `plain_text` exactly.
///
/// Unknown versions, malformed/foreign metadata, unsupported canonical
/// values, and stale metadata whose computed fallback differs from the
/// platform text all return `None`. The caller should then paste the supplied
/// plain text normally.
#[must_use]
pub fn decode_metadata(plain_text: &str, metadata: &str) -> Option<ClipboardSlice> {
    let envelope: WireEnvelope = serde_json::from_str(metadata).ok()?;
    if envelope.format != FORMAT || envelope.version != VERSION {
        return None;
    }
    let blocks = envelope
        .blocks
        .into_iter()
        .map(WireBlock::into_block)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if blocks.is_empty() {
        return None;
    }
    let slice = ClipboardSlice::new(blocks);
    (slice.plain_text() == plain_text).then_some(slice)
}

#[derive(Serialize, Deserialize)]
struct WireEnvelope {
    format: String,
    version: u32,
    blocks: Vec<WireBlock>,
}

#[derive(Serialize, Deserialize)]
struct WireBlock {
    kind: WireKind,
    attrs: BTreeMap<String, WireAttr>,
    runs: Vec<WireRun>,
}

impl WireBlock {
    fn from_block(block: &ClipboardBlock) -> Result<Self, ClipboardMetadataError> {
        Ok(Self {
            kind: WireKind::from_kind(block.kind())?,
            attrs: block
                .attrs()
                .iter()
                .map(|(key, value)| Ok((key.to_owned(), WireAttr::from_attr(value)?)))
                .collect::<Result<_, ClipboardMetadataError>>()?,
            runs: block
                .inline()
                .runs()
                .iter()
                .map(WireRun::from_run)
                .collect::<Result<_, _>>()?,
        })
    }

    fn into_block(self) -> Result<ClipboardBlock, ClipboardMetadataError> {
        let attrs = self
            .attrs
            .into_iter()
            .map(|(key, value)| Ok((key, value.into_attr()?)))
            .collect::<Result<BTreeMap<_, _>, ClipboardMetadataError>>()?;
        let runs = self
            .runs
            .into_iter()
            .map(WireRun::into_run)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ClipboardBlock::new(
            self.kind.into_kind()?,
            NodeAttrs::new(attrs).map_err(|_| ClipboardMetadataError::invalid())?,
            InlineContent::new(runs).map_err(|_| ClipboardMetadataError::invalid())?,
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum WireKind {
    Paragraph,
    Heading(u8),
    CodeBlock,
    Custom(String),
}

impl WireKind {
    fn from_kind(kind: &NodeKind) -> Result<Self, ClipboardMetadataError> {
        match kind {
            NodeKind::Paragraph => Ok(Self::Paragraph),
            NodeKind::Heading(level) => Ok(Self::Heading(level.as_u8())),
            NodeKind::CodeBlock => Ok(Self::CodeBlock),
            NodeKind::Custom(key) => Ok(Self::Custom(key.clone())),
            _ => Err(ClipboardMetadataError::unsupported()),
        }
    }

    fn into_kind(self) -> Result<NodeKind, ClipboardMetadataError> {
        match self {
            Self::Paragraph => Ok(NodeKind::Paragraph),
            Self::Heading(level) => HeadingLevel::new(level)
                .map(NodeKind::Heading)
                .map_err(|_| ClipboardMetadataError::invalid()),
            Self::CodeBlock => Ok(NodeKind::CodeBlock),
            Self::Custom(key) => {
                NodeKind::custom(key).map_err(|_| ClipboardMetadataError::invalid())
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum WireAttr {
    Bool(bool),
    Integer(i64),
    String(String),
    List(Vec<WireAttr>),
    Object(BTreeMap<String, WireAttr>),
}

impl WireAttr {
    fn from_attr(value: &AttrValue) -> Result<Self, ClipboardMetadataError> {
        match value {
            AttrValue::Bool(value) => Ok(Self::Bool(*value)),
            AttrValue::Integer(value) => Ok(Self::Integer(*value)),
            AttrValue::String(value) => Ok(Self::String(value.clone())),
            AttrValue::List(values) => Ok(Self::List(
                values
                    .iter()
                    .map(Self::from_attr)
                    .collect::<Result<_, _>>()?,
            )),
            AttrValue::Object(values) => Ok(Self::Object(
                values
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), Self::from_attr(value)?)))
                    .collect::<Result<_, ClipboardMetadataError>>()?,
            )),
            _ => Err(ClipboardMetadataError::unsupported()),
        }
    }

    fn into_attr(self) -> Result<AttrValue, ClipboardMetadataError> {
        match self {
            Self::Bool(value) => Ok(AttrValue::Bool(value)),
            Self::Integer(value) => Ok(AttrValue::Integer(value)),
            Self::String(value) => Ok(AttrValue::String(value)),
            Self::List(values) => Ok(AttrValue::List(
                values
                    .into_iter()
                    .map(Self::into_attr)
                    .collect::<Result<_, _>>()?,
            )),
            Self::Object(values) => Ok(AttrValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, value.into_attr()?)))
                    .collect::<Result<_, ClipboardMetadataError>>()?,
            )),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct WireRun {
    text: String,
    marks: Vec<WireMark>,
}

impl WireRun {
    fn from_run(run: &TextRun) -> Result<Self, ClipboardMetadataError> {
        Ok(Self {
            text: run.text().as_str().to_owned(),
            marks: run
                .marks()
                .as_slice()
                .iter()
                .map(WireMark::from_mark)
                .collect::<Result<_, _>>()?,
        })
    }

    fn into_run(self) -> Result<TextRun, ClipboardMetadataError> {
        let marks = self
            .marks
            .into_iter()
            .map(WireMark::into_mark)
            .collect::<Result<Vec<_>, _>>()?;
        TextRun::new(
            self.text,
            MarkSet::new(marks).map_err(|_| ClipboardMetadataError::invalid())?,
        )
        .map_err(|_| ClipboardMetadataError::invalid())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireMark {
    Bold,
    Italic,
    Code,
    Underline,
    Strike,
    Link { href: String, title: Option<String> },
}

impl WireMark {
    fn from_mark(mark: &Mark) -> Result<Self, ClipboardMetadataError> {
        match mark {
            Mark::Bold => Ok(Self::Bold),
            Mark::Italic => Ok(Self::Italic),
            Mark::Code => Ok(Self::Code),
            Mark::Underline => Ok(Self::Underline),
            Mark::Strike => Ok(Self::Strike),
            Mark::Link(link) => Ok(Self::Link {
                href: link.href().to_owned(),
                title: link.title().map(str::to_owned),
            }),
            _ => Err(ClipboardMetadataError::unsupported()),
        }
    }

    fn into_mark(self) -> Result<Mark, ClipboardMetadataError> {
        Ok(match self {
            Self::Bold => Mark::Bold,
            Self::Italic => Mark::Italic,
            Self::Code => Mark::Code,
            Self::Underline => Mark::Underline,
            Self::Strike => Mark::Strike,
            Self::Link { href, title } => Mark::Link(LinkMark::new(href, title)),
        })
    }
}
