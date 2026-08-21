# P0 Core Contract Progress

Status: Active

This file tracks execution evidence for P0. It is intentionally operational. Long-term architecture belongs in `docs/architecture.md`; phase design belongs in `design.md`; top-level direction belongs in `docs/planning.md`.

## Status legend

```text
[ ] not started
[~] in progress
[x] complete
[!] blocked / needs decision
```

## Current state

Current slice: **P0.0 Phase contract and module skeleton**

Branch: `feat/p0-core-contract`

P0 started with the phase design and progress documents in place before implementation.

## P0.0 Phase contract and module skeleton

- [x] Create `docs/phases/p0-core-contract/design.md`
- [x] Create `docs/phases/p0-core-contract/progress.md`
- [x] Add/confirm `xiaomu-core` module skeleton for `document`, `text`, `selection`, `transaction`, `mapping`, `history`, and `commands`
- [x] Add initial Core error/result types
- [x] Confirm `#![forbid(unsafe_code)]` remains active in Core
- [x] Review public/private visibility of bootstrap APIs
- [x] Synchronize `docs/architecture.md` with the new Core module boundary
- [ ] Run repository source-size and dependency-boundary guards
- [ ] Run `cargo fmt --all -- --check`
- [ ] Run `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Run `cargo test --workspace --all-targets`

Exit evidence:

```text
Implementation skeleton complete; CI evidence pending.
```

## P0.1 Text boundary

- [ ] Implement `TextBuffer`
- [ ] Implement `TextOffset`
- [ ] Implement `TextRange`
- [ ] Validate UTF-8 char boundaries on construction/use
- [ ] Add safe slicing APIs
- [ ] Add safe replacement APIs
- [ ] Add ASCII fixture
- [ ] Add Chinese fixture
- [ ] Add mixed Chinese/Latin fixture
- [ ] Add emoji fixture
- [ ] Add combining-mark fixture
- [ ] Add BiDi fixture
- [ ] Test invalid boundary errors
- [ ] Confirm expected invalid input never panics

Exit evidence:

```text
pending
```

## P0.2 Document model

- [ ] Implement `DocumentVersion`
- [ ] Implement `DocumentRevision`
- [ ] Implement opaque `NodeId`
- [ ] Provide deterministic NodeId allocation for tests
- [ ] Implement `NodeKind`
- [ ] Implement `NodeAttrs`
- [ ] Implement `NodeContent`
- [ ] Implement `Node`
- [ ] Implement `NodeStore`
- [ ] Implement externally immutable `XiaomuDocument`
- [ ] Implement root/tree validation
- [ ] Reject unknown child IDs
- [ ] Reject invalid parent/child shapes
- [ ] Reject root removal/invalid root states
- [ ] Implement `TextRun`
- [ ] Implement `Mark` / `MarkSet`
- [ ] Normalize adjacent equal-mark runs
- [ ] Reject persistent empty runs
- [ ] Demonstrate node-level structural sharing across a revision

Exit evidence:

```text
pending
```

## P0.3 Position and selection

- [ ] Implement `CursorAffinity`
- [ ] Implement `TextPoint`
- [ ] Implement `TextSelection`
- [ ] Implement `NodeSelection`
- [ ] Implement structural boundary position (`NodeGap` or final equivalent)
- [ ] Validate selections against document snapshot
- [ ] Test invalid/deleted node positions
- [ ] Test Chinese/emoji text positions

Exit evidence:

```text
pending
```

## P0.4 Transaction application

- [ ] Implement typed `Transaction`
- [ ] Implement transaction origin
- [ ] Add metadata seam without host-specific types
- [ ] Implement `ReplaceText`
- [ ] Implement `InsertNode`
- [ ] Implement `RemoveNode`
- [ ] Implement `SetNodeAttrs`
- [ ] Implement `AddMark`
- [ ] Implement `RemoveMark`
- [ ] Ensure apply returns a new snapshot
- [ ] Ensure apply validates resulting document
- [ ] Confirm no public direct canonical mutation escape hatch

Exit evidence:

```text
pending
```

## P0.5 Position mapping

- [ ] Define P0 mapping result semantics
- [ ] Implement text replacement mapping
- [ ] Implement insertion mapping
- [ ] Implement deletion mapping
- [ ] Implement removed-node result
- [ ] Compose mappings across a transaction
- [ ] Add mapping tables for insertion/replacement/deletion
- [ ] Add Chinese/emoji mapping fixtures
- [ ] Add removed-node mapping fixtures

Exit evidence:

```text
pending
```

## P0.6 Inverse and randomized invariants

- [ ] Define inverse/change-set prototype boundary
- [ ] Invert text replacement
- [ ] Invert node insertion/removal
- [ ] Invert attrs changes
- [ ] Invert mark changes
- [ ] Add semantic round-trip helper
- [ ] Add deterministic multi-step inverse tests
- [ ] Add randomized valid transaction sequence tests where practical
- [ ] Confirm random sequences preserve document validity
- [ ] Confirm random sequences do not panic

Exit evidence:

```text
pending
```

## P0.7 Contract stabilization

- [ ] Review public rustdoc for semantic contracts
- [ ] Document offset units explicitly
- [ ] Document mapping deletion behavior explicitly
- [ ] Update `docs/architecture.md` to implemented truth
- [ ] Record any accepted long-lived ADRs
- [ ] Review `docs/planning.md` for P0/P1 consistency
- [ ] Record unresolved P1 dependencies
- [ ] Run full `CI Success`
- [ ] Mark P0 complete

Exit evidence:

```text
pending
```

## Phase Gate

P0 completes only when:

- [ ] versioned structured document model is implemented
- [ ] snapshots are externally immutable
- [ ] NodeId/NodeStore structural-sharing prototype works
- [ ] TextRun-local marks normalize deterministically
- [ ] TextOffset/text boundary tests are green
- [ ] position/selection model validates correctly
- [ ] basic typed transactions preserve invariants
- [ ] StepMap/ChangeMap prototype maps old positions explicitly
- [ ] inverse prototype restores semantic original state
- [ ] Unicode/CJK/emoji tests are green
- [ ] property/randomized invariant tests are green
- [ ] `CI Success` is green

## Decisions / questions log

Record only decisions that affect P0 execution here. Durable architectural rationale should move to an ADR.

### 2026-08-22

- P0 uses dedicated `design.md` and `progress.md` under `docs/phases/p0-core-contract/`.
- Phase documents refine the top-level plan but do not duplicate it.
- Core text offsets start as validated UTF-8 byte offsets behind an opaque newtype; UTF-16 remains a frontend/platform concern.
- P0 starts with `String` behind `TextBuffer`; rope selection remains benchmark-driven.
- Node storage starts with standard-library ownership/structural-sharing primitives rather than binding the public contract to a persistent-collection crate.
- The bootstrap `CRATE_NAME` constant was removed instead of carrying an accidental public API into P0.
- Core semantic modules are public boundaries; the error implementation module remains private and re-exports only `Error` / `Result`.

## Regression log

No regressions recorded yet.
