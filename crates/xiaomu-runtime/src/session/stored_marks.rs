//! Runtime-transient marks for typing at a collapsed caret.
//!
//! Stored marks are editing state, not canonical document content. They are
//! never represented by empty text runs and never cross codec/persistence
//! boundaries.

use xiaomu_core::document::{InlineContent, Mark, MarkKind, MarkSet, NodeId};
use xiaomu_core::text::TextOffset;

use super::{DocumentPosition, DocumentSession, SessionError, SessionOutcome};

impl DocumentSession {
    /// Returns the marks default text replacement would use at this byte offset.
    ///
    /// The node must own the current inline focus and `replacement_start` must
    /// be a valid UTF-8 boundary in its content. A range replacement inherits
    /// at its start, not at the selection's focus. Explicit stored marks win,
    /// including `Some(empty)`; otherwise the left run wins at a boundary,
    /// except offset zero which uses the first run. Atom ordinals do not alter
    /// text-mark inheritance. Cell ranges and non-inline focus return an error.
    ///
    /// This is a read-only presentation query: it neither applies an intent nor
    /// invokes host policy. A policy that substitutes an edit plan may choose
    /// different marks. Frontends can use this for a composition overlay while
    /// retaining the normal guarded, atomic commit path.
    pub fn effective_input_marks(
        &self,
        node: NodeId,
        replacement_start: TextOffset,
    ) -> Result<MarkSet, SessionError> {
        if self.selection.active_cell_range().is_some()
            || !matches!(self.selection.focus(), DocumentPosition::Inline(point) if point.node_id() == node)
        {
            return Err(SessionError::SelectionInvalid);
        }
        let inline = self
            .document
            .node(node)
            .and_then(|node| node.content().as_inline())
            .ok_or(SessionError::SelectionInvalid)?;
        inline
            .validate_offset(replacement_start)
            .map_err(SessionError::Core)?;
        Ok(self
            .stored_marks
            .clone()
            .unwrap_or_else(|| inherited_marks_at(inline, replacement_start.as_usize())))
    }

    /// Returns the explicit pending marks for the collapsed caret, if any.
    ///
    /// `None` means text insertion follows Core's normal surrounding-run
    /// inheritance. `Some(empty)` is meaningful: it explicitly requests
    /// unmarked text even when the surrounding run carries marks.
    #[must_use]
    pub fn stored_marks(&self) -> Option<&MarkSet> {
        self.stored_marks.as_ref()
    }

    pub(super) fn toggle_stored_mark(
        &mut self,
        inline: &InlineContent,
        mark: &Mark,
    ) -> Result<SessionOutcome, SessionError> {
        let selection = self
            .selection
            .as_single_node()
            .ok_or(SessionError::SelectionInvalid)?;
        if !selection.is_collapsed() {
            return Err(SessionError::SelectionInvalid);
        }

        let offset = selection.focus().offset().as_usize();
        let base = self
            .stored_marks
            .clone()
            .unwrap_or_else(|| inherited_marks_at(inline, offset));
        let mut next: Vec<Mark> = base
            .as_slice()
            .iter()
            .filter(|existing| existing.kind() != mark.kind())
            .cloned()
            .collect();
        if !base.contains(mark.kind()) {
            next.push(mark.clone());
        }
        self.stored_marks = Some(MarkSet::new(next).map_err(SessionError::Core)?);
        self.history.break_group();

        // No canonical document or selection state changed. Frontends that
        // issued the command already repaint and can query `stored_marks()`.
        Ok(SessionOutcome::NoChange)
    }

    pub(super) fn set_stored_mark(
        &mut self,
        inline: &InlineContent,
        mark: &Mark,
    ) -> Result<SessionOutcome, SessionError> {
        self.replace_stored_mark(inline, mark.kind(), Some(mark))
    }

    pub(super) fn remove_stored_mark(
        &mut self,
        inline: &InlineContent,
        kind: MarkKind,
    ) -> Result<SessionOutcome, SessionError> {
        self.replace_stored_mark(inline, kind, None)
    }

    fn replace_stored_mark(
        &mut self,
        inline: &InlineContent,
        kind: MarkKind,
        replacement: Option<&Mark>,
    ) -> Result<SessionOutcome, SessionError> {
        let (_, focus) = self
            .selection
            .as_same_node_inline()
            .filter(|_| self.selection.is_collapsed())
            .ok_or(SessionError::SelectionInvalid)?;
        let base = self
            .stored_marks
            .clone()
            .unwrap_or_else(|| inherited_marks_at(inline, focus.text_offset().as_usize()));
        let current = base.as_slice().iter().find(|mark| mark.kind() == kind);
        if current == replacement {
            return Ok(SessionOutcome::NoChange);
        }
        let next = base
            .as_slice()
            .iter()
            .filter(|mark| mark.kind() != kind)
            .cloned()
            .chain(replacement.cloned());
        self.stored_marks = Some(MarkSet::new(next).map_err(SessionError::Core)?);
        self.history.break_group();
        Ok(SessionOutcome::NoChange)
    }

    pub(super) fn clear_stored_marks(&mut self) {
        self.stored_marks = None;
    }
}

/// Matches Core `ReplaceText` insertion inheritance: a boundary belongs to
/// the run on its left, except offset zero which uses the first run.
pub(super) fn inherited_marks_at(inline: &InlineContent, offset: usize) -> MarkSet {
    let mut cursor = 0usize;
    for run in inline.runs() {
        cursor += run.len_bytes();
        if offset <= cursor {
            return run.marks().clone();
        }
    }
    inline
        .runs()
        .last()
        .map(|run| run.marks().clone())
        .unwrap_or_else(MarkSet::empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xiaomu_core::document::TextRun;

    #[test]
    fn boundary_inheritance_prefers_left_run() {
        let bold = MarkSet::new([Mark::Bold]).unwrap();
        let inline = InlineContent::new([
            TextRun::new("a", bold).unwrap(),
            TextRun::new("b", MarkSet::empty()).unwrap(),
        ])
        .unwrap();

        assert!(inherited_marks_at(&inline, 0).contains(MarkKind::Bold));
        assert!(inherited_marks_at(&inline, 1).contains(MarkKind::Bold));
        assert!(!inherited_marks_at(&inline, 2).contains(MarkKind::Bold));
    }
}
