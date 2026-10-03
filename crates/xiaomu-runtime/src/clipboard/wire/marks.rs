//! Strict mark DTOs, including conditional v8 exact link attributes.

use serde::{Deserialize, Serialize};
use xiaomu_core::document::{LinkAttributes, LinkMark, Mark, MarkSet, StringAttribute, TextRun};

use super::ClipboardMetadataError;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireRun {
    text: String,
    marks: Vec<WireMark>,
}

impl WireRun {
    pub(super) fn carries_link_attributes(&self) -> bool {
        self.marks
            .iter()
            .any(|mark| matches!(mark, WireMark::LinkAttributes { .. }))
    }

    pub(super) fn from_run(run: &TextRun) -> Result<Self, ClipboardMetadataError> {
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

    pub(super) fn into_run(self) -> Result<TextRun, ClipboardMetadataError> {
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
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireMark {
    Bold {},
    Italic {},
    Code {},
    Underline {},
    Strike {},
    // Historical form: absent and null title both mean no title. Its v4-v7
    // interpretation remains unchanged; only v8's new variant is exact.
    Link { href: String, title: Option<String> },
    LinkAttributes { attrs: WireLinkAttributes },
}

impl WireMark {
    fn from_mark(mark: &Mark) -> Result<Self, ClipboardMetadataError> {
        Ok(match mark {
            Mark::Bold => Self::Bold {},
            Mark::Italic => Self::Italic {},
            Mark::Code => Self::Code {},
            Mark::Underline => Self::Underline {},
            Mark::Strike => Self::Strike {},
            Mark::Link(link) => match link.classic_parts() {
                Some((href, title)) => Self::Link {
                    href: href.to_owned(),
                    title: title.map(str::to_owned),
                },
                None => Self::LinkAttributes {
                    attrs: WireLinkAttributes::from_attributes(link.attributes()),
                },
            },
            _ => return Err(ClipboardMetadataError::unsupported()),
        })
    }

    fn into_mark(self) -> Result<Mark, ClipboardMetadataError> {
        Ok(match self {
            Self::Bold {} => Mark::Bold,
            Self::Italic {} => Mark::Italic,
            Self::Code {} => Mark::Code,
            Self::Underline {} => Mark::Underline,
            Self::Strike {} => Mark::Strike,
            Self::Link { href, title } => Mark::Link(LinkMark::new(href, title)),
            Self::LinkAttributes { attrs } => {
                Mark::Link(LinkMark::from_attributes(attrs.into_attributes()))
            }
        })
    }
}

/// Every field is explicit on the v8 wire: Missing is a tagged value, not a
/// serde default. Omission, unknown fields and invalid types all fail closed.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireLinkAttributes {
    href: WireStringAttribute,
    target: WireStringAttribute,
    rel: WireStringAttribute,
    class: WireStringAttribute,
    title: WireStringAttribute,
}

impl WireLinkAttributes {
    fn from_attributes(attrs: &LinkAttributes) -> Self {
        Self {
            href: WireStringAttribute::from_attribute(attrs.href()),
            target: WireStringAttribute::from_attribute(attrs.target()),
            rel: WireStringAttribute::from_attribute(attrs.rel()),
            class: WireStringAttribute::from_attribute(attrs.class()),
            title: WireStringAttribute::from_attribute(attrs.title()),
        }
    }

    fn into_attributes(self) -> LinkAttributes {
        LinkAttributes::default()
            .with_href(self.href.into_attribute())
            .with_target(self.target.into_attribute())
            .with_rel(self.rel.into_attribute())
            .with_class(self.class.into_attribute())
            .with_title(self.title.into_attribute())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireStringAttribute {
    Missing {},
    Null {},
    String { value: String },
}

impl WireStringAttribute {
    fn from_attribute(attribute: &StringAttribute) -> Self {
        match attribute {
            StringAttribute::Missing => Self::Missing {},
            StringAttribute::Null => Self::Null {},
            StringAttribute::Value(value) => Self::String {
                value: value.clone(),
            },
        }
    }

    fn into_attribute(self) -> StringAttribute {
        match self {
            Self::Missing {} => StringAttribute::Missing,
            Self::Null {} => StringAttribute::Null,
            Self::String { value } => StringAttribute::Value(value),
        }
    }
}
