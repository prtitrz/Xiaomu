//! Canonical marked text runs.

use crate::{Error, Result, text::TextBuffer};

use super::MarkSet;

/// Non-empty canonical text carrying one normalized `MarkSet`.
///
/// Run boundaries are not document coordinates. Later inline-content
/// normalization may split or merge runs without changing user-visible text
/// positions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextRun {
    text: TextBuffer,
    marks: MarkSet,
}

impl TextRun {
    /// Creates a non-empty canonical text run.
    pub fn new(text: impl Into<TextBuffer>, marks: MarkSet) -> Result<Self> {
        let text = text.into();
        if text.is_empty() {
            return Err(Error::EmptyTextRun);
        }

        Ok(Self { text, marks })
    }

    /// Returns the run text.
    #[must_use]
    pub const fn text(&self) -> &TextBuffer {
        &self.text
    }

    /// Returns the normalized mark set.
    #[must_use]
    pub const fn marks(&self) -> &MarkSet {
        &self.marks
    }

    /// Returns the run length in UTF-8 bytes.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }
}
