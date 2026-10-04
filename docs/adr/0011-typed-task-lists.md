# ADR 0011: Typed task nodes and exact task clipboard metadata

Status: Accepted for canonical/clipboard foundation; task editing and native UI pending
Date: 2026-10-04

## Context

A task list has semantic item state that an ordinary bullet list does not carry.
Representing tasks as bullets, or flattening a task clipboard fragment to its
inline leaves, loses both the kind and the checked attribute. Missing, explicit
null, false and true must remain distinguishable through copy and inverse edits.

A host's persistent-document schema can be stricter than Core's generic tree.
For example, a host may require a task item to start with a paragraph and a task
list to contain at least one item. Applying those product rules to every generic
Core snapshot would prevent existing validated construction/lift staging, and
applying them to an open clipboard fragment would reject a selection beginning
inside an attached code block.

## Decision

- Add builtin `NodeKind::TaskList` and `NodeKind::TaskItem`, distinct from
  `BulletList`, `OrderedList` and `ListItem`
- Both are child containers. TaskList accepts only TaskItem children; TaskItem
  accepts ordinary blocks, including nested task/ordinary lists, code, images,
  quotes and tables, but not bare item/row/cell/inline-atom nodes
- Core permits empty task containers and any valid first block. A stricter host
  codec and final session validator own paragraph-first/nonempty product rules
- TaskItem stores checked state only in `NodeAttrs["checked"]`. Missing, Null,
  Bool(false) and Bool(true) are preserved exactly; other values fail with
  `Error::InvalidTaskItemChecked`. Missing/null display as unchecked without
  read-time normalization. Other Core attributes remain extensible
- Generic transactions, subtree restoration, identities and mappings are reused;
  no checkbox shadow state, new coordinate system, or host-specific compound
  transaction is introduced. Canonical `DocumentVersion` remains v1

## Clipboard compatibility

A slice containing TaskList or TaskItem anywhere uses conditional metadata v12.
The traversal includes ordinary children and rectangular table rows/cells.
`task_list` and `task_item` are explicit kind tags. v12 always writes a boolean
`closed`, false for open source ranges and true for explicit whole-root sources;
missing/null/nonboolean boundaries are rejected. Checked and all other attrs use
the existing lossless tagged value encoding.

v1-v11 cannot carry task tags. Historical v1-v3 remain unsupported; valid v4-v11
non-task payloads keep their prior encoding and boundary rules. Existing raw
JSON duplicate detection, unknown-structural-field rejection, fallback equality,
and byte/value/depth budgets apply to v12 unchanged. Attribute object keys remain
extensible. Encoding a non-task slice never selects v12.

Generic source projection preserves task wrappers and attrs, including pruned
open task items beginning with attached code. A single-inline-leaf selection
continues to select only that leaf, as it already does inside ordinary lists.
This foundation does not infer whole-item provenance from text coverage.

Default `PasteSlice` rejects any task-containing slice with
`SessionError::UnsupportedEdit` before default closed/table/leaf fitting or
session state changes. A policy's `prepare_intent` still runs first and may
return an explicit `Apply` plan. This prevents a one-leaf task slice from being
silently flattened while leaving task-aware host fitting to its own contract.

The generic Markdown serializer explicitly rejects TaskList and TaskItem with
`UnsupportedNodeKind`; it does not export tasks as ordinary bullets.

## Verification and boundaries

Source regressions cover exact checked states and generic attrs, invalid checked
values, mixed subtree removal/restoration/mapping, empty/nonparagraph task staging,
open and closed source projection, code/images/marked HardBreaks, tasks in table
payloads, downgrade attempts, raw duplicates and resource limits, and atomic
default-paste rejection with policy interception. These source tests require the
integration owner's Cargo gates; writing them is not evidence they have passed.

The initial foundation did not implement task commands or controls. The next
local checkpoint adds `EditIntent::SetTaskChecked { item, checked }` and a real
GPUI pointer checkbox. Actual mutations patch only `checked`, preserve exact
selection, clear pending marks and create an isolated undo unit. An identical
boolean is a true no-op preserving typing state. Runtime rejects stale IDs and
wrong kinds; host validation still gates every candidate.

The control captures only the stable item ID, reads live state at click, and
focuses the existing selection even after accepted NoChange without requesting
scroll. Task content, nested lists, images and code stay in the content column.
No extra IME wait or platform input owner is introduced. GPUI 0.2.2 does not
expose checkbox role/checked accessibility builders; the pointer implementation
does not claim those platform semantics.

The integrated workspace/all-targets suite passed 922 tests and strict Clippy
on 2026-10-04. Twelve new control tests comprise ten virtual event/layout tests,
one explicitly callback-level no-render test, and one attribute projection.
GPUI public test input flushes effects between pointer events; the callback
test is not evidence of OS pointer delivery in a single paint frame. The IME
test models platform unmark-before-click, pending real X11 acceptance. Product
keyboard planners, persistence adapters and production migration remain
separate host work. Product history can coalesce checkbox actions; the isolated
native undo boundary is an explicit stronger transaction policy.

## Alternatives considered

- Ordinary lists plus `checked`: rejected because typed task/item identity would
  be lost and ordinary-list planners could silently change semantics
- A separate canonical checked field: rejected as redundant state alongside the
  preservation-friendly attribute representation
- Core-wide host paragraph-first rules: rejected because Core staging and open
  source fragments are not the host's persisted document schema
- Reusing generic hierarchical paste: rejected until task-aware fit semantics are
  proven, especially for single-leaf sources, open boundaries and mixed nesting

## Revisit when

A generic task-editing contract can be supported across hosts, or a canonical
schema version boundary is introduced for serialized documents. Host task
interaction and real-platform acceptance remain separate gates.
