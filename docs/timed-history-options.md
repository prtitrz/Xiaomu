# Explicit timed typing groups and shared frontend clock

2026-10-05. Library1390 all-target tests across106 binaries, six prepared-Cut
compile-fail doctests and strict Clippy/fmt/source/dependency/vendor gates pass.
Runtime621 and GPUI400 tests are included. This is engine/frontend automatic
validation; original consumer-oracle replay, product opt-in and GUI are separate.

## Fixed options and explicit time

`HistoryOptions::with_typing_group_delay_ms` enables an optional inclusive,
sliding limit on existing eligible native typing. `SelectionOnlyGrouping` is an
independent Close(default)/Preserve choice. Defaults preserve timeless grouping
and selection boundaries. Neither option changes history selection capture,
empty-stack behavior or default input-mark consumption.

`HistoryTimestamp::from_millis` supplies one session's nonnegative monotonic
millisecond domain. `apply_intent_at` and `apply_intent_with_selection_at` are
additive APIs; old APIs remain unstamped. There is no ambient next timestamp,
Runtime clock query, persisted time, history-entry timestamp or redo-time replay.

The timestamp stays in the prepared candidate until successful publication.
Only eligible typing records it. Native node/adjacency/exact-selection predicates
still apply; gap<=500 groups under a500 limit,501 splits, and each successful
edit refreshes the anchor. Selection movement never refreshes edit time.

Zero/equal/MAX stamps are valid. Checked subtraction prevents overflow. Under a
timed option, missing or regressing stamps commit valid text as separate events
and close the reusable anchor without lowering the accepted high-water mark.
This is a robustness rule, not parity with PM wall-clock regression/sentinel
behavior. Timeless sessions ignore supplied timestamps.

`close_history_group()` is an idempotent before-only boundary. It does not edit
the document, selection, marks, input-rule token, stacks or notify listeners;
the next eligible edit can form a new group. It is not two-sided isolation.

## Atomicity and preserved exceptions

All ordinary failures restore complete cheap grouping state: open flag, last
eligible successful edit and high-water. Core/selection/candidate/inverse checks
precede publication. Prepared Cut preparation, failed item preparation and guard
drop leave time unchanged; successful Cut is still one isolated publication.
Undo/Redo closes anchors but never replays timestamps, and empty-stack choices
retain their existing independent contracts.

Preserve changes ordinary direct selection installation only. Atomic changed
targets and existing CellRange convergence remain barriers, even if the later
navigation action is itself NoChange. Marks clearing/token invalidation remain.

Text-only empty InsertText and policy NoChange do not publish time. The existing
mixed-inline atom route instead commits an isolated empty ReplaceInlineText;
that legacy boundary is preserved and explicitly tested. Empty plain composition
also retains its prior grouping boundary. Do not call every empty request inert.

Host Apply/StoredMarks, input rules, composition, raw/staged edits, cell edits,
paste and deletion keep existing eligibility/isolation. Broad PM map-based
adjacency, deletion/composition/pending-mark grouping, appended transactions and
map-only nonhistory/bookmark fallback are not implemented by this option.

## GPUI ownership

Each `EditorInstance` owns one fixed shared `HistoryClock`. The default
`MonotonicHistoryClock` uses elapsed Instant milliseconds and safe saturation.
`new_with_policy_and_history_clock` accepts an explicit provider for deterministic
or embedded use; there is no mutable live clock setter.

All built DocumentViews and normal/nested/table/range/late-created ParagraphViews
share that origin. Central and native input sample once before mutable Runtime
dispatch. Hidden-table/composition/refusal guards and atomic replacement remain
in place. Preedit/cancel does not publish time; committed composition stays
isolated. Legacy standalone view constructors remain unstamped; explicit
`new_with_history_clock` constructors allow hosts to share the same provider.
Different clock domains must not be mixed within one shared session.

Deterministic virtual-platform tests cover exact499/500/501 boundaries, Unicode,
shared/later/new child views, independent instances, no-clock fallback, selection
away/back, atomic replacement, cancellation/refusal and all existing isolation.
They do not establish physical event timing, OS IME or actual GUI behavior.

The integration retained two ordinary test-development failures: a mixed-inline
fixture's leading Italic atom makes SetMark(Bold) produce Bold+Italic, and one
test passed &str to TextBuffer::from_string. Assertions/types were corrected;
production mark semantics were not changed to satisfy those assumptions.
