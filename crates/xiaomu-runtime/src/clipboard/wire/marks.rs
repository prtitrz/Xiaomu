//! Strict mark DTOs with conditional exact link and text-style attributes.

use serde::{Deserialize, Serialize};
use xiaomu_core::document::{
    LinkAttributes, LinkMark, Mark, MarkSet, StringAttribute, TextRun, TextStyleAttributes,
    TextStyleMark,
};

use super::ClipboardMetadataError;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireRun {
    text: String,
    marks: Vec<WireMark>,
}

impl WireRun {
    pub(super) fn carries_text_style(&self) -> bool {
        self.marks
            .iter()
            .any(|mark| matches!(mark, WireMark::TextStyle { .. }))
    }

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
    TextStyle { attrs: WireTextStyleAttributes },
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
            Mark::TextStyle(style) => Self::TextStyle {
                attrs: WireTextStyleAttributes::from_attributes(style.attributes()),
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
            Self::TextStyle { attrs } => {
                Mark::TextStyle(TextStyleMark::from_attributes(attrs.into_attributes()))
            }
        })
    }
}

/// All three exact slots are required, even if their values are Missing.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTextStyleAttributes {
    color: WireStringAttribute,
    font_family: WireStringAttribute,
    font_size: WireStringAttribute,
}

impl WireTextStyleAttributes {
    fn from_attributes(attrs: &TextStyleAttributes) -> Self {
        Self {
            color: WireStringAttribute::from_attribute(attrs.color()),
            font_family: WireStringAttribute::from_attribute(attrs.font_family()),
            font_size: WireStringAttribute::from_attribute(attrs.font_size()),
        }
    }

    fn into_attributes(self) -> TextStyleAttributes {
        TextStyleAttributes::default()
            .with_color(self.color.into_attribute())
            .with_font_family(self.font_family.into_attribute())
            .with_font_size(self.font_size.into_attribute())
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
