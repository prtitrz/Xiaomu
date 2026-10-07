# Committed snapshot import

Independent `EditorInstance`s retain independent documents, selections, typing
marks, Undo/Redo and native input state. A saved snapshot can be copied between
them without rebuilding either editor or sharing its `DocumentSession`.

## Core: bounded copying, ordinary inverses

`DocumentTemplate::capture(&document)` copies a validated document into an
immutable, identity-free template. It preserves root attributes, every current
canonical node kind, inline text and atom marks, missing/null attributes, table
row/cell wrappers, spans and nested tables. It does not copy source revision,
selection, allocator state or canonical identities. Cloning shares the template.

`TransactionStep::ReplaceDocument { template }` keeps the receiver's root ID,
replaces its attributes, and allocates fresh destination-lineage identities for
all descendants. It is an ordinary atomic Core transaction. Capture and apply
perform resource admission before copying payload; the exact limits and what
they account for are documented on `DocumentTemplate`. Resource failures do not
consume destination IDs. The snapshot-operation budget is cumulative within a
transaction and includes operation overhead; at most 64 replacement/restoration
steps are admitted. Ordinary non-snapshot steps keep their existing resource
rules. Table-grid limits remain an independent admission gate.

The engine-produced, opaque `DocumentRestore` inverse restores exact identities
and payloads in the same lineage, including when earlier Undo/Redo reconstituted
the expected store with a different allocation. It rejects a stale store or an
independent document, even if its visible nodes compare equal. Allocator
high-water never moves backwards. Ordinary ChangeMaps identify removed
subtrees and inserted root children. This is not a runtime reset or a foreign
`RestoreSubtree` copy.

## Runtime: explicit selection and publication origin

`DocumentSession::apply_plan(EditPlan)` is the public checked-plan entry point.
It uses normal Core, after-selection and final `SessionPolicy` validation before
publishing document, selection and history together. It does not call typed
intent preflight. Failure also preserves pending marks, the typing-group
boundary and the optional input-rule undo token. It never clears earlier Undo.

Host-created plans form one isolated Undo unit and clear Redo. This deliberately
does not promise another editor engine's short-delay typing/replacement grouping.
Even an empty transaction applies its explicit selection and records history,
matching raw `DocumentSession::apply`; it is not silently treated as a no-op.

Selection is a caller decision:

- `CaretAtDocumentEnd` chooses the last editable inline descendant, including
  trailing atom ordinals and empty paragraphs/cells. With no inline descendant,
  it chooses the valid root-end gap
- `AllDocument` chooses the new whole-root range
- `Exact` and `CaretAtGap` support other explicitly planned outcomes

Undo restores the exact receiver selection and store from before the import.
Redo restores the exact imported IDs and after-selection. The sender's selection
is never copied implicitly.

`EditPlan::with_change_origin(DocumentChangeOrigin::External)` classifies only
that commit's synchronous notification. `document_changed_with_origin` defaults
to the old `document_changed` listener for compatibility; overriding it allows a
host to distinguish an accepted external snapshot without suppressing listeners.
Subsequent Undo/Redo notifications are always `Local`. Classification does not
authorize a save, mark a revision clean, or alter history behavior.

## GPUI: passive receiving-pane refresh

`DocumentView::apply_passive_edit_plan(&plan, window, cx)` returns:

- `Ok(None)` when any receiving child or range-input surface is composing
- `Ok(Some(outcome))` after a checked commit and view refresh
- `Err(error)` without publishing a change or advancing the view epoch

Composition is left untouched. The caller may retry after composition finishes,
but must recheck its binding, base revision and cleanliness first. There is no
forced-preedit commit or internal queue.

A successful call invalidates stale geometry and interactions, syncs children,
and repaints. It captures this view's native focus before old input children are
dropped, and restores input only if this view already owned it (including the
root/range proxy). It cancels fresh-child caret-scroll requests and never asks to
scroll to the imported caret. Other panes retain their native focus and scroll.
Native viewport clamping after content height changes is not a promise of fixed
pixel position for every possible replacement.

Final session policy still owns semantic/capability admission. Identity-dependent
measured-table presentation requires a fresh receiving-view layout; old measured
admission cannot be used on an unrelated decoded snapshot. The normal protected
placeholder and edit-guard behavior remains until presentation is admitted.

## Host synchronization boundary

This API does not choose dirty-peer conflict policy, persist content, publish
receipts, acknowledge autosave revisions, or coordinate view lifetimes. A host
must validate those conditions and apply/acknowledge without yielding. Listener
callbacks run while the session is mutably borrowed and must not reenter it.
Queue cross-view delivery outside borrowed save/listener callbacks.

Safe committed-snapshot synchronization publishes only durable successful saves,
retains a dirty peer's draft, rejects stale-base saves, and advances the receiving
accepted base only after the import succeeds. An external Undo is an ordinary
local edit that becomes dirty under the host's own policy. Identical-body import
skipping is also a host decision; Core does not normalize away a requested edit.

## Evidence boundaries

Core and Runtime tests verify resource refusal, exact payload/selection round
trips, earlier-history retention, origin classification and atomic failures.
GPUI tests use mounted virtual windows for focus, native input-handler routing,
composition and scroll-request behavior. These tests are not actual platform IME
or product persistence/database acceptance. Live collaboration, selective
per-author Undo and rapid replacement/typing grouping parity are outside this
seam.
