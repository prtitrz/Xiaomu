//! Typed link attributes, independent of URI policy and serialization formats.

use super::StringAttribute;

/// The five canonical string-valued attributes of a hyperlink.
///
/// Each field independently preserves missing, explicit null and string
/// values. `Default` leaves all five absent. Fields do not receive implicit
/// browser or host defaults, and Core never opens or interprets the link.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LinkAttributes {
    href: StringAttribute,
    target: StringAttribute,
    rel: StringAttribute,
    class: StringAttribute,
    title: StringAttribute,
}

impl LinkAttributes {
    /// Replaces the exact destination attribute, including missing/null.
    #[must_use]
    pub fn with_href(mut self, href: StringAttribute) -> Self {
        self.href = href;
        self
    }

    /// Replaces the exact target attribute without inferring a default.
    #[must_use]
    pub fn with_target(mut self, target: StringAttribute) -> Self {
        self.target = target;
        self
    }

    /// Replaces the exact relationship attribute without token normalization.
    #[must_use]
    pub fn with_rel(mut self, rel: StringAttribute) -> Self {
        self.rel = rel;
        self
    }

    /// Replaces the exact class attribute without token normalization.
    #[must_use]
    pub fn with_class(mut self, class: StringAttribute) -> Self {
        self.class = class;
        self
    }

    /// Replaces the exact title attribute, including an empty string.
    #[must_use]
    pub fn with_title(mut self, title: StringAttribute) -> Self {
        self.title = title;
        self
    }

    /// Returns the exact destination attribute.
    #[must_use]
    pub const fn href(&self) -> &StringAttribute {
        &self.href
    }

    /// Returns the exact target attribute.
    #[must_use]
    pub const fn target(&self) -> &StringAttribute {
        &self.target
    }

    /// Returns the exact relationship attribute.
    #[must_use]
    pub const fn rel(&self) -> &StringAttribute {
        &self.rel
    }

    /// Returns the exact class attribute.
    #[must_use]
    pub const fn class(&self) -> &StringAttribute {
        &self.class
    }

    /// Returns the exact title attribute.
    #[must_use]
    pub const fn title(&self) -> &StringAttribute {
        &self.title
    }
}

/// A hyperlink mark with exact, typed canonical attributes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinkMark {
    attributes: LinkAttributes,
}

impl LinkMark {
    /// Creates a classic href/title link, preserving the old convenience form.
    ///
    /// `href` is a string value; `None` title means missing, not null. The
    /// target, rel and class attributes are missing. URI interpretation
    /// belongs to hosts/codecs; Core preserves strings without network policy.
    #[must_use]
    pub fn new(href: impl Into<String>, title: Option<String>) -> Self {
        Self::from_attributes(
            LinkAttributes::default()
                .with_href(StringAttribute::Value(href.into()))
                .with_title(title.map_or(StringAttribute::Missing, StringAttribute::Value)),
        )
    }

    /// Creates a link retaining all five fields exactly, with no defaults.
    #[must_use]
    pub const fn from_attributes(attributes: LinkAttributes) -> Self {
        Self { attributes }
    }

    /// Returns all typed attributes without collapsing missing/null values.
    #[must_use]
    pub const fn attributes(&self) -> &LinkAttributes {
        &self.attributes
    }

    /// Returns the destination only when it is a string, including empty text.
    ///
    /// Missing and null return `None`; inspect `attributes().href()` when the
    /// distinction matters. No synthetic empty destination is substituted.
    #[must_use]
    pub fn href(&self) -> Option<&str> {
        self.attributes.href.as_str()
    }

    /// Returns the title only when it is a string, including empty text.
    ///
    /// Inspect `attributes().title()` to distinguish a missing title from null.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.attributes.title.as_str()
    }

    /// Returns href/title only if the classic two-field form is lossless.
    ///
    /// Requires a string href, missing target/rel/class and missing or string
    /// title. Any explicit null or other present attribute returns `None`,
    /// allowing older codecs to refuse rather than silently discard data.
    #[must_use]
    pub fn classic_parts(&self) -> Option<(&str, Option<&str>)> {
        if !self.attributes.target.is_missing()
            || !self.attributes.rel.is_missing()
            || !self.attributes.class.is_missing()
            || self.attributes.title.is_null()
        {
            return None;
        }
        Some((self.href()?, self.title()))
    }
}
