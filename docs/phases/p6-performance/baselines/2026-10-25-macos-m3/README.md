# 2026-10-25 Apple M3 baseline evidence

Base: main `67d1f155183044ac326180ff6dd7a3ed293f4da0` plus the measured working
changes. Each JSON records its source-tree fingerprint, binary SHA256, Rust
version, hardware, OS, run parameters, raw per-operation samples, RSS checkpoints
and time(1) output. Those identifiers describe the actual measurement, not a
claim that an uncommitted measurement already had the later PR commit hash.

- `runtime.json.gz`: seven scenarios, three fresh processes each, release binary
  built **without** the optional native feature; 30 samples/editor after five
  warmups, 256 grouped insertions and 256 isolated paste undo entries/editor.
- `native.json.gz`: final native implementation, same seven scenarios and three
  fresh processes each, 30 redraws and 30 round-robin input frames after warmup;
  initial render, view release and idle RSS recorded separately.
- `native-before-memory-fix.json.gz`: three code-only runs with the same 30-frame
  workload, after the font/selection fixes but before bounded uniform-line
  shaping and immutable layout sharing. Supports the memory-fix comparison.
- `font-enumeration.sample.txt.gz`: one-second native stack sample before the
  lazy font catalog fix. This diagnostic was taken during a 1k process that
  did not finish within its 90-second limit; it is not a percentile baseline.
- `selection-scan.sample.txt.gz`: one-second 10k native stack sample after the
  font fix and before local-selection projection. The run was interrupted
  after profiling; no successful whole-run timing is claimed for it.
- `code-heap-before.txt.gz`, `code-vm-before.txt.gz`: post-release allocation/VM
  diagnostics. Roughly 69 MiB of allocated heap and 747 MiB of empty resident
  MALLOC_LARGE pages distinguish live objects from allocator retention.
- `code-allocation-peak-before.txt.gz`: separate MallocStackLogging=full run,
  30 code-input frames. Its 13.94 GB cumulative allocations and native shaping /
  layout clone stacks locate transient allocation churn. Instrumentation changes
  memory and timing; this file is **not** used as a latency/peak-RSS benchmark.

All compressed files are unmodified tool output (gzip compression only).
For example, inspect with `gzip -dc runtime.json.gz` or Python's `gzip.open`.
The nominal scenario key `code-128k` contains **122,880 UTF-8 bytes (120 KiB)**,
4,096 LF-terminated source lines; the final LF also creates an empty display line.
The `paste_4k` operation inserts **4,608 bytes**, recorded in Runtime JSON.
Window bounds are 1280×800; the measured content viewport on this macOS run is
1280×765 at scale 2. OS/font caches are not flushed between fresh processes.

These files support local CPU/memory conclusions only. They do not constitute
Windows/Linux native acceptance, long-duration leak certification or measured
GPU/input-to-photon latency. See [measurement contract](../../measurement.md)
and [P6 progress](../../progress.md) for interpretation and remaining gates.
