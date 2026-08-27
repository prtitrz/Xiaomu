//! Caret placement and in-block caret movement.
//!
//! Split out of `mod.rs` so P3 visual-line / history growth does not keep
//! stacking onto the session orchestration file.

use xiaomu_core::selection::TextPoint;
use xiaomu_core::text::TextOffset;

use super::intent::{self, CaretMove};
use super::{DocumentSelection, DocumentSession, SessionError, SessionOutcome};

impl DocumentSession {
    pub(super) fn move_caret(
        &mut self,
        caret_move: CaretMove,
        extend_selection: bool,
    ) -> Result<SessionOutcome, SessionError> {
        // Cross-block caret movement arrives with the multi-block frontend;
        // at a gap endpoint there is nothing to move within yet.
        let Some(focus) = self.text_focus().ok() else {
            return Ok(SessionOutcome::NoChange);
        };
        let inline = self.inline_of(focus.node_id())?;
        let text = intent::concatenated(&inline);
        let current = focus.offset().as_usize();
        let target = match caret_move {
            CaretMove::Backward => intent::previous_boundary(&text, current),
            CaretMove::Forward => intent::next_boundary(&text, current),
            CaretMove::ToStart => (current != 0).then_some(0),
            CaretMove::ToEnd => (current != text.len()).then_some(text.len()),
        };

        let Some(raw) = target else {
            return Ok(SessionOutcome::NoChange);
        };
        let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
        let moved = TextPoint::new(focus.node_id(), offset, focus.affinity());
        let next = if extend_selection {
            DocumentSelection::new(self.selection.anchor(), moved)
        } else {
            DocumentSelection::collapsed(moved)
        };

        if next == self.selection {
            return Ok(SessionOutcome::NoChange);
        }
        self.selection = next;
        self.notify_selection_changed();

        Ok(SessionOutcome::SelectionChanged)
    }

    pub(super) fn place_caret(
        &mut self,
        offset: TextOffset,
        extend_selection: bool,
    ) -> Result<SessionOutcome, SessionError> {
        // Hit-testing against the block tree lands on one block's text; gap
        // endpoints have no in-node coordinates to place into.
        let Some(focus) = self.text_focus().ok() else {
            return Ok(SessionOutcome::NoChange);
        };
        let inline = self.inline_of(focus.node_id())?;
        inline.validate_offset(offset).map_err(SessionError::Core)?;

        let moved = TextPoint::new(focus.node_id(), offset, focus.affinity());
        let next = if extend_selection {
            DocumentSelection::new(self.selection.anchor(), moved)
        } else {
            DocumentSelection::collapsed(moved)
        };

        if next == self.selection {
            return Ok(SessionOutcome::NoChange);
        }
        self.selection = next;
        self.notify_selection_changed();

        Ok(SessionOutcome::SelectionChanged)
    }

    /// Places both selection endpoints at absolute text positions.
    ///
    /// The document-level escape hatch for cross-block navigation and mouse
    /// selection. Both endpoints must be valid for the current snapshot;
    /// otherwise the session is untouched.
    pub(super) fn set_selection(
        &mut self,
        anchor: TextPoint,
        focus: TextPoint,
    ) -> Result<SessionOutcome, SessionError> {
        let next = DocumentSelection::new(anchor, focus);
        next.validate(&self.document)
            .map_err(|_| SessionError::SelectionInvalid)?;

        if next == self.selection {
            return Ok(SessionOutcome::NoChange);
        }
        self.selection = next;
        self.notify_selection_changed();

        Ok(SessionOutcome::SelectionChanged)
    }
}
