//! Disjoint legacy and exact atom DTOs: string labels never imply built-ins.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use xiaomu_core::document::{AtomKind, InlineAtomContent, MarkSet, NodeAttrs};
use xiaomu_core::text::TextBuffer;

use super::super::fragment::ClipboardAtom;
use super::marks::WireMark;
use super::{ClipboardMetadataError, WireAttr};

/// `kind` is a string in the historical DTO and a tagged object in v10.
/// Both shapes reject unknown fields. In particular a legacy kind plus a
/// `marks` field cannot be parsed as an unmarked legacy atom.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum WireAtom {
    Legacy(LegacyAtom),
    Typed(TypedAtom),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LegacyAtom {
    anchor: usize,
    kind: String,
    attrs: BTreeMap<String, WireAttr>,
    fallback: String,
}

/// Every field is required, including an empty `marks` array. Explicit null
/// never means missing, and a missing array never means an unmarked atom.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TypedAtom {
    anchor: usize,
    kind: WireAtomKind,
    attrs: BTreeMap<String, WireAttr>,
    fallback: String,
    marks: Vec<WireMark>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireAtomKind {
    HardBreak {},
    Extension { value: String },
}

impl WireAtom {
    pub(super) fn is_typed(&self) -> bool {
        matches!(self, Self::Typed(_))
    }

    pub(super) fn carries_text_style(&self) -> bool {
        matches!(self, Self::Typed(atom) if atom.marks.iter().any(|mark| {
            matches!(mark, WireMark::TextStyle { .. })
        }))
    }

    pub(super) fn carries_link_attributes(&self) -> bool {
        matches!(self, Self::Typed(atom) if atom.marks.iter().any(|mark| {
            matches!(mark, WireMark::LinkAttributes { .. })
        }))
    }

    pub(super) fn carries_null(&self) -> bool {
        let attrs = match self {
            Self::Legacy(atom) => &atom.attrs,
            Self::Typed(atom) => &atom.attrs,
        };
        attrs.values().any(WireAttr::carries_null)
    }

    pub(super) fn from_atom(atom: &ClipboardAtom) -> Result<Self, ClipboardMetadataError> {
        let attrs = atom
            .attrs()
            .iter()
            .map(|(key, value)| Ok((key.to_owned(), WireAttr::from_attr(value)?)))
            .collect::<Result<BTreeMap<_, _>, ClipboardMetadataError>>()?;
        let anchor = atom.anchor().as_usize();
        let fallback = atom.content().fallback_text().to_owned();
        if atom.kind().is_hard_break() || !atom.content().marks().is_empty() {
            let kind = if atom.kind().is_hard_break() {
                WireAtomKind::HardBreak {}
            } else {
                WireAtomKind::Extension {
                    value: atom.kind().as_str().to_owned(),
                }
            };
            let marks = atom
                .content()
                .marks()
                .as_slice()
                .iter()
                .map(WireMark::from_mark)
                .collect::<Result<_, _>>()?;
            Ok(Self::Typed(TypedAtom {
                anchor,
                kind,
                attrs,
                fallback,
                marks,
            }))
        } else {
            Ok(Self::Legacy(LegacyAtom {
                anchor,
                kind: atom.kind().as_str().to_owned(),
                attrs,
                fallback,
            }))
        }
    }

    pub(super) fn into_atom(
        self,
        buffer: &TextBuffer,
    ) -> Result<ClipboardAtom, ClipboardMetadataError> {
        let (anchor, kind, attrs, fallback, marks) = match self {
            Self::Legacy(atom) => (
                atom.anchor,
                AtomKind::new(atom.kind).map_err(|_| ClipboardMetadataError::invalid())?,
                atom.attrs,
                atom.fallback,
                MarkSet::empty(),
            ),
            Self::Typed(atom) => {
                let kind = match atom.kind {
                    WireAtomKind::HardBreak {} => AtomKind::hard_break(),
                    WireAtomKind::Extension { value } => {
                        AtomKind::new(value).map_err(|_| ClipboardMetadataError::invalid())?
                    }
                };
                let marks = atom
                    .marks
                    .into_iter()
                    .map(WireMark::into_mark)
                    .collect::<Result<Vec<_>, _>>()?;
                (
                    atom.anchor,
                    kind,
                    atom.attrs,
                    atom.fallback,
                    MarkSet::new(marks).map_err(|_| ClipboardMetadataError::invalid())?,
                )
            }
        };
        let attrs = attrs
            .into_iter()
            .map(|(key, value)| Ok((key, value.into_attr()?)))
            .collect::<Result<BTreeMap<_, _>, ClipboardMetadataError>>()?;
        let anchor = buffer
            .offset_at(anchor)
            .map_err(|_| ClipboardMetadataError::invalid())?;
        Ok(ClipboardAtom::new(
            anchor,
            kind,
            NodeAttrs::new(attrs).map_err(|_| ClipboardMetadataError::invalid())?,
            InlineAtomContent::new(fallback)
                .map_err(|_| ClipboardMetadataError::invalid())?
                .with_marks(marks),
        ))
    }
}
