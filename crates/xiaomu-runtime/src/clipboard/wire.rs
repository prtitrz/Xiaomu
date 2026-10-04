//! Versioned Xiaomu clipboard metadata codec.
//!
//! The system clipboard keeps ordinary plain text as its interoperable body.
//! Xiaomu metadata is an additional, versioned JSON value carried by the
//! frontend transport. Core canonical types do not derive or depend on serde;
//! this module converts them through private wire DTOs instead.

use std::collections::BTreeMap;
use std::fmt;

mod atoms;
mod marks;
mod strict_json;
use atoms::WireAtom;
use marks::WireRun;

use serde::{Deserialize, Serialize};
use xiaomu_core::document::{AttrValue, HeadingLevel, NodeAttrs, NodeKind};
use xiaomu_core::text::TextBuffer;

use super::fragment::{
    ClipboardInline, ClipboardNode, ClipboardNodeContent, ClipboardSlice, validate_roots,
};

const FORMAT: &str = "xiaomu.clipboard";
// v1 carried only a flat leaf list. v2 carries the detached fragment tree so
// list/quote/container semantics survive Xiaomu-to-Xiaomu copy/paste. v3 adds
// detached inline-atom payloads (kind, attrs, fallback_text) anchored inside
// the fragment text. v4 adds whole atomic blocks (HorizontalRule, Image, ...)
// captured as kind + attrs with no editable interior. v5 adds rectangular
// table selections (`ClipboardNodeContent::Table`); only slices that actually
// carry a table bump the envelope, so v4 readers fail soft to plain text on
// exactly the payloads they cannot represent.
const VERSION: u32 = 4;
const VERSION_TABLE: u32 = 5;
// Row attributes require a new envelope so v5 readers cannot silently drop them.
const VERSION_TABLE_ROW_ATTRS: u32 = 6;
// Explicit null is a new tagged attr variant, including in nested values.
// Keep older envelopes for null-free fragments; older readers reject v7.
const VERSION_NULL_ATTRS: u32 = 7;
// Exact five-field link attributes need a new mark variant, not an extension
// to the old Link DTO that an older reader could silently truncate.
const VERSION_LINK_ATTRIBUTES: u32 = 8;
// TextStyle is a new semantic mark even when all three fields are missing.
const VERSION_TEXT_STYLE: u32 = 9;
// Built-in atom identity and independent atom marks need an explicit tagged
// kind, never a string projection that could turn a built-in into an extension.
const VERSION_TYPED_ATOMS: u32 = 10;

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
            message: "clipboard slice contains a value unsupported by metadata",
        }
    }

    const fn invalid() -> Self {
        Self {
            message: "clipboard metadata value is invalid",
        }
    }

    const fn resource_limit() -> Self {
        Self {
            message: "clipboard metadata exceeds supported resource limits",
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
/// Built-in hard breaks or independently marked inline atoms encode as v10,
/// preserving typed kind identity and the complete mark set. Otherwise,
/// text-style marks encode as v9, preserving all three missing/null/string
/// attributes without interpreting CSS. Otherwise, links outside the classic
/// href/title form encode as v8, preserving all
/// five fields and their missing/null/string distinctions. Otherwise,
/// fragments containing explicit null node attributes encode as v7;
/// tables with nonempty row attributes encode as v6; other tables stay at
/// v5 and non-table fragments at v4. Older readers fall back to plain text
/// only for payloads whose semantics they cannot preserve.
///
/// All versions share a resource boundary: at most 16 MiB of metadata,
/// 100,000 JSON values and nesting within 128 levels (also subject to serde's
/// recursion limit). Encoding rejects output outside the decoding budget.
pub fn encode_metadata(slice: &ClipboardSlice) -> Result<String, ClipboardMetadataError> {
    let roots = slice
        .roots()
        .iter()
        .map(WireNode::from_node)
        .collect::<Result<Vec<_>, _>>()?;
    let version = if roots.iter().any(WireNode::carries_typed_atoms) {
        VERSION_TYPED_ATOMS
    } else if roots.iter().any(WireNode::carries_text_style) {
        VERSION_TEXT_STYLE
    } else if roots.iter().any(WireNode::carries_link_attributes) {
        VERSION_LINK_ATTRIBUTES
    } else if roots.iter().any(WireNode::carries_null) {
        VERSION_NULL_ATTRS
    } else if slice.roots().iter().any(WireNode::carries_row_attrs) {
        VERSION_TABLE_ROW_ATTRS
    } else if slice.roots().iter().any(WireNode::carries_table) {
        VERSION_TABLE
    } else {
        VERSION
    };
    let metadata = serde_json::to_string(&WireEnvelope {
        format: FORMAT.to_owned(),
        version,
        roots,
    })
    .map_err(|_| ClipboardMetadataError::serialization())?;
    if !strict_json::validate(&metadata) {
        return Err(ClipboardMetadataError::resource_limit());
    }
    Ok(metadata)
}

/// Decodes Xiaomu metadata when it matches `plain_text` exactly.
///
/// Unknown versions, malformed/foreign metadata, unsupported canonical
/// values, invalid fragment trees, and stale metadata whose computed fallback
/// differs from the platform text all return `None`. The caller should then
/// paste the supplied plain text normally. An older envelope carrying a
/// newer feature (v4 tables, v5 row attributes, pre-v7 null attributes, or
/// pre-v8 extended link marks, pre-v9 text-style marks, or pre-v10 typed/marked atoms)
/// is also rejected. Unknown attribute variants reject the entire fragment
/// rather than silently dropping values. Historical v1-v3 envelopes remain
/// unsupported, as before the null-attribute extension. All versions are
/// subject to the resource limits documented on [`encode_metadata`].
#[must_use]
pub fn decode_metadata(plain_text: &str, metadata: &str) -> Option<ClipboardSlice> {
    // serde's map deserializer keeps the last duplicate key. Reject that
    // ambiguity before DTO parsing can overwrite an unsupported/null value.
    if !strict_json::validate(metadata) {
        return None;
    }
    let envelope: WireEnvelope = serde_json::from_str(metadata).ok()?;
    if envelope.format != FORMAT
        || !matches!(
            envelope.version,
            VERSION
                | VERSION_TABLE
                | VERSION_TABLE_ROW_ATTRS
                | VERSION_NULL_ATTRS
                | VERSION_LINK_ATTRIBUTES
                | VERSION_TEXT_STYLE
                | VERSION_TYPED_ATOMS
        )
    {
        return None;
    }
    if envelope.version < VERSION_TYPED_ATOMS
        && envelope.roots.iter().any(WireNode::carries_typed_atoms)
    {
        return None;
    }
    if envelope.version < VERSION_TEXT_STYLE
        && envelope.roots.iter().any(WireNode::carries_text_style)
    {
        return None;
    }
    if envelope.version < VERSION_LINK_ATTRIBUTES
        && envelope.roots.iter().any(WireNode::carries_link_attributes)
    {
        return None;
    }
    if envelope.version < VERSION_NULL_ATTRS && envelope.roots.iter().any(WireNode::carries_null) {
        return None;
    }
    let roots = envelope
        .roots
        .into_iter()
        .map(WireNode::into_node)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if roots.is_empty() || validate_roots(&roots).is_err() {
        return None;
    }
    if (envelope.version < VERSION_TABLE && roots.iter().any(WireNode::carries_table))
        || (envelope.version < VERSION_TABLE_ROW_ATTRS
            && roots.iter().any(WireNode::carries_row_attrs))
    {
        return None;
    }
    let slice = match &roots[..] {
        [root] if root.content().as_table().is_some() => ClipboardSlice::from_table(root.clone()),
        _ => ClipboardSlice::from_roots(roots),
    };
    (slice.plain_text() == plain_text).then_some(slice)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireEnvelope {
    format: String,
    version: u32,
    roots: Vec<WireNode>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireNode {
    kind: WireKind,
    attrs: BTreeMap<String, WireAttr>,
    content: WireContent,
}

impl WireNode {
    fn carries_typed_atoms(&self) -> bool {
        match &self.content {
            WireContent::Inline { atoms, .. } => atoms.iter().any(WireAtom::is_typed),
            WireContent::Children { children } => children.iter().any(Self::carries_typed_atoms),
            WireContent::Table { rows, .. } => rows.iter().flatten().any(Self::carries_typed_atoms),
            WireContent::Atomic => false,
        }
    }

    fn carries_text_style(&self) -> bool {
        match &self.content {
            WireContent::Inline { runs, atoms } => {
                runs.iter().any(WireRun::carries_text_style)
                    || atoms.iter().any(WireAtom::carries_text_style)
            }
            WireContent::Children { children } => children.iter().any(Self::carries_text_style),
            WireContent::Table { rows, .. } => rows.iter().flatten().any(Self::carries_text_style),
            WireContent::Atomic => false,
        }
    }

    fn carries_link_attributes(&self) -> bool {
        match &self.content {
            WireContent::Inline { runs, atoms } => {
                runs.iter().any(WireRun::carries_link_attributes)
                    || atoms.iter().any(WireAtom::carries_link_attributes)
            }
            WireContent::Children { children } => {
                children.iter().any(Self::carries_link_attributes)
            }
            WireContent::Table { rows, .. } => {
                rows.iter().flatten().any(Self::carries_link_attributes)
            }
            WireContent::Atomic => false,
        }
    }

    fn carries_null(&self) -> bool {
        if self.attrs.values().any(WireAttr::carries_null) {
            return true;
        }
        match &self.content {
            WireContent::Inline { atoms, .. } => atoms.iter().any(WireAtom::carries_null),
            WireContent::Children { children } => children.iter().any(Self::carries_null),
            WireContent::Table { rows, row_attrs } => {
                row_attrs
                    .iter()
                    .any(|attrs| attrs.values().any(WireAttr::carries_null))
                    || rows.iter().flatten().any(Self::carries_null)
            }
            WireContent::Atomic => false,
        }
    }

    fn carries_row_attrs(node: &ClipboardNode) -> bool {
        match node.content() {
            ClipboardNodeContent::Table { rows, row_attrs } => {
                row_attrs.iter().any(|attrs| !attrs.is_empty())
                    || rows.iter().flatten().any(Self::carries_row_attrs)
            }
            ClipboardNodeContent::Children(children) => {
                children.iter().any(Self::carries_row_attrs)
            }
            _ => false,
        }
    }
    /// Whether this node (or its subtree) carries a table payload, which is
    /// what bumps the envelope to v5.
    fn carries_table(node: &ClipboardNode) -> bool {
        if node.content().as_table().is_some() {
            return true;
        }
        match node.content().as_children() {
            Some(children) => children.iter().any(Self::carries_table),
            None => false,
        }
    }

    fn from_node(node: &ClipboardNode) -> Result<Self, ClipboardMetadataError> {
        let content = match node.content() {
            ClipboardNodeContent::Inline(inline) => WireContent::Inline {
                runs: inline
                    .runs()
                    .iter()
                    .map(WireRun::from_run)
                    .collect::<Result<_, _>>()?,
                atoms: inline
                    .atoms()
                    .iter()
                    .map(WireAtom::from_atom)
                    .collect::<Result<_, _>>()?,
            },
            ClipboardNodeContent::Children(children) => WireContent::Children {
                children: children
                    .iter()
                    .map(Self::from_node)
                    .collect::<Result<_, _>>()?,
            },
            ClipboardNodeContent::Table { rows, row_attrs } => WireContent::Table {
                row_attrs: if row_attrs.iter().all(NodeAttrs::is_empty) {
                    Vec::new()
                } else {
                    row_attrs
                        .iter()
                        .map(|attrs| {
                            attrs
                                .iter()
                                .map(|(key, value)| {
                                    Ok((key.to_owned(), WireAttr::from_attr(value)?))
                                })
                                .collect::<Result<_, ClipboardMetadataError>>()
                        })
                        .collect::<Result<_, _>>()?
                },
                rows: rows
                    .iter()
                    .map(|cells| {
                        cells
                            .iter()
                            .map(Self::from_node)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<_, _>>()?,
            },
            ClipboardNodeContent::Atomic => WireContent::Atomic,
        };
        Ok(Self {
            kind: WireKind::from_kind(node.kind())?,
            attrs: node
                .attrs()
                .iter()
                .map(|(key, value)| Ok((key.to_owned(), WireAttr::from_attr(value)?)))
                .collect::<Result<_, ClipboardMetadataError>>()?,
            content,
        })
    }

    fn into_node(self) -> Result<ClipboardNode, ClipboardMetadataError> {
        let attrs = self
            .attrs
            .into_iter()
            .map(|(key, value)| Ok((key, value.into_attr()?)))
            .collect::<Result<BTreeMap<_, _>, ClipboardMetadataError>>()?;
        let content = match self.content {
            WireContent::Inline { runs, atoms } => {
                let runs = runs
                    .into_iter()
                    .map(WireRun::into_run)
                    .collect::<Result<Vec<_>, _>>()?;
                // Atom anchors are validated against the exact fragment text.
                let buffer =
                    TextBuffer::from_string(runs.iter().map(|run| run.text().as_str()).collect());
                let atoms = atoms
                    .into_iter()
                    .map(|atom| atom.into_atom(&buffer))
                    .collect::<Result<Vec<_>, _>>()?;
                ClipboardNodeContent::Inline(
                    ClipboardInline::new(runs, atoms)
                        .map_err(|_| ClipboardMetadataError::invalid())?,
                )
            }
            WireContent::Children { children } => ClipboardNodeContent::Children(
                children
                    .into_iter()
                    .map(Self::into_node)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            WireContent::Table { rows, row_attrs } => ClipboardNodeContent::Table {
                row_attrs: row_attrs
                    .into_iter()
                    .map(|attrs| {
                        let attrs = attrs
                            .into_iter()
                            .map(|(key, value)| Ok((key, value.into_attr()?)))
                            .collect::<Result<_, ClipboardMetadataError>>()?;
                        NodeAttrs::new(attrs).map_err(|_| ClipboardMetadataError::invalid())
                    })
                    .collect::<Result<_, _>>()?,
                rows: rows
                    .into_iter()
                    .map(|cells| {
                        cells
                            .into_iter()
                            .map(Self::into_node)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<_, _>>()?,
            },
            WireContent::Atomic => ClipboardNodeContent::Atomic,
        };
        Ok(ClipboardNode::new(
            self.kind.into_kind()?,
            NodeAttrs::new(attrs).map_err(|_| ClipboardMetadataError::invalid())?,
            content,
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum WireContent {
    Inline {
        runs: Vec<WireRun>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        atoms: Vec<WireAtom>,
    },
    Children {
        children: Vec<WireNode>,
    },
    /// Rectangular table payload (v5): rows of cell nodes, reading order.
    Table {
        rows: Vec<Vec<WireNode>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        row_attrs: Vec<BTreeMap<String, WireAttr>>,
    },
    Atomic,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum WireKind {
    Paragraph,
    Heading(u8),
    Quote,
    BulletList,
    OrderedList,
    ListItem,
    CodeBlock,
    HorizontalRule,
    Image,
    Table,
    TableRow,
    TableCell,
    Custom(String),
}

impl WireKind {
    fn from_kind(kind: &NodeKind) -> Result<Self, ClipboardMetadataError> {
        match kind {
            NodeKind::Paragraph => Ok(Self::Paragraph),
            NodeKind::Heading(level) => Ok(Self::Heading(level.as_u8())),
            NodeKind::Quote => Ok(Self::Quote),
            NodeKind::BulletList => Ok(Self::BulletList),
            NodeKind::OrderedList => Ok(Self::OrderedList),
            NodeKind::ListItem => Ok(Self::ListItem),
            NodeKind::CodeBlock => Ok(Self::CodeBlock),
            NodeKind::HorizontalRule => Ok(Self::HorizontalRule),
            NodeKind::Image => Ok(Self::Image),
            NodeKind::Table => Ok(Self::Table),
            NodeKind::TableRow => Ok(Self::TableRow),
            NodeKind::TableCell => Ok(Self::TableCell),
            NodeKind::Custom(key) => Ok(Self::Custom(key.clone())),
            NodeKind::Document | _ => Err(ClipboardMetadataError::unsupported()),
        }
    }

    fn into_kind(self) -> Result<NodeKind, ClipboardMetadataError> {
        match self {
            Self::Paragraph => Ok(NodeKind::Paragraph),
            Self::Heading(level) => HeadingLevel::new(level)
                .map(NodeKind::Heading)
                .map_err(|_| ClipboardMetadataError::invalid()),
            Self::Quote => Ok(NodeKind::Quote),
            Self::BulletList => Ok(NodeKind::BulletList),
            Self::OrderedList => Ok(NodeKind::OrderedList),
            Self::ListItem => Ok(NodeKind::ListItem),
            Self::CodeBlock => Ok(NodeKind::CodeBlock),
            Self::HorizontalRule => Ok(NodeKind::HorizontalRule),
            Self::Image => Ok(NodeKind::Image),
            Self::Table => Ok(NodeKind::Table),
            Self::TableRow => Ok(NodeKind::TableRow),
            Self::TableCell => Ok(NodeKind::TableCell),
            Self::Custom(key) => {
                NodeKind::custom(key).map_err(|_| ClipboardMetadataError::invalid())
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum WireAttr {
    Null,
    Bool(bool),
    Integer(i64),
    String(String),
    List(Vec<WireAttr>),
    Object(BTreeMap<String, WireAttr>),
}

impl WireAttr {
    fn carries_null(&self) -> bool {
        match self {
            Self::Null => true,
            Self::List(values) => values.iter().any(Self::carries_null),
            Self::Object(values) => values.values().any(Self::carries_null),
            Self::Bool(_) | Self::Integer(_) | Self::String(_) => false,
        }
    }

    fn from_attr(value: &AttrValue) -> Result<Self, ClipboardMetadataError> {
        match value {
            AttrValue::Null => Ok(Self::Null),
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
            Self::Null => Ok(AttrValue::Null),
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
