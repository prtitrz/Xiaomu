//! Explicitly width-constrained spanning-table layout.
//!
//! `DocumentView` can explicitly opt into this path per instance; default views
//! retain their legacy placeholder and input guards. The opt-in connection uses
//! current-frame viewport width, full-cell hit registration and exact per-table
//! measurement admission shared with retained native input handlers. This does
//! not claim browser intrinsic-width parity or native GUI verification.
//! Runtime measurement failures render a visible, noninteractive placeholder,
//! never a zero-sized invisible table. The owning host must still refuse edits
//! through unavailable descendants; presentation failure cannot grant admission.
//!
//! Stock GPUI 0.2.2 only exposes equal-fraction `grid_cols`, not arbitrary grid
//! tracks. Its public `AnyElement::layout_as_root` and `prepaint_at` are enough
//! to measure and position genuine child trees if the width is already known.
//! They cannot be called inside `request_measured_layout`'s callback: GPUI takes
//! the layout engine out of `Window` while invoking that callback. Consequently
//! this prototype requires an explicit resolved width, never guesses from the
//! viewport, and never repositions children only during paint.
//!
//! Width hints are read across all logical columns. Missing/null/zero hints are
//! automatic; conflicting positive hints are refused without modifying attrs.
//! Fixed tracks retain their exact widths; automatic tracks share the remaining
//! space subject to the caller's minimum. An all-fixed table stays at its sum,
//! and a too-narrow container produces overflow, not silently shrunken hints.
//! This is an explicit native policy, not a claim of browser CSS table parity.
//! Header identity is retained for host styling. Heights come from the actual
//! child layouts; rowspan deficits are distributed over covered rows.
//!
//! # Same-frame host connection
//!
//! A fixed-viewport adapter can request the editor viewport's ordinary full
//! width/height, then, in **prepaint**, build the document subtree using those
//! same-frame viewport bounds. At that point `layout_as_root` is legal: the
//! outer layout computation has finished. The adapter lays out the complete
//! scroll container as a separate root with definite viewport width/height,
//! prepaints at the viewport origin, then paints it unchanged. Carry resolved
//! content width down `render_block_tree`, subtracting actual host insets and
//! borders; pass a spanning cell's resolved width minus its own padding to any
//! nested table. The scroll container still measures the complete document's
//! height, so sibling flow/scroll range do not depend on last-frame heights.
//! This avoids both a cached-width/notify loop and measuring children inside a
//! measured-layout callback. It requires a bounded viewport, as the current
//! `DocumentView::render`'s `size_full()` scroller already does. Intrinsically
//! sized outer hosts need a different explicit measurement contract.
//! The opt-in `DocumentView` connection follows this bounded-viewport route.
//!
//! Cell padding belongs to the real child subtree (for example a padded div),
//! so child layout, hit tests and native input bounds all include it naturally.
//! Extra rowspan height extends only the full-cell rectangle. Content remains
//! top-aligned at its natural measured height; it is never vertically scaled
//! or painted with a different offset. A host must separately register the
//! full-cell rectangle to handle blank-space selection.
//!
//! Chuanyun's `editor-blocks.css` uses `table-layout: auto; width: 100%`, 9px/12px
//! cell padding, 13px text at 1.55 line height and top alignment. The schema and
//! command oracles do not establish intrinsic/min-content track sizing. That
//! browser/native comparison is still needed before claiming visual parity;
//! equal sharing here only specifies automatic tracks in this prototype.

mod element;
mod geometry;

pub use element::{
    SpanningTableElement, TableCellElement, TableElementLayoutState, TableElementPrepaintState,
};
pub use geometry::{
    TableCellGeometry, TableCellPlan, TableGeometry, TableLayoutError, TableLayoutOptions,
    TableLayoutPlan,
};
