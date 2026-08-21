//! Canonical inline marks and normalized mark sets.

use crate::{Error, Result};

/// Semantic identity of a mark independent of its attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkKind {
    /// Bold emphasis.
    Bold,
    /// Italic emphasis.
    Italic,
    /// Inline code.
    Code,
    /// Underline.
    Underline,
    /// Strikethrough.
    Strike,
    /// Hyperlink.
    Link,
}

/// Attributes carried by a link mark.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinkMark {
    href: String,
    title: Option<String>,
}

impl LinkMark {
    /// Creates a link mark. URI interpretation belongs to hosts/codecs; Core
    /// preserves the string without applying network or product policy.
    #[must_use]
    pub fn new(href: impl Into<String>, title: Option<String>) -> Self {
        Self {
            href: href.into(),
            title,
        }
    }

    /// Returns the preserved link target.
    #[must_use]
    pub fn href(&self) -> &str {
        &self.href
    }

    /// Returns the optional preserved title.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
}

/// Canonical inline formatting mark.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Mark {
    /// Bold emphasis.
    Bold,
    /// Italic emphasis.
    Italic,
    /// Inline code.
    Code,
    /// Underline.
    Underline,
    /// Strikethrough.
    Strike,
    /// Hyperlink with preserved attributes.
    Link(LinkMark),
}

impl Mark {
    /// Returns the semantic mark identity used for canonical ordering and
    /// duplicate detection.
    #[must_use]
    pub const fn kind(&self) -> MarkKind {
        match self {
            Self::Bold => MarkKind::Bold,
            Self::Italic => MarkKind::Italic,
            Self::Code => MarkKind::Code,
            Self::Underline => MarkKind::Underline,
            Self::Strike => MarkKind::Strike,
            Self::Link(_) => MarkKind::Link,
        }
    }
}

/// Canonically ordered, duplicate-free set of inline marks.
///
/// Identical duplicate marks are normalized away. Two marks with the same
/// `MarkKind` but conflicting attributes are rejected because a text run must
/// not carry two competing values for the same semantic mark.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MarkSet {
    marks: Vec<Mark>,
}

impl MarkSet {
    /// Creates a normalized mark set.
    pub fn new(marks: impl IntoIterator<Item = Mark>) -> Result<Self> {
        let mut marks: Vec<_> = marks.into_iter().collect();
        marks.sort_by_key(Mark::kind);

        let mut normalized = Vec::with_capacity(marks.len());
        for mark in marks {
            if let Some(previous) = normalized.last() {
                if previous.kind() == mark.kind() {
                    if previous == &mark {
                        continue;
                    }
                    return Err(Error::InvalidMarkSet);
                }
            }
            normalized.push(mark);
        }

        Ok(Self { marks: normalized })
    }

    /// Returns an empty mark set.
    #[must_use]
    pub const fn empty() -> Self {
        Self { marks: Vec::new() }
    }

    /// Returns the number of semantic marks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Returns whether no marks are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// Returns marks in canonical order.
    #[must_use]
    pub fn as_slice(&self) -> &[Mark] {
        &self.marks
    }

    /// Returns whether the set contains a mark kind.
    #[must_use]
    pub fn contains(&self, kind: MarkKind) -> bool {
        self.marks.iter().any(|mark| mark.kind() == kind)
    }
}
