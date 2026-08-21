# P0 Core Contract Design

Status: Active

This document is the executable design for P0. The top-level roadmap remains in `docs/planning.md`; architecture that is already true remains in `docs/architecture.md`.

P0 exists to establish Xiaomu's canonical document semantics and mutation contracts before any native frontend code is introduced.

## 1. Scope

P0 is a `xiaomu-core` phase. `xiaomu-testkit` may gain helpers used to exercise Core invariants.

P0 must deliver:

```text
versioned document schema
externally immutable document snapshots
stable opaque NodeId values
NodeStore and a structural-sharing prototype
TextRun-local normalized marks
TextOffset and Unicode text boundaries
position and selection primitives
typed transaction primitives
StepMap / ChangeMap prototype
validation
inverse/change-set prototype
property and regression tests
```

P0 has no GPUI dependency.

## 2. Non-goals

P0 does not implement:

```text
native rendering or input
IME composition runtime
clipboard integration
host persistence APIs
full command/keybinding behavior
collaboration protocol
collaborative undo
production virtualization
full table editing
InlineAtom interaction semantics
Markdown editing semantics
```

P0 may reserve types or seams required by later phases, but it must not implement speculative systems solely for future completeness.

## 3. Core invariants

The following are hard P0 invariants.

### 3.1 Canonical document state is structured

External formats are codecs. No Markdown, HTML, source byte range, or GPUI type is part of canonical document identity.

### 3.2 Documents are immutable from the outside

Callers receive a document snapshot and query it through controlled APIs. Canonical nodes and stores do not expose public mutable fields that bypass validation.

Mutation follows:

```text
Document + Transaction
        ↓
      apply
        ↓
new Document + ChangeSet/Mapping + inverse information
```

### 3.3 Node identity is stable and opaque

`NodeId` is a newtype whose representation is not part of the public semantic contract.

P0 requires:

```text
stable identity across edits that preserve a node
no caller-constructed arbitrary raw IDs through normal APIs
deterministic IDs available to tests
no assumption that NodeId defines document order
```

The first implementation may use a simple allocator. Wire-format identity and distributed ID allocation remain deferred.

### 3.4 The canonical document is a tree

The document root owns a structured node tree. A flat block vector must not become the public long-term model.

Node content categories may include:

```text
children
inline content
atomic/custom payload
```

P0 only needs enough built-in node kinds to exercise the model and transactions. Paragraph and basic container nodes are sufficient for the first implementation slices; later P0 slices may add additional built-in kinds already defined by the top-level plan when they improve invariant coverage.

### 3.5 Text offsets are typed and Unicode-safe

`TextOffset` is an opaque Core coordinate within one text-bearing node or inline text container.

The initial implementation uses UTF-8 byte offsets internally because Rust strings are UTF-8, but construction and mutation APIs must validate character boundaries. A caller must not be able to create an operational text range that points into the middle of a UTF-8 code point through normal safe APIs.

UTF-16 conversion belongs to the future platform adapter and is not part of P0 Core coordinates.

Required fixtures include:

```text
ASCII
Chinese
mixed Chinese/Latin
emoji / surrogate-pair equivalents
combining marks
BiDi samples
```

P0 does not promise grapheme-cluster cursor movement. It does require that byte boundaries and Unicode scalar boundaries are never confused.

### 3.6 Marks are local to text runs

Canonical marks are stored on `TextRun` values rather than in a global range table.

Normalization rules:

```text
adjacent runs with equal MarkSet merge
persistent empty runs are rejected
mark order is canonical
invalid duplicate mark attributes are rejected or normalized
```

TextRun boundaries are an implementation detail. Positions and selections must not expose run segmentation as a semantic coordinate.

### 3.7 Transactions are the only canonical mutation path

P0 introduces typed steps rather than ad hoc mutator methods.

The first transaction surface should cover enough cases to validate text mutation, tree mutation, mapping, and inverse behavior without implementing all P2 editing commands.

Initial step families:

```text
ReplaceText
InsertNode
RemoveNode
SetNodeAttrs
AddMark
RemoveMark
```

`SplitNode`, `JoinNodes`, `MoveNode`, list-specific operations, and InlineAtom operations may be introduced during P0 only when the earlier contracts are stable enough to define their mapping/inverse semantics cleanly. Their interactive behavior remains P2/P4 work.

Every applied transaction returns explicit change information. No subsystem may repair offsets independently after mutation.

### 3.8 Mapping is explicit

Each applied step must produce mapping information sufficient to transform relevant old positions into the new document coordinate space.

P0 mapping requirements begin with text replacement and node insertion/removal.

The mapping API must distinguish a surviving mapped position from a position whose target was deleted. Silent clamping is not the default semantic contract.

Later phases may add recovery/bias policies on top of this explicit result.

### 3.9 Inverse behavior is testable

For reversible P0 operations:

```text
D1 = apply(D0, T)
D2 = apply(D1, inverse(T))
```

`D2` must be semantically equivalent to `D0`, including normalized text/marks and structurally relevant node identity where the operation promises identity preservation.

Inverse generation may be represented as an inverse transaction or a change set that can construct one. The public API should not prematurely freeze that internal representation.

## 4. Initial implementation strategy

P0 favors correctness and observability over premature data-structure optimization.

### 4.1 Text storage

Start with a small `TextBuffer` abstraction backed by `String`.

Reasons:

```text
simple Unicode boundary validation
small dependency surface
clear transaction semantics
rope choice can remain benchmark-driven
```

A future rope must fit behind the same semantic boundary.

### 4.2 Node storage and snapshots

Start with standard-library ownership primitives and node-level structural sharing, for example immutable nodes behind `Arc` and a snapshot-owned store.

The prototype must demonstrate that unchanged node payloads can be shared across document revisions. P0 does not require selecting a permanent HAMT/persistent-vector implementation.

If map cloning becomes the dominant cost in later benchmarks, the store implementation may change without altering the public document contract.

### 4.3 Error model

Invalid operations return typed errors rather than panicking for expected bad input.

Examples:

```text
unknown NodeId
wrong node/content kind
invalid TextOffset boundary
range out of bounds
invalid parent/child relationship
root removal
invalid mark operation
```

Internal invariant violations may use debug assertions, but public safe APIs must report invalid caller input predictably.

## 5. Position and selection surface

P0 establishes the semantic shapes required by later phases without implementing visual caret behavior.

Initial types:

```text
TextPoint
TextSelection
NodeSelection
NodeGap or equivalent structural boundary position
CursorAffinity
```

`TextPoint` includes stable node identity and `TextOffset`.

`CursorAffinity` is retained in the type model so soft-wrap/BiDi frontend behavior does not later require changing the canonical selection contract. P0 does not implement visual affinity resolution.

Cell selection remains deferred to the table phase.

## 6. P0 implementation slices

### P0.0 Phase contract and module skeleton

Deliver:

```text
phase design/progress docs
core module boundaries
public/private visibility policy applied
initial error/result types
```

Gate: workspace CI remains green and no architecture boundary changes are required.

### P0.1 Text boundary

Deliver:

```text
TextBuffer
TextOffset
TextRange
validated slicing/replacement
UTF-8 boundary checks
Unicode regression fixtures
```

Gate: ASCII, Chinese, mixed text, emoji, combining-mark and BiDi boundary tests pass; invalid byte boundaries return errors and never panic.

### P0.2 Document model

Deliver:

```text
DocumentVersion / DocumentRevision
NodeId
Node / NodeKind / NodeAttrs / NodeContent
NodeStore
immutable document snapshot
basic validation
node-level structural-sharing prototype
TextRun / Mark / MarkSet normalization
```

Gate: valid tree construction succeeds; malformed trees are rejected; unchanged node payloads are shared across a simple revision test.

### P0.3 Position and selection

Deliver:

```text
TextPoint
CursorAffinity
TextSelection
NodeSelection
structural boundary position
selection validation against a document
```

Gate: invalid node/range positions are rejected; Unicode fixture positions validate consistently.

### P0.4 Transaction application

Deliver the first typed steps:

```text
ReplaceText
InsertNode
RemoveNode
SetNodeAttrs
AddMark
RemoveMark
Transaction
TransactionOrigin / metadata seam
```

Gate: all mutations preserve document invariants; direct public mutation paths do not exist.

### P0.5 Position mapping

Deliver:

```text
StepMap / ChangeMap prototype
text replacement mapping
node insertion/removal mapping
explicit deleted-target result
transaction mapping composition
```

Gate: old-position to new-position tables cover insertion, deletion, replacement, Chinese/emoji offsets, and removed nodes.

### P0.6 Inverse and randomized invariants

Deliver:

```text
inverse/change-set prototype
transaction round-trip tests
normalized-mark inverse tests
random valid transaction sequences where practical
```

Gate: reversible operation sequences restore semantic original state; random tests do not produce invalid documents or panics.

### P0.7 Contract stabilization

Deliver:

```text
public rustdoc review
architecture.md synchronization
P0 progress evidence complete
unresolved P1 dependencies documented
```

Gate: top-level P0 gate is satisfied and `CI Success` is green.

## 7. Testing strategy

P0 tests should prefer semantic assertions over implementation-shape assertions.

Required categories:

```text
unit tests for boundary/value types
normalization tests
invalid-input tests
transaction result tests
mapping tables
inverse tests
property/randomized tests
regression fixtures
```

Tests must not rely on incidental internal ordering unless ordering is part of the contract.

## 8. Design changes during P0

A small implementation detail can change directly in the P0 branch.

Update this design document when a change alters a P0 contract, slice, or Gate.

Create an ADR when P0 settles a decision that is expensive to reverse or becomes a long-lived public semantic contract, such as a canonical position unit or a fundamental mapping/deletion policy.

`docs/architecture.md` is updated only when the corresponding implementation is actually true.

## 9. P0 completion definition

P0 is complete only when all of the following are true:

```text
versioned structured document model works
snapshots are externally immutable
text boundaries are Unicode-safe
marks normalize deterministically
positions/selections validate against documents
typed transactions preserve invariants
mapping is explicit and composable
inverse prototype passes round-trip tests
Unicode/CJK/emoji/property tests are green
architecture docs match implementation
CI Success is green
```

P1 must not compensate for missing Core invariants with GPUI-specific offset or mutation logic.
