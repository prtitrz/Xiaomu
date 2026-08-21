# Xiaomu Engineering Rules

This document defines the engineering rules that should remain true as Xiaomu grows. The rules are intentionally small in number and biased toward checks that prevent expensive architectural drift.

## 1. Dependency direction

The production dependency direction is fixed:

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

Additional rules:

```text
xiaomu-codec-*  → xiaomu-core
xiaomu-testkit  → test/support layers only
examples        → public Xiaomu crates
```

Forbidden examples:

```text
xiaomu-core    → xiaomu-runtime
xiaomu-core    → xiaomu-gpui
xiaomu-core    → codec
xiaomu-runtime → xiaomu-gpui
production     → xiaomu-testkit
```

`tools/check_dependency_boundaries.py` enforces crate-level boundaries in CI. It does not replace architecture review for module-level coupling.

## 2. File size guardrail

Rust source files should stay small enough that one file has one recognizable responsibility.

```text
<= 500 lines   normal
501-700        warning; review whether responsibilities should split
> 700          CI failure by default
```

The check applies to Rust source under `crates/` and `examples/`.

Dedicated `tests/`, `benches/`, `fixtures/`, generated output, and vendored code are excluded. Inline tests inside production source still count because a source file that becomes hard to navigate should be split regardless of whether the growth comes from implementation or tests.

Generated Rust may opt out only when it is clearly marked with `@generated` near the top of the file.

Line count is a guardrail, not a design target. A 450-line file with mixed responsibilities should still be split.

## 3. Public API discipline

Default visibility is private.

Use `pub(crate)` for crate-internal sharing and `pub` only for intentional cross-crate or downstream API.

Public document-model state must preserve invariants. Canonical structures such as document nodes, selections, positions, transactions, and mappings should avoid public mutable fields when mutation could bypass validation.

During `0.x`, breaking changes are allowed, but accidental public surface area is still treated as a defect.

Every public item should have useful rustdoc. Documentation should explain semantics that types alone cannot express, especially:

```text
offset units
Unicode boundary guarantees
selection affinity
transaction invariants
mapping behavior
error conditions
ownership/lifetime expectations
```

Once crates are published and API shape becomes meaningful, add `cargo-semver-checks` to CI before declaring SemVer stability.

## 4. Document truth and design records

Documentation has three layers.

### `docs/planning.md`

Describes intended architecture, stage gates, and roadmap. It may contain future work.

### `docs/architecture.md`

Describes architecture that is currently true in the repository. If implementation changes make this document false, update it in the same pull request.

### `docs/adr/`

Records decisions whose rationale should survive code movement or refactoring.

Write an ADR when a decision:

- is expensive to reverse;
- constrains multiple crates or future extensions;
- establishes a canonical representation or semantic contract;
- rejects a plausible alternative that future contributors are likely to reconsider.

Do not write an ADR for routine refactors, naming changes, or easily reversible implementation details.

## 5. Testing rules

Xiaomu prioritizes invariant and regression coverage over a numeric line-coverage target.

Required patterns:

```text
bug fix              → regression test
Core invariant       → invariant/property test where practical
Unicode bug          → permanent Unicode regression fixture
transaction          → resulting document + mapping assertions
reversible operation → inverse/undo assertion
mapping change       → old position → expected new position
IME behavior         → interaction harness / real-platform gate
```

A deterministic bug in Unicode, selection, mapping, history, or transaction logic should never be fixed without leaving behind a test that would have caught it.

Randomized transaction sequences and fuzzing should be introduced once the P0 model is expressive enough to generate valid operations.

No hard code-coverage percentage is required at this stage.

## 6. Unsafe policy

`xiaomu-core` and `xiaomu-runtime` use:

```rust
#![forbid(unsafe_code)]
```

That rule should remain unless there is a demonstrated requirement that cannot be solved at a lower layer.

Frontend or performance-critical code may use `unsafe` only when all of the following are true:

1. the unsafe code is isolated behind a narrow safe API;
2. every unsafe block has a `SAFETY:` comment explaining the invariant;
3. targeted tests exercise the invariant;
4. the pull request explains why a safe alternative is inadequate.

## 7. Dependency policy

Dependencies are part of Xiaomu's long-term maintenance surface.

Before adding a crate, consider:

```text
capability gained
maintenance activity
license
MSRV impact
binary/build impact
transitive dependency cost
whether default features are actually needed
whether a small local implementation is clearer
```

Rules:

- avoid wildcard versions;
- keep `xiaomu-core` dependency-light;
- disable unnecessary default features;
- avoid adding a large utility dependency for a very small helper;
- use crates.io or explicitly reviewed Git sources only;
- pin GPUI revisions according to `docs/planning.md` once the GPUI dependency is introduced.

`cargo-deny` checks licenses, bans, and dependency sources. Advisory checks are visible in CI but should not make unrelated source changes suddenly unmergeable without maintainer review.

## 8. Formatting, linting, and warnings

CI requires:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Do not silence a Clippy warning globally merely to make CI green. Prefer fixing the code or adding a narrowly scoped allow with a reason when the lint is genuinely inappropriate.

## 9. Architecture before convenience

Host integration is an explicit quality goal, but downstream convenience does not justify product-specific branches inside the canonical model.

If a host needs behavior that does not generalize cleanly:

```text
first choice   adapter
second choice  capability service
third choice   extension point
last resort    core change, only if the concept is generally valid
```

Public extension seams should be driven by concrete use cases. Avoid speculative plugin systems before the underlying editing semantics are stable.

## 10. Change discipline

A pull request should have one coherent purpose.

Large architectural changes should include an ADR or design-document update before or with the code. Mechanical refactors should avoid changing semantics at the same time unless the coupling makes separation impractical.

`main` should remain buildable and pass required CI gates.

## 11. Rules intentionally deferred

The project does not currently require:

```text
hard code-coverage percentage
per-phase time estimates
CLA
CODEOWNERS
formal RFC process
complex release automation
mandatory ADR for every design change
```

These should be introduced only when project scale creates a concrete need.
