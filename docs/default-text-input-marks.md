# Default text-input mark consumption

2026-10-05. `SessionPolicy::default_text_input_marks()` is captured once when a
session is constructed. `DefaultTextInputMarks::PreservePending` is the default;
`ConsumePending` is a separate explicit choice, independent from HistoryOptions.

Automatic validation:1320 workspace/all-target tests across104 binaries, six
prepared-Cut compile-fail doctests, strict Clippy/fmt/source-size/dependency and
audited decoder checks pass. This is not a native GUI result.

Only a nonempty InsertText or CommitComposition that reaches the default
single-inline planner is affected. Planning first uses explicit pending marks
normally, including Some(empty), and creates canonical marked/unmarked text.
The successful Commit plan then requests `with_stored_marks(None)`. Existing
Core, target-selection and candidate-policy validation completes before this
state is published. The final document, selection and consumed pending marks
are installed before document listeners; there is no second notification or
post-listener repair. Subsequent text inherits the newly inserted runs.

Default ordinary typing keeps its existing HistoryPolicy::Typing and eligible
Unicode/batched input grouping. Composition keeps its existing isolation. Host
Apply plans are not used as a wrapper for ordinary input, since that would break
grouping. A rejected plan preserves the previous state and history exactly.

## Narrow boundary

- No-policy sessions, default policies and explicitly Preserve sessions retain
  their previous behavior
- Empty input, including an empty-string range deletion, is excluded; existing
  NoChange and empty-composition history-boundary contracts are unchanged
- Host Apply/input rules/explicit marks-after, raw commits, paste, Backspace,
  Delete, Enter/split and staged plans retain their own existing contracts
- CellRange input is an earlier independent route which already clears pending
  marks; this option does not introduce or replace that behavior
- This is not an all-canonical-step clearing policy. PM deletion and split may
  restore marks after steps, while an appended transaction can clear them again

Thirteen public-API tests compare defaults and opt-in across plain/empty nodes,
Bold and explicit empty marks, Unicode continuation, composition replacement,
mixed-inline and typed-hardBreak seams, no-ops, UTF-8/planner/policy rejection,
and an atom-crossing composition Core failure after plan decoration,
redo preservation, explicit host marks-after and excluded command boundaries.
The listener API exposes document/selection, not stored marks: callback counts
and returned final state are tested, while before-notification ordering is also
established by the existing atomic publication path's source contract.

The consumer's actual original full-factory/DOMObserver evidence is distinct
from these generic guarantees. Synthetic DOM or composition events are not
trusted browser input or OS IME acceptance. Time-based typing groups, complete
PM transaction semantics and all remaining migration work are separate.
