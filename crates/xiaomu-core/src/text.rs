//! Unicode-safe text coordinates, ranges, buffers, and normalization helpers.
//!
//! Core text coordinates are typed UTF-8 byte offsets. Platform UTF-16
//! conversion belongs outside `xiaomu-core`.
//!
//! `TextOffset` deliberately has no public raw-`usize` constructor. Callers
//! obtain offsets from a `TextBuffer`, which validates UTF-8 scalar boundaries.
//! A previously valid offset must still be revalidated when used with another
//! buffer or a later revision because edits can invalidate old coordinates.
//!
//! The long-term coordinate decision (UTF-8 scalar boundaries in Core,
//! UTF-16 only in platform adapters) is fixed by `docs/adr/0001` in the
//! repository.

use core::fmt;

use crate::{Error, Result};

/// A validated UTF-8 byte offset into a text buffer.
///
/// This type identifies a Unicode scalar boundary, not a grapheme-cluster
/// boundary. Cursor movement by grapheme is a higher-level editing concern.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextOffset(usize);

impl TextOffset {
    /// The start of any text buffer.
    pub const ZERO: Self = Self(0);

    /// Returns the underlying UTF-8 byte index.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0
    }

    pub(crate) const fn from_validated_byte_index(byte_index: usize) -> Self {
        Self(byte_index)
    }
}

impl fmt::Debug for TextOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextOffset").field(&self.0).finish()
    }
}

/// A half-open text range `[start, end)` expressed in UTF-8 byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextRange {
    start: TextOffset,
    end: TextOffset,
}

impl TextRange {
    /// Creates an ordered range.
    ///
    /// Boundary and bounds validation still belongs to the target
    /// `TextBuffer`, because offsets can outlive the buffer revision that
    /// originally produced them.
    pub fn new(start: TextOffset, end: TextOffset) -> Result<Self> {
        if start > end {
            return Err(Error::InvalidTextRange {
                start: start.as_usize(),
                end: end.as_usize(),
            });
        }

        Ok(Self { start, end })
    }

    /// Creates an empty range at `offset`.
    #[must_use]
    pub const fn empty(offset: TextOffset) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Returns the inclusive start coordinate.
    #[must_use]
    pub const fn start(self) -> TextOffset {
        self.start
    }

    /// Returns the exclusive end coordinate.
    #[must_use]
    pub const fn end(self) -> TextOffset {
        self.end
    }

    /// Returns the range length in UTF-8 bytes.
    #[must_use]
    pub const fn len_bytes(self) -> usize {
        self.end.0 - self.start.0
    }

    /// Returns whether this range contains no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }
}

/// Owned text storage behind Xiaomu's Core text boundary.
///
/// P0 intentionally starts with `String`. The public semantics do not expose
/// that storage choice, allowing a future rope implementation to preserve the
/// same coordinate and validation contracts.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct TextBuffer {
    text: String,
}

impl TextBuffer {
    /// Creates an empty buffer.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    /// Creates a buffer from owned UTF-8 text.
    #[must_use]
    pub fn from_string(text: String) -> Self {
        Self { text }
    }

    /// Borrows the full buffer as UTF-8 text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the buffer length in UTF-8 bytes.
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.text.len()
    }

    /// Returns whether the buffer contains no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns a validated coordinate for `byte_index`.
    pub fn offset_at(&self, byte_index: usize) -> Result<TextOffset> {
        self.validate_byte_index(byte_index)?;
        Ok(TextOffset::from_validated_byte_index(byte_index))
    }

    /// Returns the coordinate at the end of the current buffer.
    #[must_use]
    pub fn end_offset(&self) -> TextOffset {
        TextOffset::from_validated_byte_index(self.text.len())
    }

    /// Creates and validates a half-open range in the current buffer.
    pub fn range(&self, start: TextOffset, end: TextOffset) -> Result<TextRange> {
        let range = TextRange::new(start, end)?;
        self.validate_range(range)?;
        Ok(range)
    }

    /// Validates that an existing coordinate is usable in this buffer.
    pub fn validate_offset(&self, offset: TextOffset) -> Result<()> {
        self.validate_byte_index(offset.as_usize())
    }

    /// Validates that an existing range is ordered, in bounds, and on UTF-8
    /// scalar boundaries in this buffer.
    pub fn validate_range(&self, range: TextRange) -> Result<()> {
        if range.start > range.end {
            return Err(Error::InvalidTextRange {
                start: range.start.as_usize(),
                end: range.end.as_usize(),
            });
        }

        self.validate_offset(range.start)?;
        self.validate_offset(range.end)
    }

    /// Borrows a validated slice from the buffer.
    pub fn slice(&self, range: TextRange) -> Result<&str> {
        self.validate_range(range)?;
        self.text
            .get(range.start.as_usize()..range.end.as_usize())
            .ok_or(Error::InvalidTextRange {
                start: range.start.as_usize(),
                end: range.end.as_usize(),
            })
    }

    /// Returns a new buffer with `range` replaced by `replacement`.
    ///
    /// The original buffer is left unchanged, matching the immutable snapshot
    /// direction of the document model that will consume `TextBuffer`.
    pub fn replaced(&self, range: TextRange, replacement: &str) -> Result<Self> {
        self.validate_range(range)?;

        let mut next = self.text.clone();
        next.replace_range(range.start.as_usize()..range.end.as_usize(), replacement);
        Ok(Self { text: next })
    }

    fn validate_byte_index(&self, byte_index: usize) -> Result<()> {
        if byte_index > self.text.len() {
            return Err(Error::TextOutOfBounds {
                offset: byte_index,
                len: self.text.len(),
            });
        }

        if !self.text.is_char_boundary(byte_index) {
            return Err(Error::InvalidTextBoundary { offset: byte_index });
        }

        Ok(())
    }
}

impl fmt::Debug for TextBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextBuffer").field(&self.text).finish()
    }
}

impl From<String> for TextBuffer {
    fn from(text: String) -> Self {
        Self::from_string(text)
    }
}

impl From<&str> for TextBuffer {
    fn from(text: &str) -> Self {
        Self::from_string(text.to_owned())
    }
}
