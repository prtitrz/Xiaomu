# Opt-in image-paste source admission

`EditorCommandRouter::admit_image_paste` is a read-only GPUI transport hook.
Existing routers need no new implementation: `ImagePasteAdmission::Default`
retains the collapsed-inline, non-code image route and its side-effect order.
Install it with the existing `EditorInstance::with_command_router` or
`DocumentView::set_command_router`; no new `EditorHooks` field is required.

`Allow` admits only the complete original selection to the existing synchronous
`AssetService::import_image` followed by `EditIntent::InsertImage` route. It does
not return a replacement selection or supply an insertion planner. A host must
implement any additional range, All, node or cell semantics in `SessionPolicy`.
Generic transport tests with synthetic planners are not downstream planner,
resource-lifecycle or native OS clipboard acceptance evidence.

## Ordering and guards

- Recognized native metadata is validated first. Valid structured data retains
  priority; invalid native metadata rejects without image or text fallback
- At a code target, nonempty text accompanying an image keeps its existing
  `route_code_paste` opportunity. If consumed, image admission is not called
- Ordinary text and structured paste never enter image admission
- Read-only input and native composition stop paste before the callback
- `Default` retains all legacy image source-shape checks, including code refusal
- `Allow` checks current hidden-table endpoints/active range, read-only input,
  composition and original selection validity before import. This endpoint
  guard does not scan all descendants between All/root-gap endpoints
- An admission error consumes the paste before import, with payload-free
  `CommandRouting / Policy` feedback emitted after the callback borrow ends
- Import must still succeed with an `ImageSource::AssetRef`. The central view
  guards, original session policy and final candidate validation still apply

The context includes the original selection, direction, affinity, node identity,
All gaps, active cell range, parked endpoints and typing marks. It contains no
image bytes, external handle, resource credentials or product-specific identity.
Callbacks are pure, read-only and non-reentrant, with no imports, dispatch,
mutation or external side effects.

## Synchronous source protection

For `Allow`, a private local stamp captures session identity, document revision,
full selection and stored marks before import. No session borrow spans the
external importer call. After a successful import, insertion proceeds only if
those fields still match and mutable session access and view guards allow it.
There is no await, request manager, replacement-selection installation or retry
at a new caret.

Same-session revision, selection or marks changes cancel insertion and emit
`ClipboardPaste / InvalidSelection` after releasing the borrow. A replaced
session or a retained session borrow silently cancels, avoiding feedback or
insertion into another session. Queued rejection delivery follows the ordinary
GPUI event contract and carries only stage, reason and current revision.

Cancellation leaves the importer's latest state intact. It prevents this stale
image's native edit/history publication; it does not roll back arbitrary importer
side effects. Successful import followed by cancellation can retain cached
assets under the existing `AssetService` lifecycle contract. This is no promise
of zero disk writes, immediate garbage collection, or protection against every
change-then-restore sequence. Importers remain responsible for not editing the
session. No document/store copy, resource-generation check, new importer trait
or Core geometry is introduced.
