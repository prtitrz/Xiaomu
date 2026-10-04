//! Exact absence, null and string values for typed canonical attributes.

/// A typed string attribute retaining the distinction between missing and null.
///
/// Empty strings remain `Value("")`; they are not absence or null. Core
/// preserves strings verbatim and leaves interpretation to hosts and codecs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum StringAttribute {
    /// The attribute is absent.
    #[default]
    Missing,
    /// The attribute is present with an explicit null value.
    Null,
    /// The attribute is present with this exact string, including empty text.
    Value(String),
}

impl StringAttribute {
    /// Returns only a string value; inspect the enum to distinguish missing/null.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Value(value) => Some(value),
            Self::Missing | Self::Null => None,
        }
    }

    /// Returns whether the attribute is absent rather than explicitly null.
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// Returns whether the attribute is explicitly null rather than absent.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl From<String> for StringAttribute {
    fn from(value: String) -> Self {
        Self::Value(value)
    }
}

impl From<&str> for StringAttribute {
    fn from(value: &str) -> Self {
        Self::Value(value.to_owned())
    }
}
