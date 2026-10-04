//! Exact inline text-style values, independent of CSS interpretation and layout.

use super::StringAttribute;

/// Three independently preserved string-valued text-style attributes.
///
/// Missing, explicit null and strings (including empty or unrecognized strings)
/// remain distinct. `Default` leaves all fields missing. Core does not parse
/// colors, resolve fonts or convert font-size units; those are frontend or host
/// capabilities. No value here promises that a renderer supports the style.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextStyleAttributes {
    color: StringAttribute,
    font_family: StringAttribute,
    font_size: StringAttribute,
}

impl TextStyleAttributes {
    /// Replaces the exact color attribute without parsing or normalization.
    #[must_use]
    pub fn with_color(mut self, color: StringAttribute) -> Self {
        self.color = color;
        self
    }

    /// Replaces the exact font-family attribute without resolving fonts.
    #[must_use]
    pub fn with_font_family(mut self, font_family: StringAttribute) -> Self {
        self.font_family = font_family;
        self
    }

    /// Replaces the exact font-size attribute without interpreting its units.
    #[must_use]
    pub fn with_font_size(mut self, font_size: StringAttribute) -> Self {
        self.font_size = font_size;
        self
    }

    /// Returns the exact color attribute, preserving missing/null distinctions.
    #[must_use]
    pub const fn color(&self) -> &StringAttribute {
        &self.color
    }

    /// Returns the exact font-family attribute, including its original spelling.
    #[must_use]
    pub const fn font_family(&self) -> &StringAttribute {
        &self.font_family
    }

    /// Returns the exact font-size attribute, including its original units.
    #[must_use]
    pub const fn font_size(&self) -> &StringAttribute {
        &self.font_size
    }
}

/// A canonical text-style mark with exact typed attributes.
///
/// A mark with all missing, null or empty fields is still a mark. Removal and
/// host-specific empty-style cleanup are explicit edits, not normalization.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextStyleMark {
    attributes: TextStyleAttributes,
}

impl TextStyleMark {
    /// Creates a mark retaining all three attributes exactly, without defaults.
    #[must_use]
    pub const fn from_attributes(attributes: TextStyleAttributes) -> Self {
        Self { attributes }
    }

    /// Returns the exact attributes without collapsing missing/null values.
    #[must_use]
    pub const fn attributes(&self) -> &TextStyleAttributes {
        &self.attributes
    }
}
