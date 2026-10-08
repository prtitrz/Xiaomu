//! Literal, non-overlapping, source-preserving Unicode search.

use std::ops::Range;

use xiaomu_core::{document::DocumentRevision, selection::InlinePoint};

use super::position::point_key;
use super::{
    BoundarySide, ReadingBudget, ReadingError, ReadingProjection, ReadingSpanKind, ReadingTextBlock,
};

/// Literal search comparison, without normalization, regex or word modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadingCase {
    /// Compare Unicode scalars exactly (case-sensitive).
    #[default]
    Exact,
    /// Default non-Turkic Unicode 17.0.0 simple case-fold equivalence.
    ///
    /// One scalar always remains one scalar, though its UTF-8 width may differ.
    /// This includes s/long-s, k/Kelvin, sigma/final-sigma and sharp-s/capital
    /// sharp-s, but neither dotted/dotless-I variants nor sharp-s/"ss". The
    /// complete pinned official table is independent of the Rust toolchain.
    UnicodeSimple,
}

impl ReadingCase {
    fn key(self, ch: char) -> char {
        match self {
            Self::Exact => ch,
            Self::UnicodeSimple => super::case_fold::fold(ch),
        }
    }
}

/// Allocation limits for one complete search.
///
/// The query bound also bounds three preprocessing/ring buffers: one char and
/// two usize slots per query scalar. No per-document folded string or boundary
/// array is allocated. Matches are bounded separately and never truncated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadingSearchLimits {
    /// Maximum literal query UTF-8 bytes (default 4 KiB).
    pub max_query_bytes: usize,
    /// Maximum returned matches over all blocks (default 100,000).
    pub max_matches: usize,
}

impl Default for ReadingSearchLimits {
    fn default() -> Self {
        Self {
            max_query_bytes: 4096,
            max_matches: 100_000,
        }
    }
}

/// One source-mapped match, produced only by a complete projection search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingMatch {
    revision: DocumentRevision,
    block_order: usize,
    projected: Range<usize>,
    start: InlinePoint,
    end: InlinePoint,
    contains_atoms: bool,
}

impl ReadingMatch {
    /// Source document revision; the owner/session must be checked separately.
    #[must_use]
    pub const fn revision(&self) -> DocumentRevision {
        self.revision
    }
    /// Index of its textblock in [`ReadingProjection::blocks`].
    #[must_use]
    pub const fn block_order(&self) -> usize {
        self.block_order
    }
    /// UTF-8 byte range in that textblock's selected projection.
    #[must_use]
    pub fn projected_range(&self) -> Range<usize> {
        self.projected.clone()
    }
    /// Real source start, after omitted atoms at the match's leading boundary.
    #[must_use]
    pub const fn start(&self) -> InlinePoint {
        self.start
    }
    /// Real source end, before omitted atoms at the match's trailing boundary.
    #[must_use]
    pub const fn end(&self) -> InlinePoint {
        self.end
    }
    /// Whether the source range contains any atom, including omitted atoms.
    /// This is evidence for a future edit planner, not permission to replace.
    #[must_use]
    pub const fn contains_atoms(&self) -> bool {
        self.contains_atoms
    }
}

/// Complete ordered matches for one immutable projection revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingSearchResults {
    revision: DocumentRevision,
    matches: Vec<ReadingMatch>,
}

impl ReadingSearchResults {
    /// Source revision of the whole result, including an empty result.
    #[must_use]
    pub const fn revision(&self) -> DocumentRevision {
        self.revision
    }
    /// All matches in document order. Search never returns partial counts.
    #[must_use]
    pub fn matches(&self) -> &[ReadingMatch] {
        &self.matches
    }
}

impl ReadingProjection {
    /// Finds a literal independently in each block, across formatting runs.
    ///
    /// Empty queries yield no matches. Matches are non-overlapping and source
    /// text is never transformed: offsets refer to the original UTF-8 string.
    /// Explicit atom characters may match and remain typed in the result.
    /// Streaming KMP costs linear scalar comparisons plus bounded table lookup
    /// and mapping/output work, without a regex automaton or backtracking.
    pub fn find_literal(
        &self,
        query: &str,
        case: ReadingCase,
        limits: ReadingSearchLimits,
    ) -> Result<ReadingSearchResults, ReadingError> {
        if query.len() > limits.max_query_bytes {
            return Err(ReadingError::BudgetExceeded(ReadingBudget::QueryBytes));
        }
        let mut result = ReadingSearchResults {
            revision: self.revision(),
            matches: Vec::new(),
        };
        if query.is_empty() {
            return Ok(result);
        }
        let count = query.chars().count();
        let mut pattern = Vec::new();
        pattern
            .try_reserve_exact(count)
            .map_err(|_| ReadingError::AllocationFailed)?;
        pattern.extend(query.chars().map(|ch| case.key(ch)));
        let mut failure = zeroes(count)?;
        let mut boundaries = zeroes(count)?;
        for index in 1..count {
            let mut matched = failure[index - 1];
            while matched > 0 && pattern[index] != pattern[matched] {
                matched = failure[matched - 1];
            }
            if pattern[index] == pattern[matched] {
                matched += 1;
            }
            failure[index] = matched;
        }
        for block in self.blocks() {
            let mut matched = 0;
            for (index, (offset, ch)) in block.text().char_indices().enumerate() {
                boundaries[index % count] = offset;
                let key = case.key(ch);
                while matched > 0 && key != pattern[matched] {
                    matched = failure[matched - 1];
                }
                if key == pattern[matched] {
                    matched += 1;
                }
                if matched != count {
                    continue;
                }
                if result.matches.len() == limits.max_matches {
                    return Err(ReadingError::BudgetExceeded(ReadingBudget::Matches));
                }
                if result.matches.len() == result.matches.capacity() {
                    result
                        .matches
                        .try_reserve_exact((limits.max_matches - result.matches.len()).min(256))
                        .map_err(|_| ReadingError::AllocationFailed)?;
                }
                let start = boundaries[(index + 1 - count) % count];
                result.matches.push(source_match(
                    block,
                    self.revision(),
                    start..offset + ch.len_utf8(),
                )?);
                matched = 0; // The next match starts after this one, never overlaps.
            }
        }
        Ok(result)
    }
}

fn zeroes(count: usize) -> Result<Vec<usize>, ReadingError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| ReadingError::AllocationFailed)?;
    values.resize(count, 0);
    Ok(values)
}

fn source_match(
    block: &ReadingTextBlock,
    revision: DocumentRevision,
    projected: Range<usize>,
) -> Result<ReadingMatch, ReadingError> {
    let start = block.point_at(projected.start, BoundarySide::AfterAtoms)?;
    let end = block.point_at(projected.end, BoundarySide::BeforeAtoms)?;
    let first = block
        .spans()
        .partition_point(|span| span.projected_range().end < projected.start);
    let contains_atoms = block.spans()[first..]
        .iter()
        .take_while(|span| span.projected_range().start <= projected.end)
        .any(|span| {
            matches!(span.kind(), ReadingSpanKind::InlineAtom { .. })
                && point_key(span.start()) >= point_key(start)
                && point_key(span.end()) <= point_key(end)
        });
    Ok(ReadingMatch {
        revision,
        block_order: block.order(),
        projected,
        start,
        end,
        contains_atoms,
    })
}
