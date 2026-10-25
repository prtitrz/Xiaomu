# P6 measurement contract

The unpublished `xiaomu-perf-harness` example owns workloads and measurement.
Production crates do not depend on it. GPUI remains exactly 0.2.2. Run optimized
binaries with a quiet machine and record the actual OS/toolchain; shared CI checks
semantics and compilation, not machine-specific millisecond limits.

## Reproduction

```sh
cargo build --locked --release -p xiaomu-perf-harness --bin xiaomu-perf-baseline
python3 tools/run_p6_baselines.py --binary target/release/xiaomu-perf-baseline --output /tmp/p6-runtime.json
cargo build --locked --release -p xiaomu-perf-harness --features native --bin xiaomu-native-perf
python3 tools/run_p6_baselines.py --kind native --binary target/release/xiaomu-native-perf --output /tmp/p6-native.json
```

On Windows append `.exe` to binary paths and use `python`. Native runs require an
interactive desktop and open a temporary measurement window with generated data.
The driver runs each scenario in a fresh process (OS/font caches are not flushed),
three repetitions by default,
saves after every run, and stops on a failure. Its timeout kills the measurement
process tree. A failed/partial run is evidence of a failure, never a zero timing.
Builds must finish before recording timings. Build the Runtime binary separately
without `--features native` when recording its standalone process memory. Do not rebuild the binary during a
measurement batch. Preserve both raw JSON and the source/binary identifiers.

## Fixed scenarios

| Name | Content | Independent editors |
| --- | --- | ---: |
| long-1k / long-10k | 1,000 / 10,000 paragraphs; Chinese, combining mark, emoji | 1 |
| code-128k | One CodeBlock with 4,096 LF-terminated lines, 122,880 bytes (120 KiB; nominal key retained) | 1 |
| table-20x20 / table-50x20 | Logical 20×20 / 50×20 outer table, headers, rich text, horizontal and vertical spans, nested tables every 10 rows | 1 |
| multi-4x1k / multi-4x10k | Same long document initially shared as an immutable snapshot; independent selection and history, edits trigger COW | 4 |

The target is the last editable block. Multi-editor timings are individual editor
operations, not concurrent throughput. This shared-initial-snapshot case does
not cover four unrelated loaded documents. Native table layout is explicitly
opted into measured tables; the benchmark must not time a fallback placeholder.

## Runtime and memory

Five warmup edit cycles precede 30 recorded cycles per editor. Record fixture
construction and session mount separately; then InsertText, a fixed 4,608-byte PasteText
payload, undo, redo, and logical column insertion/undo for the complex tables.
Exact node-store restoration and selection are checked outside timers. There is
no Markdown parsing or platform clipboard I/O in PasteText's timing.

A separate series records 256 adjacent insertions per editor, grouped as one undo
unit, and another series retains 256 isolated paste undo units. Both undo/redo
paths check restored text/store and history depth. The JSON keeps every latency
sample and nearest-rank p50/p95 rather than discarding slow samples.

RSS is sampled outside operation timers: Linux `/proc/self/status`, macOS `ps`,
Windows WorkingSet64. OS peak RSS comes from `/usr/bin/time -l` on macOS (bytes),
`time -v` on Linux (KiB converted to bytes); unavailable readings remain null.
Working-set/RSS includes runtime/allocator pages and is not retained canonical
payload size. Pages can remain resident after owners are dropped. A plateau or
failure to return to initial RSS alone neither proves nor disproves a leak.
Native runs additionally verify released view entities no longer upgrade, then
sample again after one idle second. For allocation tools only, set
`XIAOMU_PERF_IDLE_MS` between 1,000 and 30,000; the result records this delay.
Instrumented heap/VM profiles are separate diagnostics, never timing samples.

## Native timing boundaries

Use a 1280×800 logical-pixel window, 16px text, the platform's GPUI default font,
and recorded scale factor. Multi-editor panes share that width equally. Record
initial construction/render separately, five warmup redraws, steady forced full
redraws, and consecutive InsertText commands applied through DocumentView's public
host-command path, round-robin across independent editors. The final text and
history depth are asserted separately for every editor.

The wrapper records request-layout CPU, prepaint CPU, paint CPU and the enclosing
request-layout-start → paint-end interval. The enclosing interval includes
Taffy's layout work between callbacks and native text shaping; the component
measurements are nested, so **do not add them to the enclosing interval**. The
command CPU duration includes Runtime application and view synchronization before
the next painted frame. These numbers do not measure GPU completion, vsync,
platform event dispatch, IME candidate latency, or user-perceived input-to-photon
latency. Each stage's sample count is retained for audit. A 250ms watchdog re-requests
only a stalled redraw without advancing the scenario; its wakeup count is
recorded. Waiting between frames is outside CPU intervals and must not be
reported as measured input latency.

## Decision gates

First compare growth between 1k and 10k, plus repeatability across three fresh
processes. Investigate repeated full-tree scans before adding more caches. Any
cache must be revision-scoped or rebuilt with its owner, retain identity/focus,
and invalidate after structural edits and undo. CI regressions assert these
properties rather than elapsed time. Record memory cost alongside latency gains.

A 16.7ms frame budget is a diagnostic reference for a 60Hz host, not an accepted
cross-platform SLA. Sustained layout/paint over it remains a P6 issue even after
an optimization. Windowing needs separate selection, off-screen IME/focus,
scroll/hit-test, table geometry and accessibility correctness gates. Linux and
Windows native performance runs remain distinct from their automated CI tests.
