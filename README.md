# 晓木 Xiaomu

**A native structured rich-text editor engine for Rust.**

晓木是一个面向 Rust 原生应用的结构化富文本 / Block Editor engine。项目将文档语义、编辑事务、运行时编排与具体 UI 框架分层，首个原生前端基于 GPUI。

> Status: early architecture / bootstrap stage.

## Goals

- Versioned structured document model
- Unicode-correct text boundaries and selections
- Typed transaction and position-mapping engine
- Native IME, caret, selection and clipboard behavior
- Structured blocks, inline atoms and tables
- Extensible rendering and command boundaries
- Host-neutral embedding API
- GPUI as the first native frontend, without coupling Core to GPUI

## Architecture

```text
                 ┌─ Markdown codec
                 ├─ future codecs
XiaomuDocument ──┤
       ↓         └─ host adapters
Transaction Engine
       ↓
DocumentSession
       ↓
Frontend boundary
       ↓
GPUI Native Surface
       ↓
Host application
```

Dependency direction:

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

Codecs depend on `xiaomu-core`; Core never depends on a codec or UI framework.

## Design principles

1. Document semantics are independent of serialization formats.
2. Editing operations are typed transactions over a stable document model.
3. UI-framework APIs do not enter Core.
4. Host applications own persistence, networking, assets and product lifecycle.
5. Downstream integration requirements are served through adapters and capabilities, not product-specific branches inside Xiaomu.
6. When host convenience conflicts with Xiaomu's long-term correctness or extensibility, Xiaomu's architecture takes precedence and the host adapts at its boundary.

## Workspace

```text
crates/
  xiaomu-core/            document, text, selection, transaction, history
  xiaomu-runtime/         session, commands, extension/runtime orchestration
  xiaomu-gpui/            native GPUI input, layout, paint, focus, clipboard
  xiaomu-codec-markdown/  Markdown import/export
  xiaomu-testkit/         fixtures, property tests and interaction helpers
examples/
  editor_harness/         standalone integration harness
docs/
  planning.md             top-level architecture and roadmap
  architecture.md         architecture that is currently true
  engineering-rules.md    repository engineering constraints
```

## Roadmap

The first hard gates are Unicode/IME correctness, transaction semantics, position mapping, multi-block editing and history. Tables, richer extensions and performance work follow only after those foundations are stable.

See [docs/planning.md](docs/planning.md).

## Development

Engineering rules live in [docs/engineering-rules.md](docs/engineering-rules.md). Current architecture facts live in [docs/architecture.md](docs/architecture.md). Contribution workflow is documented in [CONTRIBUTING.md](CONTRIBUTING.md).

The main local gates are:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
python tools/check_source_size.py
python tools/check_dependency_boundaries.py
```

## License

Apache-2.0.
