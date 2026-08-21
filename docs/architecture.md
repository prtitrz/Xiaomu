# Xiaomu Architecture

This document records architecture that is currently true in the repository. Future intentions belong in `planning.md`; major design rationale belongs in `adr/`.

## Workspace boundary

The workspace is split into five library crates and one example harness:

```text
xiaomu-core
xiaomu-runtime
xiaomu-gpui
xiaomu-codec-markdown
xiaomu-testkit
examples/editor_harness
```

The intended production dependency direction is already treated as a repository invariant:

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

`xiaomu-codec-markdown` depends only on the canonical Core model. `xiaomu-testkit` exists for test/support code and must not become a production dependency.

At the current bootstrap stage, the crates expose only minimal marker APIs. Their internal module trees have not yet been implemented.

## Core boundary

`xiaomu-core` is the home of document semantics and must not depend on a UI framework, host application, persistence layer, network layer, or codec.

The Core contract being implemented from P0 includes:

```text
versioned document model
text boundaries
positions and selections
typed transactions
position mapping
history primitives
commands and structural invariants
```

Core forbids unsafe code.

## Runtime boundary

`xiaomu-runtime` coordinates editing sessions and command execution around Core types. It may depend on `xiaomu-core` but not on GPUI.

Runtime is not an application shell. Persistence, file lifecycle, networking, product configuration, and window ownership remain host responsibilities.

Runtime forbids unsafe code.

## GPUI boundary

`xiaomu-gpui` is the first native frontend implementation. GPUI-specific input, focus, layout, paint, hit testing, clipboard integration, and virtualization belong here.

GPUI platform types must not leak into Core or Runtime public contracts.

The GPUI dependency itself has not yet been introduced. When it is added, it will be pinned to an explicit revision as described in `planning.md`.

## Codec boundary

`xiaomu-codec-markdown` is an import/export boundary. Markdown is not the canonical editing state and Markdown source offsets are not document positions.

Future codecs should follow the same direction:

```text
external format
      ↕
codec crate
      ↕
xiaomu-core document model
```

Core never depends on a codec.

## Host boundary

Hosts integrate through public APIs, adapters, capability services, and extension seams. Host-specific business models must remain outside Xiaomu's canonical document semantics unless a concept proves generally useful to editor users.

When host convenience conflicts with the long-term correctness or extensibility of Xiaomu, the host adapts at its boundary.

## Repository enforcement

Architecture is reinforced by:

- `tools/check_dependency_boundaries.py` for crate-level dependency direction;
- `tools/check_source_size.py` for source-file growth guardrails;
- Rust formatting, Clippy, and tests in CI;
- `cargo-deny` for dependency source and license policy;
- documentation synchronization rules in `engineering-rules.md`.

This document must be updated in the same pull request whenever implementation makes any statement above inaccurate.
