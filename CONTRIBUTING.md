# Contributing to Xiaomu

Xiaomu is an early-stage Rust native structured rich-text editor engine. The project favors correctness, explicit architecture boundaries, small public APIs, and regression-driven testing over feature breadth.

Before contributing, read:

- `README.md`
- `docs/planning.md`
- `docs/engineering-rules.md`
- `docs/architecture.md`

## Development baseline

The workspace uses the pinned toolchain in `rust-toolchain.toml`.

Before opening a pull request, run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
python tools/check_source_size.py
python tools/check_dependency_boundaries.py
```

Dependency policy is checked in CI with `cargo-deny`.

## Change scope

Keep each pull request focused on one coherent goal. Refactors that change architecture should not be bundled with unrelated feature work.

Bug fixes must include a regression test whenever the bug can be reproduced deterministically. Unicode, selection, transaction, mapping, history, and IME regressions should remain in the permanent test matrix after they are fixed.

## Architecture boundaries

The dependency direction is:

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

Codecs depend on `xiaomu-core`. Production crates must not depend on `xiaomu-testkit`.

Do not introduce host-specific concepts into `xiaomu-core`. Do not introduce GPUI types into `xiaomu-core` or `xiaomu-runtime`.

The CI dependency-boundary check enforces the crate-level rules. Module-level design still requires review.

## Public API

Default to private visibility.

A symbol should become `pub` only when another crate or a downstream user needs it. Public document-model types should protect invariants through constructors, getters, query APIs, and typed transactions rather than exposing mutable fields.

Public API changes during the `0.x` phase may be breaking, but they must still be deliberate and documented. Public items require useful rustdoc describing semantics, invariants, units, and failure behavior where relevant.

## Documentation

The documentation layers have different purposes:

- `docs/planning.md` describes intended direction and roadmap.
- `docs/architecture.md` records architecture that is currently true.
- `docs/adr/` records decisions whose rationale should survive implementation changes.

If a code change makes `docs/architecture.md` inaccurate, update that document in the same pull request.

Use an ADR only for decisions that are expensive to reverse, constrain future architecture, or are likely to make a future contributor ask why the project chose a particular path.

## Dependencies

Keep `xiaomu-core` especially small and dependency-light.

A new dependency should justify the capability it provides, its maintenance quality, its license, and why the same result should not be implemented with a small amount of local code. Avoid wildcard dependency versions and unnecessary default features.

## Unsafe code

`xiaomu-core` and `xiaomu-runtime` forbid unsafe code.

If a future frontend or performance-critical module genuinely requires `unsafe`, isolate it behind a narrow safe interface, document every unsafe block with a `SAFETY:` explanation, and add targeted tests for the invariant being relied upon.

## Commits and pull requests

Use concise Conventional Commit-style subjects when practical:

```text
feat:
fix:
refactor:
test:
docs:
perf:
chore:
```

`main` should remain buildable. Large changes should land through a branch and pull request so CI and review can validate them before merge.
