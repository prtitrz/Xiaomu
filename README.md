# 晓木 Xiaomu

**A native structured rich-text editor engine for Rust.**

晓木是一个面向 Rust 原生应用的结构化富文本 / Block Editor engine。项目将文档语义、编辑事务、运行时编排与具体 UI 框架分层，首个原生前端基于 GPUI。

> Status (2026-10-25): P0–P5 closed; P6 has started a child-reuse baseline slice, with native performance and memory gates still pending. Xiaomu remains an early-stage library, not a finished writing application. See the [documentation index](docs/README.md) and [delivery boundaries](docs/planning.md#delivery-boundaries).

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
  README.md               documentation index and reading order
  planning.md             top-level architecture and roadmap
  architecture.md         architecture that is currently true
  engineering-rules.md    repository engineering constraints
```

## Roadmap

P0–P5 have delivered the core editing model, native input, structured content and tables within their recorded acceptance scope. The current next phase is P6: establish reproducible long-document, table and multi-editor performance baselines before choosing optimizations.

The [top-level plan](docs/planning.md) is the single execution roadmap. Historical gap reviews are audit records, not a second work queue. The image-import slice on main adds PNG/JPEG clipboard pixels through a host import service, with durable sidecar assets in the harness and Undo/Redo/save/reopen tests. Native platform acceptance remains separate; see [delivery boundaries and unscheduled editing work](docs/planning.md#delivery-boundaries).

## Linux embedding note

Stock Linux input uses a narrowly patched, explicitly vendored `xim-ctext` decoder based on an identified official Git revision. Embedding applications must use that audited copy and repeat the root Cargo override; it is not inherited from Xiaomu as a dependency. See [the decoder and native-test boundaries](docs/linux-xim-decoder.md). This does not replace GPUI or add a custom input-owner/wait protocol.

## Embedding dependency patches

The workspace also carries a one-line lazy min-content backport in Taffy 0.9.0,
the exact version required by stock GPUI 0.2.2. The official crate manifests,
features and API are unchanged; [provenance and verification](vendor/taffy/LOCAL_PATCHES.md)
describe the source import and regression. A host must repeat both overrides in
its own workspace-root manifest because Cargo does not inherit dependency patches:

```toml
[patch.crates-io]
xim-ctext = { path = "path/to/Xiaomu/vendor/xim-ctext" }
taffy = { path = "path/to/Xiaomu/vendor/taffy" }
```

Point these paths at the reviewed Xiaomu revision and verify the host's resolved
dependency graph and lockfile. This avoids redundant intrinsic measurements in
the tested library cases; a native host speedup requires separate measurement.

## Development

Structured clipboard compatibility: valid v4/v5/v6 payloads remain supported; explicit null node attributes use conditional v7. Exact five-field link attributes use conditional v8 when the classic href/title form is insufficient; ordinary links keep older versions. TextStyle marks preserve color/fontFamily/fontSize missing/null/string states and use conditional v9; documents without TextStyle keep the v4–v8 feature choice. Data preservation does not imply renderer support for every CSS string. Decoding rejects unknown structural wire fields, wrong types and duplicate JSON keys rather than silently dropping data. Canonical attribute/object map keys remain extensible and preserved. See [null attributes](docs/adr/0006-nullable-node-attrs.md), [exact link attributes](docs/adr/0007-exact-link-attributes.md) and [text-style values](docs/adr/0008-exact-text-style.md).

Engineering rules live in [docs/engineering-rules.md](docs/engineering-rules.md). Current architecture facts live in [docs/architecture.md](docs/architecture.md). Contribution workflow is documented in [CONTRIBUTING.md](CONTRIBUTING.md).

GPUI hosts can opt into [native mixed font sizes](docs/mixed-font-size.md) with a fixed style provider and the same final-candidate admission policy. Uniform text with zero inset retains stock shaping; mixed text has explicit script, cluster and work-budget limits. The optional [first-line inset provider](docs/first-line-inset.md) shares this admission contract for genuine first-row reflow, including uniform paragraphs. Native font and IME acceptance remains separate from virtual tests.

Embedded readers can opt into [read-only user input](docs/read-only-input.md)
without changing default editable behavior. Native preedit and clipboard/import
side effects are guarded before they begin; host updates still require their
independent session policy.

Hosts can also opt into [bounded clipboard leaf projection](docs/clipboard-leaf-projection.md):
exact extension atom kinds emit LF while rich payload and reading coordinates
remain unchanged. Only that descriptor uses framed v15; default V1/v14 behavior
is preserved. The contract documents the small `Clone`/`const` API migration.

The main local gates are:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
python tools/check_source_size.py
python tools/check_dependency_boundaries.py
python tools/check_xim_decoder_source.py
python tools/check_taffy_source.py
```

## License

Apache-2.0.
