# P6 Performance — baseline in progress

Updated 2026-10-25. P0–P5 historical gates remain closed. P6 has begun with a
repeatable child-reuse CPU baseline and a targeted quadratic-cost fix; it has
**not** passed the long-document/native performance gate.

## Delivered baseline slice

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

## Remaining P6 gate

Execution order is maintained in [planning](../../planning.md#p6--performance--long-document).
The next baseline must include 1k/10k paragraphs, long CodeBlock, nested/merged
complex tables and multiple editors. Record hardware, OS, commit, toolchain,
release flags and repeated input/paste/undo/redo p50/p95; separately measure
native shaping/layout/paint, peak RSS and retained history growth. Report initial
mount and steady edits separately. Profile `parent_of`, first-write COW and
per-child updates before broad changes. Virtualization needs selection, IME,
scrolling and accessibility correctness gates of its own.

These scenarios, memory measurements and native rendering evidence are pending.
The numbers above establish only the repaired algorithm's cost and do not close
P6 or promise an end-to-end host speedup.
