# Passive reading presentation

`xiaomu_gpui::document_view::DocumentView` exposes host-neutral reading
presentation independently of canonical editing and native focus ownership.
It does not implement a product find bar, outline hierarchy or replacement.

## Authority and decoration lifetime

Capture `reading_snapshot()` before deriving ranges. The opaque
`ReadingViewSnapshot` binds the exact view, process-local session instance and
canonical revision. A second view sharing that session cannot use the stamp;
replacing the session in the same `Rc<RefCell<_>>` also invalidates it, even
when document IDs and revision are identical. Hosts additionally verify their
own pane lifetime and asynchronous query generation before calling the view.

`ReadingRange` is an ordered half-open pair of canonical `InlinePoint`s within
one block. Every receiving API validates UTF-8 boundaries, atom ordinals, node
identity and ordering against the stamped document. `set_reading_highlights`
validates the complete list and optional active index before replacing the
view-only layer. It never issues a transaction or changes marks, selection,
pending input marks, typing grouping, history or change notifications.

The paint path maps positions through the existing
`InlineAtomDisplayProjection` and uses `BlockTextLayout::selection_rects`.
HardBreak and other same-offset atoms therefore retain their exact gaps;
renderer label lengths never become canonical bytes. Decorations do not enter
text runs, shaping fingerprints, layout epochs, serialization or save state.
Visible-row culling precedes rectangle generation, with binary row/atom lookup
and a lazily shared measured glyph-position index. Result counts and the
installed range list remain complete.
A composing block suppresses canonical decorations until its normal preedit
ends; reading code never commits or cancels composition.

Installing a new decoration list or calling `clear_reading_highlights` cancels
older pending reading navigation. Install decorations before requesting the
new active match's reveal; do not re-install an unchanged list every host poll.

## Geometry and passive navigation

`reveal_position` and `reveal_range` retain external find/outline focus and the
canonical selection. They return `Deferred` while a request waits for a
complete measured/painted frame. `reading_reveal_status` reports completion,
unavailable presentation or a stale request. A target that cannot be built by
current table presentation is refused immediately. No node-count, font-size
or assumed-line-height approximation is used.

The range's leading real visual rectangle is revealed, including its complete
first-row span when it fits, or the leading viewport-sized portion when it
does not. Every enclosing measured table's horizontal viewport participates,
from the innermost outward; then the document viewport supplies the necessary
vertical (and any document-horizontal) adjustment. Column widths are not
changed. Visible targets do not move the viewport.

Offsets are published together after the complete paint/effect cycle, so the
next rendered frame sees a coherent scroll tree, after validation of the view,
session, revision, request generation, measured frame, viewport size/scale and
all participating scroll handles' observed offsets/bounds/maxima. Geometry
changes require fresh measurement; user scroll supersedes a queued reveal.
Newer requests, query clearing and revision/session replacement cannot replay
old offsets. Requests containing an active preedit wait without intervening in
native composition. A fully painted unsupported text/table layout finishes as
`Unavailable`; it is not falsely reported as revealed.

`reading_start` uses the current ordered selection start if its measured caret
intersects the document viewport and every nested table clip. Otherwise it
hit-tests the leading visible text surface at the actual viewport top. It
returns `None` when no fresh accessible text geometry is available. Wrapped,
aligned, variable-size rows and renderer atom spans use existing layout and
hit-test projection; this query does not move the caret. Pass the current
`Window` and `App`; a scroll/viewport-size/scale change before repaint returns
`None` rather than reusing old painted geometry. The visible-caret probe also
uses the host's real caret-height policy at the ordered endpoint.

`focus_selection_without_scroll` restores native focus to the existing
canonical selection and cancels queued reading/caret scroll requests. It does
not simulate a reveal by installing and restoring a temporary selection.
The existing edit transaction and passive edit-plan APIs remain unchanged.

## Verification boundary

Mounted GPUI virtual-platform tests exercise immutable decorations, atom seams,
shape-cache stability, native focus ownership, typing grouping, ordered/reverse
selection starts, actual wrapped mixed-size/right-aligned layout, nested table
horizontal plus document-vertical scroll, stale/session replacement guards,
query cancellation and queued-request supersession. They are deterministic
layout/focus evidence, not real font, native IME or product GUI acceptance.
