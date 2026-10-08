//! Exact scalar-boundary mapping, including zero-width omitted atom events.

use std::cmp::Ordering;

use xiaomu_core::selection::{CursorAffinity, InlinePoint};

use super::{BoundarySide, ReadingError, ReadingProjection, ReadingSpanKind, ReadingTextBlock};

pub(super) fn point_key(point: InlinePoint) -> (usize, usize) {
    (point.text_offset().as_usize(), point.atom_index())
}

impl ReadingTextBlock {
    /// Maps a projected UTF-8 boundary back to a real source point.
    ///
    /// Invalid byte/surrogate interiors are rejected, never clamped. At omitted
    /// atoms `side` selects before/after the whole zero-width group. Affinity is
    /// Before for `BeforeAtoms` and After for `AfterAtoms`; it does not change
    /// the source coordinate. An empty block's sole boundary is its start.
    pub fn point_at(&self, offset: usize, side: BoundarySide) -> Result<InlinePoint, ReadingError> {
        if !self.text.is_char_boundary(offset) {
            return Err(ReadingError::InvalidProjectedBoundary);
        }
        let affinity = match side {
            BoundarySide::BeforeAtoms => CursorAffinity::Before,
            BoundarySide::AfterAtoms => CursorAffinity::After,
        };
        let first = self
            .spans
            .partition_point(|span| span.projected.end < offset);
        let last = self
            .spans
            .partition_point(|span| span.projected.start <= offset);
        let span = match side {
            BoundarySide::BeforeAtoms => self.spans.get(first).filter(|_| first < last),
            BoundarySide::AfterAtoms => last.checked_sub(1).and_then(|index| self.spans.get(index)),
        };
        let Some(span) = span else {
            return Ok(InlinePoint::at_start_of(self.node_id).with_affinity(affinity));
        };
        let point = if span.projected.is_empty() {
            match side {
                BoundarySide::BeforeAtoms => span.start,
                BoundarySide::AfterAtoms => span.end,
            }
        } else if offset == span.projected.start {
            span.start
        } else if offset == span.projected.end {
            span.end
        } else if matches!(span.kind, ReadingSpanKind::Text) {
            InlinePoint::new(
                self.node_id,
                self.canonical
                    .offset_at(span.start.text_offset().as_usize() + offset - span.projected.start)
                    .map_err(|_| ReadingError::InvalidProjectedBoundary)?,
                0,
                affinity,
            )
        } else {
            return Err(ReadingError::InvalidProjectedBoundary);
        };
        Ok(point.with_affinity(affinity))
    }

    /// Maps an exact source boundary into this projection.
    ///
    /// All valid gaps between omitted atoms map to the same projected offset.
    /// Unknown nodes, invalid UTF-8 boundaries and invalid ordinals fail closed.
    pub fn projected_offset(&self, point: InlinePoint) -> Result<usize, ReadingError> {
        if point.node_id() != self.node_id {
            return Err(ReadingError::InvalidPoint);
        }
        if self.spans.is_empty() {
            return (point_key(point) == (0, 0))
                .then_some(0)
                .ok_or(ReadingError::InvalidPoint);
        }
        let key = point_key(point);
        let index = self.spans.partition_point(|span| point_key(span.end) < key);
        let span = self.spans.get(index).ok_or(ReadingError::InvalidPoint)?;
        if key == point_key(span.start) {
            return Ok(span.projected.start);
        }
        if key == point_key(span.end) {
            return Ok(span.projected.end);
        }
        if !matches!(span.kind, ReadingSpanKind::Text)
            || point.atom_index() != 0
            || key <= point_key(span.start)
            || key >= point_key(span.end)
        {
            return Err(ReadingError::InvalidPoint);
        }
        let offset = span.projected.start + key.0 - span.start.text_offset().as_usize();
        self.text
            .is_char_boundary(offset)
            .then_some(offset)
            .ok_or(ReadingError::InvalidPoint)
    }

    /// Original text-only fragments before a validated source point.
    /// Does not allocate, emit atoms, add block separators or trim whitespace.
    pub fn text_fragments_before(
        &self,
        point: InlinePoint,
    ) -> Result<impl Iterator<Item = &str>, ReadingError> {
        let end = self.projected_offset(point)?;
        Ok(self
            .spans
            .iter()
            .filter(move |span| {
                matches!(span.kind, ReadingSpanKind::Text) && span.projected.start < end
            })
            .map(move |span| &self.text[span.projected.start..span.projected.end.min(end)]))
    }
}

impl ReadingProjection {
    /// Orders validated source points in canonical document order.
    /// Affinity has no effect. Session identity must be checked by the host.
    pub fn compare_points(
        &self,
        left: InlinePoint,
        right: InlinePoint,
    ) -> Result<Ordering, ReadingError> {
        let left_block = self
            .block(left.node_id())
            .ok_or(ReadingError::InvalidPoint)?;
        let right_block = self
            .block(right.node_id())
            .ok_or(ReadingError::InvalidPoint)?;
        left_block.projected_offset(left)?;
        right_block.projected_offset(right)?;
        Ok((left_block.order, point_key(left)).cmp(&(right_block.order, point_key(right))))
    }
}
