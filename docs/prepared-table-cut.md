# Scoped prepared CellRange Cut

2026-10-05. This explicit host opt-in does not change generic Delete, legacy
unit-cell projection, default Paste fitting or schema admission.

## API and publication

`SessionPolicy::prepare_cut(context)` defaults to `Ok(None)`. A host may supply
one dedicated CellRange `EditPlan` and an explicit Cut export spec. The session
then evaluates bounded source projection, actual Core candidate, final collapsed
inline selection, candidate policy and exact inverse/identity-preserving redo.
Empty transactions and non-CellRange dedicated plans reject. Projection-only
opted-in Cut remains rejected; projection alone never approves deletion.

Opaque `PreparedCut<'a>` exclusively borrows the same live session and exposes
only `clipboard_slice()` and single-use `publish(self)`. Dropping it preserves
document/revision/allocator, selection/marks, complete history/grouping,
input-rule token and listeners. The borrow prevents source edits, selection-only
changes, Undo/Redo, overlapping guards, owner substitution and duplicate publish.
There is no detached token checking only document revision.

Ordinary commit and prepared Cut share the same private preparation function.
Cut forces isolated history, marks=None and no input-rule undo token, but only
on publication. The publisher installs the exact candidate/history once,
without another Core apply, policy callback or selection resolution. No IDs are
predicted or reallocated after the external write.

## GPUI sequence

Before any Cut write, the view checks hidden endpoints and active composition.
For a dedicated plan it prepares the guard, constructs an owned lossless platform
item using the actual receiving decoder, invokes stock GPUI once, then publishes
that exact guard. The session borrow ends before view epoch/children/focus/scroll
updates or typed ClipboardCut rejection callbacks. All ordinary metadata,
source, policy, Core, selection and budget failures occur before the writer.
No apply-then-Undo preflight or compensating clipboard restoration is used.

`None` preserves the legacy projection/write/generic-Delete route, with only the
new early hidden/composition checks. Legacy Delete can still reject after a
write. The prepared-path guarantee must not be attributed to that unchanged
path or described as an all-Cut atomicity fix.

## Platform limits

Stock GPUI0.2.2 `write_to_clipboard` returns unit. X11 logs a set-text failure and
replaces its cached item; Wayland can return without publishing when the device
or focus is absent. Success here means codec preflight passed and the writer
was invoked, not that the OS acknowledged ownership. Semantic-failure atomicity
is not crash atomicity across the clipboard and document. Allocation failure,
listener panic, process exit and external ownership changes are outside this
bounded guarantee. Read-back equality is not an acknowledgment/rollback contract.

## Evidence

The current library has1294 passing all-target tests, plus six passing borrow
compile-fail doctests. Nine new Runtime tests compare full history transactions,
group/token/listener state and future allocated IDs for success, rejection and
drop. Seven actual virtual-GPUI tests cover candidate validation before writer0,
publication after writer1, exact Undo/Redo, reborrow-safe rejection, dropped
guard/item, hidden/composition guards and real legacy metadata-budget rejection.
Passive counters observe actual writer calls without replacing writer or result.

The prepared source projection is more conservative than the wire decoder for
known payloads. No naturally accepted dedicated Slice that later exceeded the
metadata limit was found. Its metadata-error branch is source-reviewed; real
legacy codec rejection and guard cancellation are tested separately. No fake
decoder bypass is used to claim end-to-end reachability of that branch.

Host product code separately owns selected-origin deletion, factory defaults,
complete work budget and original history parity. Native OS/GUI acceptance is
also separate. No growth/repetition/arbitrary target clipping, external HTML,
new attrs, platform backend or legacy GPUI fork is enabled by this API.
