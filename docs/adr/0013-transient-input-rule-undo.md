# ADR 0013: bounded opt-in input-rule restoration

Status: accepted Runtime foundation, 2026-10-04. Product integration is separate.

An input-rule Backspace can restore the marker including its trigger, unlike
ordinary history Undo. Runtime must not infer host syntax from current nodes.
`EditPlan::with_input_rule_undo` optionally carries an `InputRuleUndoSpec` with
an exact reversal transaction, selection and optional typing marks. A readonly
context query exposes eligibility; only an explicit host `UndoInputRule`
disposition consumes it. Default editing behavior is unchanged.

Forward and reversal candidates, selections, marks, host validation and payload
admission are checked before publishing document/history/listeners. The one
session token binds the actual committed revision and selection, not private
planner revisions. Failed intents/staged/history operations restore the token.
Successful document/history edits and explicit selection changes, including
same-coordinate calls, clear it. Copy and readonly queries preserve it. Undo
and Redo do not serialize or recreate the token. Truly empty history queries
remain no-ops. No token enters document JSON or persistent storage.

Admission bounds owned payload to 64 steps, 256 payload nodes and 64 KiB, with
32 levels of attribute nesting. It is not an execution-memory sandbox: compact
commands can expand data. Hosts must supply validated bounded rule reversals.
Oversized specs return an error before conversion; hosts may choose literal
input instead. No silent removal of restoration metadata is promised.

This first version publishes restoration as an isolated forward history unit.
It does not reproduce ProseMirror's continuous typing/history grouping. Hosts
must also account for later rule-chain changes: the measured root-only
Task/Quote TrailingNode append invalidates restoration, unlike HR's successor
created inside its own rule transaction. No generic tree heuristic applies.

`EditIntent::InsertHorizontalRule` is an additional logical host command with
default `UnsupportedEdit`; Runtime adds no automatic HR insertion semantics.
Custom exhaustive intent/disposition matches need to handle both new variants.

Validation: 438 Runtime tests and strict Runtime all-target Clippy passed;
31 dedicated tests cover payload limits, publication/rollback, selection and
history lifecycle, Unicode/atoms/marks and identity-preserving restoration.
No product-host input-rule or native GUI completion is claimed by this ADR.
