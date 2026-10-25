# P6 Performance — baselines and measured fixes, gate open

Updated 2026-10-25. This batch starts from latest main `67d1f15` (PR #122).
P0–P5 historical gates remain closed. Seven reproducible Runtime and macOS native
workloads now cover long documents, complex tables and independent editors,
including process memory and history. **P6 is not accepted**: large-document
windowing, broader selection/style stress and other platforms' native gates remain.

## Delivered in this batch

- An unpublished performance host with fixed 1k/10k paragraph, 120-KiB CodeBlock,
  merged/header/nested-table and four-editor fixtures. Runtime includes insert,
  4,608-byte paste, undo/redo, logical column edits and independent histories.
- Three isolated release processes per scenario; five warmups and 30 samples;
  raw samples, nearest-rank p50/p95, source/binary hashes, hardware/toolchain,
  initial/steady CPU, RSS checkpoints and OS peak RSS. Native timings include
  actual GPUI/Taffy/text shaping and CPU paint, not GPU completion or event latency.
- Three measured fixes: lazy per-projection font enumeration; local mixed-inline
  selection projection without whole-document scans; bounded uniform multiline
  shaping plus shared immutable native line/carrier arrays. Alignment geometry
  remains independent. No Core API, canonical payload or GPUI dependency upgrade.
- Deterministic regressions for lazy loading, Unicode/reversed/atom-seam selection,
  cross-block/all-selection fallback, stock-GPUI newline/decoration equivalence,
  payload sharing/release, complex fixtures, multi-editor isolation and history.
  The runner tests timeout cleanup and OS RSS units. CI compiles the native host
  on all three platforms without treating virtual tests as native acceptance.

Protocol and commands: [measurement contract](measurement.md).
Raw compressed evidence: [2026-10-25 M3 bundle](baselines/2026-10-25-macos-m3/README.md).

## Local reference measurements

Apple M3, 24 GiB, macOS 26.1 (25B78), Rust 1.97.1, release, 2026-10-25.
The tables show the median of three per-run p50 values and **largest per-run p95**;
peak RSS is the largest of three processes. Initial render has one sample/process
and is summarized by its median. These are local observations, not cross-platform
SLAs. Operating-system font/file caches were not flushed.

Runtime, without the native feature (undo/redo columns refer to single insertion):

| Scenario | Insert p50/p95 ms | Paste p50/p95 ms | Undo p50/p95 ms | Redo p50/p95 ms | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| long-1k | 0.20 / 0.31 | 0.19 / 0.31 | 0.10 / 0.15 | 0.10 / 0.15 | 4.4 |
| long-10k | 3.07 / 4.32 | 3.08 / 4.35 | 1.55 / 2.46 | 1.54 / 2.14 | 10.2 |
| code-128k | 0.02 / 0.02 | 0.02 / 0.02 | 0.01 / 0.01 | 0.01 / 0.01 | 4.7 |
| table-20x20 | 0.18 / 0.34 | 0.18 / 0.21 | 0.09 / 0.13 | 0.09 / 0.12 | 5.2 |
| table-50x20 | 0.57 / 0.69 | 0.52 / 0.60 | 0.27 / 0.35 | 0.27 / 0.35 | 6.5 |
| multi-4x1k | 0.21 / 0.23 | 0.21 / 0.23 | 0.11 / 0.11 | 0.11 / 0.12 | 6.1 |
| multi-4x10k | 3.08 / 3.89 | 3.10 / 3.86 | 1.54 / 1.93 | 1.54 / 1.92 | 12.8 |

Runtime grouped-history checkpoints below are medians across three processes.
Every editor separately reached 1 grouped undo unit or 256 isolated paste units;
both forms were undone and redone with exact text/store assertions. RSS includes
allocator-retained pages from preceding cycles, so its delta is not a measurement
of retained history payload bytes. Quarter/full/undo/redo/release and isolated
checkpoints remain in the raw JSON.

| Scenario | Empty history RSS MiB | 256 grouped edits RSS MiB | Grouped undo units/editor | Isolated undo units/editor |
| --- | ---: | ---: | ---: | ---: |
| long-1k | 3.22 | 3.88 | 1 | 256 |
| long-10k | 8.97 | 9.64 | 1 | 256 |
| code-128k | 3.31 | 4.16 | 1 | 256 |
| table-20x20 | 3.59 | 4.33 | 1 | 256 |
| table-50x20 | 4.92 | 5.56 | 1 | 256 |
| multi-4x1k | 3.38 | 4.66 | 1 | 256 |
| multi-4x10k | 10.03 | 11.45 | 1 | 256 |

Native window: 1280×800 logical bounds, actual 1280×765 content viewport, scale 2,
16px inherited font, measured-table mode enabled. Each input command edits one
pane round-robin, followed by an explicitly requested full redraw of all panes.
The enclosing CPU pipeline includes nested request-layout, prepaint and paint;
do not add their times to this enclosing interval. All 21 runs verified view
release and completed the idle checkpoint. Redraw watchdog wakeups were zero.

| Scenario | Initial median ms | Redraw p50/p95 ms | Input frame p50/p95 ms | Command p50/p95 ms | Peak RSS MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| long-1k | 285.99 | 14.49 / 21.84 | 16.32 / 18.93 | 0.64 / 0.79 | 136.4 |
| long-10k | 2797.32 | 177.04 / 187.51 | 202.16 / 218.07 | 7.32 / 7.62 | 604.1 |
| code-128k | 17.60 | 5.04 / 5.76 | 8.41 / 11.59 | 0.07 / 0.10 | 130.9 |
| table-20x20 | 43.23 | 10.98 / 17.64 | 11.03 / 13.80 | 0.66 / 0.78 | 121.4 |
| table-50x20 | 103.53 | 30.59 / 32.79 | 31.51 / 33.60 | 1.60 / 1.78 | 169.7 |
| multi-4x1k | 434.28 | 75.35 / 101.13 | 82.34 / 111.43 | 0.87 / 1.60 | 298.4 |
| multi-4x10k | 4212.70 | 1305.24 / 3788.79 | 1112.81 / 2467.20 | 10.62 / 21.42 | 1820.5 |

The four-10k case remains very slow and noisy. Its per-run instruction counts
are about 764 billion versus about 778 billion in the intermediate implementation,
but elapsed CPU time increased and involuntary context switches rose sharply.
This does **not** establish a multi-editor latency improvement. Keep the measured
slow samples; repeat on a controlled foreground/power/scheduling setup while
profiling the remaining full-tree work. No outlier was silently removed.

## Why these changes were made

1. A one-second stack sample of the original 1k native workload spent nearly all
   sampled main-thread time in `all_font_names` / CoreText enumeration. That process
   failed to finish within 90 seconds. Ordinary inherited-font text now avoids
   enumeration; explicit font resolution remains lazy and scoped to one projection
   so a later projection can observe host-added fonts.
2. After the font fix, the 10k stack showed per-paragraph `Slots::walk`, selection
   ordering and text-block collection. A same-block selection needs only its own
   display mapping. Cross-block, structural and virtual-input cases keep their
   established path; this is not a global selection-cache or windowing claim.
3. The long CodeBlock still peaked around **893–895 MiB** after those CPU fixes.
   Heap/VM tools distinguished about 69 MiB of allocated heap from about 747 MiB
   of empty resident MALLOC_LARGE pages. Full allocation logging located large
   geometrically grown GPUI `WrappedLine` arrays and layout clones (13.94 GB of
   cumulative allocations in that instrumented run). Uniform multiline text now
   shapes each logical line through the same pinned GPUI API into an exactly
   reserved output, and layouts share read-only line/carrier arrays. Final native
   code runs peak around **131 MiB**, roughly **85% lower**. A three-run one-second
   idle comparison corroborates the result; no allocator purge or unsafe code is
   used. This fixes allocation churn, not a proven leaked-entity bug.

## Validation and acceptance record

Local validation and three-platform PR CI completed on 2026-10-25. Native performance evidence here is
macOS only. The existing reviewed git2 test-dependency advisories remain tracked
in [dependency disposition](../../dependency-advisories.md), independently of P6.

- Workspace all-target tests: **1,877 passed**, including 582 GPUI unit tests and
  three performance-fixture/history tests; workspace doctests: **6 passed**.
- Strict Clippy includes `xiaomu-perf-harness/native`; formatting, source-size,
  dependency-boundary, XIM/Taffy provenance and reviewed advisory-scope guards pass.
- Measurement-runner regression tests: **3 passed** on macOS.
- Explicit Taffy min-content integration: **4 passed**; original XIM decoder:
  **8 passed**; published Taffy suite: **89 tests + 5 doctests passed**.
- `cargo deny check bans licenses sources`: passed. Existing advisory findings
  are recorded separately and are not represented as resolved by this batch.
- [PR #123](https://github.com/prtitrz/Xiaomu/pull/123), implementation commit
  `e7053ffe7b46ccac3b5e13cc204df48c3ba9ccfd`:
  [CI run 40171729276](https://github.com/prtitrz/Xiaomu/actions/runs/40171729276)
  passed policy, Linux, macOS, Windows and the aggregate **CI Success** check.
  Native-host compilation and runner checks passed on all three platforms
  (the POSIX process-tree test is explicitly skipped on Windows).
  This record is a documentation-only follow-up to that verified implementation;
  PR status remains the source of truth for checks on the latest branch tip.
- Interactive Linux/Windows native performance acceptance remains open regardless
  of automated CI status. This batch is submitted for review; it does not mark
  P6 accepted or claim that PR #123 is already merged into main.

## Remaining work, in priority order

1. **P1 — large-document windowing and remaining full-tree work.** One 10k pane
   still spends about 177–202 ms on a full redraw/input frame; four panes remain
   far beyond an interactive frame budget. Profile per-child synchronization,
   parent/child lookups and layout before introducing revision-scoped caches.
   Windowing must pass off-screen selection/IME/focus, scrolling/hit-testing,
   tables, undo and accessibility gates before it becomes the default.
2. **P1 — broaden performance and memory acceptance.** Add cross-block/all/cell
   selections, long text with multiple styled/font runs, long-lived edit/undo/drop
   cycles and unrelated multi-editor documents. The new uniform-line path does
   not optimize every mixed-size or multi-run case. Native view-release checks,
   one-second idle RSS and short fresh processes do not certify absence of leaks.
3. **P1 — Linux/Windows native measurements.** Run the same release host on an
   interactive desktop, record backend/fonts/scale/hardware and compare raw data.
   Their compilation and correctness CI are necessary but do not close this gate.
4. **P2 — host event-to-photon and sustained scrolling/resize.** Measure event
   dispatch, IME candidate updates, GPU completion and paced user-like input
   separately from this CPU-only burst workload; agree on budgets per target host.

## Earlier child-reuse kernel baseline (PR #122)

`DocumentView::sync_children` previously searched/removes from a Vec for every
node, moving the tail even in unchanged document order. `child_pool::reconcile`
now indexes existing children by NodeId and emits in document order: expected
O(n) time and O(n) temporary space. Deleted entities are dropped, new nodes alone
create views, and retained nodes keep their native entities and focus handles.
This does not virtualize paragraphs or eliminate per-child configuration updates.

The benchmark in `xiaomu-testkit/benches/child_reconcile.rs` compiles the same
private reconciliation helper without pulling GPUI into the measured kernel.
It compares the old Vec algorithm with the current helper, in stable and reversed
order. IDs/fixtures are prepared outside the timer; HashMap construction, removal
and output allocation are inside. Five warmups precede 30 samples; p50/p95 use
nearest-rank indices. Results include synthetic `usize` children, not native
entities, text layout, paint or user input.

Reproduce with the pinned toolchain:

```sh
cargo bench --locked -p xiaomu-testkit --bench child_reconcile
```

Local sample: Apple M3, 24 GiB, macOS 26.1 (25B78), Rust 1.97.1, optimized bench
profile, 2026-10-25. Source base `7015166` plus this maintenance batch.

| Algorithm/order | Blocks | p50 µs | p95 µs |
| --- | ---: | ---: | ---: |
| Old Vec / stable | 1,000 | 90.417 | 95.250 |
| NodeId map / stable | 1,000 | 19.083 | 20.083 |
| Old Vec / reverse | 1,000 | 144.917 | 281.042 |
| NodeId map / reverse | 1,000 | 19.208 | 19.834 |
| Old Vec / stable | 10,000 | 11,103.500 | 11,898.916 |
| NodeId map / stable | 10,000 | 249.708 | 374.625 |
| Old Vec / reverse | 10,000 | 14,423.167 | 15,300.750 |
| NodeId map / reverse | 10,000 | 236.125 | 517.417 |

No absolute timing assertion is placed in shared CI. Permanent behavior tests
cover identity/order, creation/deletion and native focus-handle retention.
