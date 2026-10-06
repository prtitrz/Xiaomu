//! Opt-in measured column dragging. The host owns authorization and persistence.
//!
//! Resize previews change only this view's layout. They never edit a session,
//! advance its revision, or create history. A valid release calls the host once
//! after discarding the preview; the host may plan and apply one transaction.
//! Native automatic sizing remains independent of the explicit drag minimum.

use std::rc::Rc;

use gpui::{Context, Window};
use xiaomu_core::document::{DocumentRevision, NodeId};
use xiaomu_runtime::session::DocumentSession;

use crate::document_view::DocumentView;

/// Pointer policy in logical pixels. Merely constructing this does not opt in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableColumnResizeConfig {
    /// Maximum distance on either side of a visible cell edge, positive/finite.
    pub handle_width: f32,
    /// Smallest explicit dragged track, in integer logical pixels (1..=1,000,000).
    /// This is independent of the native automatic-column minimum (40).
    pub min_column_width: u32,
    /// Whether the final logical column's right edge can be dragged.
    pub last_column_resizable: bool,
}

impl TableColumnResizeConfig {
    pub(crate) fn valid(self) -> bool {
        self.handle_width.is_finite()
            && self.handle_width > 0.0
            && self.handle_width <= 1_000_000.0
            && (1..=1_000_000).contains(&self.min_column_width)
    }
}

/// A measured logical-column request, not a canonical edit or storage grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableColumnResizeIntent {
    /// The innermost measured table hit by the initial pointer event.
    pub table: NodeId,
    /// Zero-based logical column immediately before the dragged edge.
    pub column: usize,
    /// Measured width at pointer-down, before any transient preview.
    pub initial_width: u32,
    /// Requested integer width after the minimum and native pointer projection.
    pub width: u32,
    /// Canonical revision captured at pointer-down; must still be current.
    pub revision: DocumentRevision,
}

type ResizeGuard = dyn Fn(&DocumentSession, &TableColumnResizeIntent) -> bool;
type ResizeCommit =
    dyn Fn(TableColumnResizeIntent, &mut DocumentView, &mut Window, &mut Context<DocumentView>);

/// Host callbacks for one view's measured resize capability.
///
/// The guard must be read-only and verify the host's current editor/note owner,
/// session identity and editability. Carry a host lifecycle/lease generation:
/// replacing a session in place with identical snapshot data is not observable
/// through document revision or node IDs alone. It runs at pointer-down, before each preview,
/// on rendering and immediately before release. Returning false cancels without
/// calling `commit`. Do not retain a successful guard as lasting authorization.
///
/// A release matching an already-measured preview commits synchronously. An
/// unmeasured final width must pass a real child-layout frame first; an intervening
/// edit or owner change cancels it. Its callback runs after that frame, with no session borrow
/// held and no preview remaining. It must revalidate host policy, build its own
/// exact attribute transaction against `intent.revision`, and apply it through
/// [`DocumentView::apply_edit_transaction`]. That method does not run typed-intent
/// preflight. Errors/refusals must be handled by the host; Xiaomu never retries.
/// Neither callback should panic or reenter the view.
#[derive(Clone)]
pub struct TableColumnResize {
    pub(crate) config: TableColumnResizeConfig,
    pub(crate) guard: Rc<ResizeGuard>,
    pub(crate) commit: Rc<ResizeCommit>,
}

impl TableColumnResize {
    /// Creates a capability; install it with `set_table_column_resize`.
    ///
    /// Invalid configuration is inert; fractional measured starting widths are
    /// unsupported. Finite `f32` logical pointer coordinates are widened to `f64`
    /// before computing the delta from the original down. Add that delta to the
    /// integral initial width, clamp to the configured minimum, then round the
    /// positive target to nearest integer (exact halves upward). Nonfinite
    /// coordinates and unrounded targets above 1,000,000 cancel. This explicit
    /// native pointer policy does not round canonical attributes or extend the
    /// document model; a browser may preserve fractional resulting widths.
    /// Release without movement is delivered too; the host decides whether its
    /// canonical width hint needs changing. Mouse-leave alone does not cancel.
    pub fn new(
        config: TableColumnResizeConfig,
        guard: impl Fn(&DocumentSession, &TableColumnResizeIntent) -> bool + 'static,
        commit: impl Fn(
            TableColumnResizeIntent,
            &mut DocumentView,
            &mut Window,
            &mut Context<DocumentView>,
        ) + 'static,
    ) -> Self {
        Self {
            config,
            guard: Rc::new(guard),
            commit: Rc::new(commit),
        }
    }
}
