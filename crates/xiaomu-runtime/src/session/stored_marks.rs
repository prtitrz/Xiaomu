//! Runtime-transient marks for typing at a collapsed caret.
//!
//! Stored marks are editing state, not canonical document content. They are
//! never represented by empty text runs and never cross codec/persistence
//! boundaries.

use xiaomu_core::document::{Mark, MarkKind, MarkSet, NodeId, NodeKind, XiaomuDocument};
use xiaomu_core::selection::InlinePoint;
use xiaomu_core::text::{TextOffset, TextRange};

use super::{DocumentPosition, DocumentSession, SessionError, SessionOutcome};

impl DocumentSession {
    /// Returns default input marks at a byte-addressed replacement start.
    ///
    /// The current collapsed caret supplies its exact atom ordinal when its
    /// offset matches. At a selected range's ordered start, range inheritance
    /// applies; other offsets use the gap after their same-boundary atoms,
    /// matching a byte-addressed composition start. Explicit stored marks,
    /// including an empty set, always win. Use [`Self::effective_input_marks_at`]
    /// when a caller already has an exact mixed-inline caret.
    pub fn effective_input_marks(
        &self,
        node: NodeId,
        replacement_start: TextOffset,
    ) -> Result<MarkSet, SessionError> {
        let focus = self.input_focus(node)?;
        let inline = self
            .document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        inline
            .validate_offset(replacement_start)
            .map_err(SessionError::Core)?;
        if let Some((anchor, focus)) = self.selection.as_same_node_inline()
            && !self.selection.is_collapsed()
        {
            let (start, end) = if (anchor.text_offset(), anchor.atom_index())
                <= (focus.text_offset(), focus.atom_index())
            {
                (anchor, focus)
            } else {
                (focus, anchor)
            };
            if start.text_offset() == replacement_start {
                return self.effective_input_marks_for_range(start, end);
            }
        }
        let ordinal = if self.selection.is_collapsed() && focus.text_offset() == replacement_start {
            focus.atom_index()
        } else {
            inline.atom_count_at(replacement_start)
        };
        self.effective_input_marks_at(InlinePoint::new(
            node,
            replacement_start,
            ordinal,
            focus.affinity(),
        ))
    }

    /// Returns default insertion marks at an exact mixed-inline caret gap.
    ///
    /// The point must belong to the current inline focus node and be valid.
    /// Explicit stored marks, including `Some(empty)`, take precedence over
    /// [`xiaomu_core::document::XiaomuDocument::inherited_inline_marks`].
    /// This is a read-only query; it does not invoke host policy or alter state.
    pub fn effective_input_marks_at(&self, at: InlinePoint) -> Result<MarkSet, SessionError> {
        self.input_focus(at.node_id())?;
        at.validate(&self.document).map_err(SessionError::Core)?;
        match &self.stored_marks {
            Some(marks) => Ok(marks.clone()),
            None => self
                .document
                .inherited_inline_marks(at)
                .map_err(SessionError::Core),
        }
    }

    /// Returns default text-input marks for ordered gaps of one inline node.
    ///
    /// A collapsed range uses caret inheritance. A nonempty range in a node
    /// with typed breaks or marked extensions uses the child after its start
    /// gap. Text-only nodes and legacy unmarked-extension-only nodes retain
    /// left-run inheritance. Explicit stored marks always take precedence.
    /// This models typing replacement, not clipboard-specific host policy.
    pub fn effective_input_marks_for_range(
        &self,
        start: InlinePoint,
        end: InlinePoint,
    ) -> Result<MarkSet, SessionError> {
        self.input_focus(start.node_id())?;
        let range_marks = self
            .document
            .inherited_inline_range_marks(start, end)
            .map_err(SessionError::Core)?;
        if let Some(marks) = &self.stored_marks {
            return Ok(marks.clone());
        }
        if (start.text_offset(), start.atom_index()) == (end.text_offset(), end.atom_index())
            || !has_mark_contributing_atoms(&self.document, start.node_id())
        {
            return self
                .document
                .inherited_inline_marks(start)
                .map_err(SessionError::Core);
        }
        Ok(range_marks.unwrap_or_else(MarkSet::empty))
    }

    /// Returns the marks a default composition commit will use for `range`.
    ///
    /// This resolves the same exact start gap as `CommitComposition`, so a
    /// byte-addressed replacement after a hard break agrees with its preview.
    /// Both byte endpoints are validated; atom-containing spans may still be
    /// rejected at commit because composition cannot delete inline atoms.
    pub fn effective_composition_marks(
        &self,
        node: NodeId,
        range: TextRange,
    ) -> Result<MarkSet, SessionError> {
        let focus = self.input_focus(node)?;
        let inline = self
            .document
            .node(node)
            .unwrap()
            .content()
            .as_inline()
            .unwrap();
        inline
            .validate_offset(range.start())
            .map_err(SessionError::Core)?;
        inline
            .validate_offset(range.end())
            .map_err(SessionError::Core)?;
        let start = super::atom_edit::composition_start(inline, focus, range);
        let end = if range.is_empty() {
            start
        } else {
            InlinePoint::new(node, range.end(), 0, start.affinity())
        };
        self.effective_input_marks_for_range(start, end)
    }

    fn input_focus(&self, node: NodeId) -> Result<InlinePoint, SessionError> {
        match self.selection.focus() {
            DocumentPosition::Inline(point)
                if self.selection.active_cell_range().is_none() && point.node_id() == node =>
            {
                Ok(point)
            }
            _ => Err(SessionError::SelectionInvalid),
        }
    }

    /// Returns the explicit pending marks for the collapsed caret, if any.
    ///
    /// `None` means text insertion follows Core's exact mixed-inline gap
    /// inheritance. `Some(empty)` is meaningful: it explicitly requests
    /// unmarked text even when the surrounding run carries marks.
    #[must_use]
    pub fn stored_marks(&self) -> Option<&MarkSet> {
        self.stored_marks.as_ref()
    }

    pub(super) fn toggle_stored_mark(
        &mut self,
        mark: &Mark,
    ) -> Result<SessionOutcome, SessionError> {
        let (_, focus) = self
            .selection
            .as_same_node_inline()
            .filter(|_| self.selection.is_collapsed())
            .ok_or(SessionError::SelectionInvalid)?;

        let base = self.effective_input_marks_at(focus)?;
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

    pub(super) fn set_stored_mark(&mut self, mark: &Mark) -> Result<SessionOutcome, SessionError> {
        self.replace_stored_mark(mark.kind(), Some(mark))
    }

    pub(super) fn remove_stored_mark(
        &mut self,
        kind: MarkKind,
    ) -> Result<SessionOutcome, SessionError> {
        self.replace_stored_mark(kind, None)
    }

    fn replace_stored_mark(
        &mut self,
        kind: MarkKind,
        replacement: Option<&Mark>,
    ) -> Result<SessionOutcome, SessionError> {
        let (_, focus) = self
            .selection
            .as_same_node_inline()
            .filter(|_| self.selection.is_collapsed())
            .ok_or(SessionError::SelectionInvalid)?;
        let base = self.effective_input_marks_at(focus)?;
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

/// Legacy unmarked extensions are transparent to inheritance. Their mere
/// presence must not switch otherwise text-only range replacement semantics.
pub(super) fn has_mark_contributing_atoms(document: &XiaomuDocument, node: NodeId) -> bool {
    document
        .node(node)
        .and_then(|node| node.content().as_inline())
        .is_some_and(|inline| {
            inline.atoms().iter().any(|placement| {
                document.node(placement.atom()).is_some_and(|atom| {
                    matches!(atom.kind(), NodeKind::InlineAtom(kind) if kind.is_hard_break())
                        || atom
                            .content()
                            .as_inline_atom()
                            .is_some_and(|content| !content.marks().is_empty())
                })
            })
        })
}
