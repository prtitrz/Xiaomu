# ADR 0012: Explicit whole-block node selection

Status: Runtime foundation implemented, 2026-10-04. Native view and product
commands are separate follow-on work.

## Need

Some host commands select the complete next block, including a quote or list.
Representing that as its first text caret loses the intended Copy/Delete/Undo
boundary. Inferring node selection from matching gap endpoints is equally
incorrect: a normal range, a single block, and explicit All have different fits.

## Contract

`DocumentSelection::node(document, id)` creates explicit private node/root
provenance, with the parent gaps immediately around that node. The existing
`DocumentPosition` enum remains unchanged. `as_node_selection()` exposes the
selected ID; `DocumentSession::set_node_selection(id)` validates before changing
selection, marks, history grouping or listeners. Repeating the selection is a
true no-op.

A sole root child is not AllSelection, even though its gap endpoints match
All. Existing atomic selections remain distinct. Supported whole blocks are
paragraphs, headings, Quote, ordinary/Task lists, CodeBlock, HR, Image and Table.
Independent item/row/cell/custom/inline-atom/root targets are excluded until their
detached fragment schema and product behavior are defined. Supported containers
still copy all legitimate nested children, attributes and inline atoms.

The private root ID rejects a different root, not every foreign session: node
IDs remain document-local. Hosts continue to enforce their session/generation
identity. No canonical attribute or persistence sidecar is introduced.

Mapping uses node identity and inward-biased parent gaps. `PreserveSelection`
reconstructs current gaps from the same ID in the final snapshot, including an
atomic remove/restore move between parents. Deletion/invalid-kind mapping fails
instead of inventing a fallback caret. A host wanting Selection.near must plan
that explicitly. Raw transaction history stores the exact before/after tag.

Copy projects exactly one complete subtree as a closed slice. Existing v11 and
conditional Task v12 preserve provenance and reject lossy downgrades; wire
schemas are unchanged. The new tag's default selection-driven edits fail closed
until a node-aware policy handles them. Explicit selection departure and
identity-addressed SetTaskChecked are still supported. Policy preflight occurs
before that default guard.

## Verification and remaining work

Nineteen targeted Runtime tests passed, including closed copies, stale/invalid
targets, exact attrs/atoms, atomic selection rollback, mapping, moves, Undo/Redo,
checkbox identity updates and unchanged legacy selection paths. The complete
workspace/all-targets suite passed 943 tests and strict Clippy.

No node-selection GPUI highlight, input proxy, caret navigation, Cut/Delete or
host HR command completion is claimed. Those must read explicit provenance,
preserve IME event ordering and avoid rebuilding the tag as an ordinary gap
range. Existing Atomic/All behavior is not evidence of new-container UI parity.
