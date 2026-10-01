//! Bridge between native editable-text UTF-8 offsets and atom-aware layout.
//! The platform never sees renderer bytes. Composition captures its exact
//! seam ordinal so text, chips, caret and candidate bounds agree visually.
use super::ParagraphView;
use crate::inline_atom_display::InlineAtomDisplayProjection;
use crate::input::utf16;
use std::ops::Range;
use xiaomu_core::selection::{CursorAffinity, InlinePoint};

impl ParagraphView {
    /// Resolve a native text range without discarding an existing seam ordinal.
    pub(super) fn input_range_points(
        &self,
        explicit: Option<Range<usize>>,
    ) -> Option<(InlinePoint, InlinePoint)> {
        let inline = self.inline()?;
        if self.is_range_input() {
            let at = InlinePoint::at_start_of(self.node());
            return Some((at, at));
        }
        let (anchor, focus) = self.session.borrow().selection().as_same_node_inline()?;
        if anchor.node_id() != self.node() {
            return None;
        }
        let key = |p: InlinePoint| (p.text_offset(), p.atom_index());
        let (head, tail) = if key(anchor) <= key(focus) {
            (anchor, focus)
        } else {
            (focus, anchor)
        };
        let Some(range) = explicit else {
            return Some((head, tail));
        };
        let text = self.canonical_text();
        let start = inline
            .offset_at(utf16::utf8_offset(&text, range.start))
            .ok()?;
        let end = inline
            .offset_at(utf16::utf8_offset(&text, range.end))
            .ok()?;
        if start > end {
            return None;
        }
        // An echo of selected_text_range keeps the original mixed endpoints.
        if start == head.text_offset() && end == tail.text_offset() {
            return Some((head, tail));
        }
        let ordinal = if start == focus.text_offset() {
            focus.atom_index()
        } else {
            inline.atom_count_at(start)
        };
        let at = InlinePoint::new(self.node(), start, ordinal, CursorAffinity::Before);
        let end = if start == end {
            at
        } else {
            InlinePoint::new(self.node(), end, 0, CursorAffinity::Before)
        };
        Some((at, end))
    }

    pub(super) fn composition_layout_range(
        &self,
        projection: &InlineAtomDisplayProjection,
    ) -> Option<Range<usize>> {
        let state = self.composition.as_ref()?;
        let range = state.base_range();
        let inline = self.inline()?;
        let at = InlinePoint::new(
            self.node(),
            inline.offset_at(range.start).ok()?,
            state.start_atom_index(),
            CursorAffinity::Before,
        );
        let start = projection.display_offset_for_inline_point(at)?;
        let end = if range.is_empty() {
            start
        } else {
            projection.display_offset_for_inline_point(InlinePoint::new(
                self.node(),
                inline.offset_at(range.end).ok()?,
                0,
                CursorAffinity::Before,
            ))?
        };
        (start <= end).then_some(start..end)
    }

    pub(super) fn input_byte_to_layout(&self, raw: usize) -> Option<usize> {
        let Some(projection) = self
            .atom_display_projection()
            .filter(|p| !p.atoms().is_empty())
        else {
            return Some(raw);
        };
        let inline = self.inline()?;
        if let Some(state) = &self.composition {
            let base = state.base_range();
            let visual = self.composition_layout_range(&projection)?;
            let preedit_end = base.start + state.preedit().len();
            if raw >= base.start && raw <= preedit_end {
                return Some(visual.start + raw - base.start);
            }
            let canonical = if raw > preedit_end {
                raw - state.preedit().len() + base.len()
            } else {
                raw
            };
            let display = projection.display_offset_for_inline_point(InlinePoint::new(
                self.node(),
                inline.offset_at(canonical).ok()?,
                0,
                CursorAffinity::Before,
            ))?;
            return if raw > preedit_end {
                Some(display - visual.len() + state.preedit().len())
            } else {
                Some(display)
            };
        }
        let ordinal = self
            .session
            .borrow()
            .selection()
            .as_same_node_inline()
            .filter(|(_, focus)| {
                focus.node_id() == self.node() && focus.text_offset().as_usize() == raw
            })
            .map_or(0, |(_, focus)| focus.atom_index());
        projection.display_offset_for_inline_point(InlinePoint::new(
            self.node(),
            inline.offset_at(raw).ok()?,
            ordinal,
            CursorAffinity::Before,
        ))
    }

    pub(super) fn layout_byte_to_input(&self, raw: usize) -> Option<usize> {
        let Some(projection) = self
            .atom_display_projection()
            .filter(|p| !p.atoms().is_empty())
        else {
            return Some(raw);
        };
        let (original, suffix) = if let Some(state) = &self.composition {
            let visual = self.composition_layout_range(&projection)?;
            let end = visual.start + state.preedit().len();
            if raw >= visual.start && raw <= end {
                return Some(state.base_range().start + raw - visual.start);
            }
            if raw > end {
                (raw - state.preedit().len() + visual.len(), true)
            } else {
                (raw, false)
            }
        } else {
            (raw, false)
        };
        let canonical = projection
            .inline_point_for_display_hit(original, CursorAffinity::Before)?
            .text_offset()
            .as_usize();
        if suffix {
            let state = self.composition.as_ref()?;
            Some(canonical - state.base_range().len() + state.preedit().len())
        } else {
            Some(canonical)
        }
    }

    /// Chip decorations follow the same splice used for text layout.
    pub(super) fn layout_atom_ranges(&self) -> Vec<Range<usize>> {
        let Some(projection) = self.atom_display_projection() else {
            return Vec::new();
        };
        let splice = self
            .composition_layout_range(&projection)
            .zip(self.composition.as_ref());
        projection
            .atoms()
            .iter()
            .filter_map(|atom| {
                let range = atom.display_range().clone();
                let Some((base, state)) = &splice else {
                    return Some(range);
                };
                if range.end <= base.start {
                    Some(range)
                } else if range.start >= base.end {
                    Some(
                        range.start - base.len() + state.preedit().len()
                            ..range.end - base.len() + state.preedit().len(),
                    )
                } else {
                    None
                }
            })
            .collect()
    }
}
