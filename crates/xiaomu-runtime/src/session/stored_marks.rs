//! Runtime-transient marks for typing at a collapsed caret.
//!
//! Stored marks are editing state, not canonical document content. They are
//! never represented by empty text runs and never cross codec/persistence
//! boundaries.

use xiaomu_core::document::{InlineContent, Mark, MarkSet};

use super::{DocumentSession, SessionError, SessionOutcome};

impl DocumentSession {
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

    pub(super) fn clear_stored_marks(&mut self) {
        self.stored_marks = None;
    }
}

/// Matches Core `ReplaceText` insertion inheritance: a boundary belongs to
/// the run on its left, except offset zero which uses the first run.
fn inherited_marks_at(inline: &InlineContent, offset: usize) -> MarkSet {
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
    use xiaomu_core::document::{MarkKind, TextRun};

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
