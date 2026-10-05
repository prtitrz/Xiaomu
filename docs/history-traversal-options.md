# Immutable history traversal options

The following explicit-timing stage is documented separately in
[timed history options](timed-history-options.md). Its default-compatible options
and frontend clock do not replace the traversal guarantees below.

2026-10-05. `SessionPolicy::history_options()` is evaluated once at construction.
Its value is copied into the session and cannot be replaced while it is live.
Defaults preserve the existing recorded-selection/empty-stack behavior.

## Independent choices

`HistorySelectionMode::Recorded` restores the entry's existing before/after
selection without recapturing. `CaptureOnTraversal` changes only successful
traversal: Undo stores the pre-Undo current selection in the entry's after side,
and Redo stores the pre-Redo current selection in its before side.

Captured selections belong to the next opposite-direction snapshot. A long
source caret may be outside the shorter Redo target, or a source CellRange may
refer to a table that Redo removes. Do not validate or map that captured value
into the current target. Exact identity-preserving inverse/redo restores its
own snapshot on the next traversal. Actual Core/candidate/target-selection
validation still happens before publication. Failed traversal leaves both
entries/stacks and editing transients unchanged and captures nothing.

`EmptyHistoryBehavior::ClearPendingMarks` retains the previous behavior of
clearing marks and ending grouping even with no entry. `PreserveEditingState`
returns NoChange before either operation on an empty requested stack. It keeps
marks, selection, grouping, input-rule state, revision and notifications intact.
Successful Undo/Redo still clears pending marks; they are not stored in bookmarks.
The two choices are independent, including capture with legacy empty behavior.

The configuration is neither a canonical document attribute nor a serialized
history format. There are no new callbacks during Undo/Redo, no switchable live
policy and no per-command options that could change existing entries' meaning.
The scoped prepared-Cut borrow continues to prevent stale concurrent history
navigation without relying solely on a document revision.

## Evidence and limits

Thirteen Runtime tests cover public default/capture workflows across text, reverse
ranges, All and CellRange, source-only caret/cells, repeated/two-level traversals,
per-instance construction-time capture and independent empty-stack options.
Private Core, selection and candidate-policy fault sentinels are explicitly
separated from publicly reachable workflows; they verify complete entries,
transients, notifications and repaired retries against unaffected controls.

This traversal option alone does not implement ProseMirror map-only entries or
bookmark fallback. The separate timed stage adds bounded native typing delay and
ordinary selection preservation, not all PM grouping. The host's actual factory oracle and native
consumer/GUI conformance remain separate from these generic library guarantees.
Do not label selection capture as complete ProseMirror history compatibility.
