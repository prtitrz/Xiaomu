//! Revision-bound view-only reading decorations and passive navigation.
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{Context, Window};
use xiaomu_core::{document::DocumentRevision, selection::InlinePoint};
use xiaomu_runtime::session::{DocumentSession, DocumentSessionIdentity};

use super::DocumentView;

/// An opaque capability for this exact view, session instance and revision.
///
/// Capture before deriving reading ranges. A second view over the same session,
/// a session replaced in place, or any edit invalidates the capability. Hosts
/// must additionally discard asynchronous results from superseded query/pane
/// lifetimes; the engine deliberately knows nothing about those host concepts.
#[derive(Clone, Debug)]
pub struct ReadingViewSnapshot {
    identity: Rc<()>,
    session: DocumentSessionIdentity,
    revision: DocumentRevision,
}

impl ReadingViewSnapshot {
    /// The canonical revision whose positions this capability can present.
    #[must_use]
    pub const fn revision(&self) -> DocumentRevision {
        self.revision
    }

    pub(crate) fn matches(&self, state: &ReadingState, session: &DocumentSession) -> bool {
        Rc::ptr_eq(&self.identity, &state.identity)
            && self.session == session.identity()
            && self.revision == session.document().revision()
    }
}

/// An ordered half-open range within one inline block, including atom gaps.
///
/// Coordinates are canonical UTF-8 text offsets plus same-boundary atom
/// ordinals, never renderer bytes. Construction is unchecked; every view API
/// validates both endpoints against its current document before accepting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadingRange {
    start: InlinePoint,
    end: InlinePoint,
}

impl ReadingRange {
    /// Creates an unchecked range, validated by the receiving view API.
    #[must_use]
    pub const fn new(start: InlinePoint, end: InlinePoint) -> Self {
        Self { start, end }
    }
    /// Returns the inclusive start position.
    #[must_use]
    pub const fn start(self) -> InlinePoint {
        self.start
    }
    /// Returns the exclusive end position.
    #[must_use]
    pub const fn end(self) -> InlinePoint {
        self.end
    }
    pub(crate) fn valid(self, document: &xiaomu_core::document::XiaomuDocument) -> bool {
        self.start.node_id() == self.end.node_id()
            && self.start.validate(document).is_ok()
            && self.end.validate(document).is_ok()
            && (self.start.text_offset(), self.start.atom_index())
                <= (self.end.text_offset(), self.end.atom_index())
    }
}

/// A refused reading operation never changes canonical or visual state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingViewError {
    /// The view/session/revision capability no longer matches.
    StaleSnapshot,
    /// A range is reversed, crosses blocks or has an invalid endpoint.
    InvalidRange,
    /// The active index does not identify a supplied range.
    InvalidActiveIndex,
    /// The target cannot be built by this view's presentation capabilities.
    Unavailable,
}

impl std::fmt::Display for ReadingViewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::StaleSnapshot => {
                "reading snapshot no longer belongs to this view/session/revision"
            }
            Self::InvalidRange => "reading range has invalid, reversed or cross-block endpoints",
            Self::InvalidActiveIndex => "active reading highlight index is out of bounds",
            Self::Unavailable => "reading target is unavailable in this view",
        })
    }
}
impl std::error::Error for ReadingViewError {}

/// State of the most recent accepted passive reveal request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadingRevealStatus {
    /// Waiting for current measured geometry or the next coherent scroll frame.
    Deferred,
    /// The measured target's leading portion has been revealed.
    Revealed,
    /// Current presentation/measurement cannot reach the target.
    Unavailable,
    /// A newer request, cancellation, session or document superseded it.
    Stale,
}

pub(crate) type SharedReadingState = Rc<RefCell<ReadingState>>;

#[derive(Default)]
pub(crate) struct ReadingState {
    identity: Rc<()>,
    pub highlights: HashMap<xiaomu_core::document::NodeId, Vec<(ReadingRange, bool)>>,
    pub snapshot: Option<ReadingViewSnapshot>,
    pub(super) pending: Option<PendingReveal>,
    pub(super) generation: u64,
    pub(super) frame: u64,
    pub(super) scheduled: bool,
    pub(super) status: Option<ReadingRevealStatus>,
    pub(super) tables: HashMap<xiaomu_core::document::NodeId, gpui::ScrollHandle>,
    pub(super) geometry: Option<super::reading_geometry::ReadingGeometry>,
}

#[derive(Clone)]
pub(super) struct PendingReveal {
    pub snapshot: ReadingViewSnapshot,
    pub range: ReadingRange,
    pub generation: u64,
}

impl ReadingState {
    pub(crate) fn snapshot(&self, session: &DocumentSession) -> ReadingViewSnapshot {
        ReadingViewSnapshot {
            identity: self.identity.clone(),
            session: session.identity(),
            revision: session.document().revision(),
        }
    }
    pub(super) fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.pending.take().is_some() {
            self.status = Some(ReadingRevealStatus::Stale);
        }
    }
}

impl DocumentView {
    /// Captures a capability without changing document, selection or history.
    #[must_use]
    pub fn reading_snapshot(&self) -> ReadingViewSnapshot {
        self.reading.borrow().snapshot(&self.session.borrow())
    }

    /// Installs non-document highlights atomically, preserving all editing state.
    ///
    /// Empty/collapsed ranges are legal but paint no area. `active` indexes the
    /// supplied list. A successful installation cancels an older pending reveal;
    /// install decorations before requesting Next/Previous navigation. This does
    /// not reshape text, alter marks, interrupt typing groups or take focus.
    pub fn set_reading_highlights(
        &mut self,
        snapshot: &ReadingViewSnapshot,
        ranges: &[ReadingRange],
        active: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Result<(), ReadingViewError> {
        self.validate_reading_snapshot(snapshot)?;
        if active.is_some_and(|index| index >= ranges.len()) {
            return Err(ReadingViewError::InvalidActiveIndex);
        }
        let session = self.session.borrow();
        if ranges.iter().any(|range| !range.valid(session.document())) {
            return Err(ReadingViewError::InvalidRange);
        }
        let mut highlights = HashMap::<_, Vec<_>>::new();
        for (index, range) in ranges.iter().enumerate() {
            highlights
                .entry(range.start.node_id())
                .or_default()
                .push((*range, active == Some(index)));
        }
        drop(session);
        let mut state = self.reading.borrow_mut();
        state.cancel();
        state.snapshot = Some(snapshot.clone());
        state.highlights = highlights;
        cx.notify();
        Ok(())
    }

    /// Removes view-only highlights and cancels any pending reading reveal.
    pub fn clear_reading_highlights(&mut self, cx: &mut Context<Self>) {
        let mut state = self.reading.borrow_mut();
        state.cancel();
        state.snapshot = None;
        state.highlights.clear();
        cx.notify();
    }

    /// Reveals a canonical point without moving selection or native focus.
    /// Unmeasured requests wait for this view's next paint, with stale guards.
    pub fn reveal_position(
        &mut self,
        snapshot: &ReadingViewSnapshot,
        point: InlinePoint,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ReadingRevealStatus, ReadingViewError> {
        self.reveal_range(snapshot, ReadingRange::new(point, point), window, cx)
    }

    /// Reveals a range's leading measured rectangle, keeping external focus.
    ///
    /// The full first visual-row portion is revealed when it fits. Oversized
    /// targets reveal their leading viewport-sized portion. All enclosing table
    /// horizontal viewports and the document viewport participate. No caret or
    /// document selection is temporarily moved to simulate this operation.
    pub fn reveal_range(
        &mut self,
        snapshot: &ReadingViewSnapshot,
        range: ReadingRange,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ReadingRevealStatus, ReadingViewError> {
        self.validate_reading_snapshot(snapshot)?;
        {
            let session = self.session.borrow();
            if !range.valid(session.document()) {
                return Err(ReadingViewError::InvalidRange);
            }
            if self
                .table_capability
                .borrow()
                .hidden_ancestor_for_build(session.document(), range.start.node_id())
                .is_some()
            {
                return Err(ReadingViewError::Unavailable);
            }
        }
        self.cancel_reading_caret_scroll(cx);
        let mut state = self.reading.borrow_mut();
        state.cancel();
        state.pending = Some(PendingReveal {
            snapshot: snapshot.clone(),
            range,
            generation: state.generation,
        });
        state.status = Some(ReadingRevealStatus::Deferred);
        cx.notify();
        Ok(ReadingRevealStatus::Deferred)
    }

    /// Reports completion/refusal of the latest accepted reveal, if any.
    #[must_use]
    pub fn reading_reveal_status(&self) -> Option<ReadingRevealStatus> {
        let state = self.reading.borrow();
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.snapshot.matches(&state, &self.session.borrow()))
        {
            Some(ReadingRevealStatus::Stale)
        } else {
            state.status
        }
    }

    /// Restores the existing canonical selection's native focus without scroll.
    /// This also cancels queued caret scrolling and any pending reading reveal.
    pub fn focus_selection_without_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reading.borrow_mut().cancel();
        self.sync_children(cx);
        self.cancel_reading_caret_scroll(cx);
        self.route_focus(window, cx);
        cx.notify();
    }

    pub(super) fn validate_reading_snapshot(
        &self,
        snapshot: &ReadingViewSnapshot,
    ) -> Result<(), ReadingViewError> {
        snapshot
            .matches(&self.reading.borrow(), &self.session.borrow())
            .then_some(())
            .ok_or(ReadingViewError::StaleSnapshot)
    }

    fn cancel_reading_caret_scroll(&self, cx: &gpui::App) {
        for (_, child) in self.children.iter().chain(self.range_input.iter()) {
            child.read(cx).cancel_caret_scroll();
        }
    }

    pub(super) fn begin_reading_frame(&self) {
        let mut state = self.reading.borrow_mut();
        state.geometry = None;
        state.frame = state.frame.wrapping_add(1);
        state.tables.clear();
        let session = self.session.borrow();
        if state
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.matches(&state, &session))
        {
            state.highlights.clear();
            state.snapshot = None;
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.snapshot.matches(&state, &session))
        {
            state.cancel();
        }
    }
}
