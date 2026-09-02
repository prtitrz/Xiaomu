//! Caret placement and in-block caret movement.
//!
//! Split out of `mod.rs` so P3 visual-line / history growth does not keep
//! stacking onto the session orchestration file.
//!
//! Movement is one caret unit per step (P4.3): an inline atom is an
//! indivisible single unit at its anchor boundary (ADR 0005), so stepping
//! walks the same-boundary atom ordinal first and text scalars second.

use xiaomu_core::selection::{InlinePoint, TextPoint};
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
        let Some(focus) = self.inline_focus().ok() else {
            return Ok(SessionOutcome::NoChange);
        };
        let node = focus.node_id();
        let inline = self.inline_of(node)?;
        let text = intent::concatenated(&inline);
        let current = focus.text_offset().as_usize();
        let ordinal = focus.atom_index();
        let affinity = focus.affinity();

        // Arriving at a boundary from the left puts the caret before every
        // atom anchored there; arriving from the right puts it after them.
        let boundary_point = |raw: usize, from_right: bool| -> Result<InlinePoint, SessionError> {
            let offset = inline.offset_at(raw).map_err(SessionError::Core)?;
            let ordinal = if from_right {
                inline.atom_count_at(offset)
            } else {
                0
            };
            Ok(InlinePoint::new(node, offset, ordinal, affinity))
        };

        let target: Option<InlinePoint> = match caret_move {
            CaretMove::Backward => {
                if ordinal > 0 {
                    Some(InlinePoint::new(
                        node,
                        focus.text_offset(),
                        ordinal - 1,
                        affinity,
                    ))
                } else {
                    intent::previous_boundary(&text, current)
                        .map(|raw| boundary_point(raw, true))
                        .transpose()?
                }
            }
            CaretMove::Forward => {
                if ordinal < inline.atom_count_at(focus.text_offset()) {
                    Some(InlinePoint::new(
                        node,
                        focus.text_offset(),
                        ordinal + 1,
                        affinity,
                    ))
                } else {
                    intent::next_boundary(&text, current)
                        .map(|raw| boundary_point(raw, false))
                        .transpose()?
                }
            }
            CaretMove::ToStart => {
                (current != 0).then_some(InlinePoint::new(node, TextOffset::ZERO, 0, affinity))
            }
            CaretMove::ToEnd => {
                if current == text.len() {
                    None
                } else {
                    let end = inline.offset_at(text.len()).map_err(SessionError::Core)?;
                    Some(InlinePoint::new(
                        node,
                        end,
                        inline.atom_count_at(end),
                        affinity,
                    ))
                }
            }
        };

        let Some(moved) = target else {
            return Ok(SessionOutcome::NoChange);
        };
        let next = if extend_selection {
            DocumentSelection::new(self.selection.anchor(), moved)
        } else {
            DocumentSelection::collapsed(moved)
        };

        self.install_selection(next)
    }

    pub(super) fn place_caret(
        &mut self,
        offset: TextOffset,
        extend_selection: bool,
    ) -> Result<SessionOutcome, SessionError> {
        // Hit-testing against the block tree lands on one block's text; gap
        // endpoints have no in-node coordinates to place into. Atom-precise
        // hit resolution arrives with the atom renderer; a placed caret
        // resolves to the gap before any atoms anchored at the boundary.
        let Some(focus) = self.inline_focus().ok() else {
            return Ok(SessionOutcome::NoChange);
        };
        let inline = self.inline_of(focus.node_id())?;
        inline.validate_offset(offset).map_err(SessionError::Core)?;

        let moved = InlinePoint::new(focus.node_id(), offset, 0, focus.affinity());
        let next = if extend_selection {
            DocumentSelection::new(self.selection.anchor(), moved)
        } else {
            DocumentSelection::collapsed(moved)
        };

        self.install_selection(next)
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
        self.install_selection(next)
    }

    fn install_selection(
        &mut self,
        next: DocumentSelection,
    ) -> Result<SessionOutcome, SessionError> {
        if next == self.selection {
            return Ok(SessionOutcome::NoChange);
        }
        self.selection = next;
        self.clear_stored_marks();
        self.history.break_group();
        self.notify_selection_changed();
        Ok(SessionOutcome::SelectionChanged)
    }
}
