# P4 Structured Content / Extension Progress

## Current status

P3 已于 2026-09-01 完成 Windows 最终实机 Gate，并通过 PR #53 squash merge 到 `main`（`f1167e97f2cac852fc56933c631b37978adcac9f`），阶段状态 `P3 = CLOSED`。

P4 统一为两条连续子线：

```text
P4A Inline Atom / Extension Seam  ← 当前施工
P4B Atomic Block / Media          ← P4A 后继续
```

此前两个并列目录 `p4-inline-atom-extension` 与 `p4-atomic-media-extension` 已收敛为统一目录 `p4-structured-content-extension`，避免出现两套冲突的 P4.1/P4.2 编号。

P4.1 与 P4.2A/P4.2B 已分别通过 PR #54、#55、#56 squash merge 到 `main`（2026-09-01），早期 Draft PR #50/#51/#52 已由上述重放 PR 取代并关闭。

当前 P4.2 收尾分支 / PR：

```text
feat/p4.2-atom-aware-replacement
PR #57（atom-aware text replacement contract + mapping/inverse）
```

## P4A — Inline Atom / Extension Seam

### P4.1 Inline Coordinate Contract（CLOSED，PR #54）

- [x] P4 overall design
- [x] mixed-inline coordinate ADR 0005
- [x] `InlinePoint` Core value type
- [x] pure-text validation / conversion
- [x] Runtime `DocumentPosition` compatibility seam
- [x] Core `StepMap / ChangeMap` mapping seam
- [x] GPUI `DocumentView` focus/selection projection seam
- [x] P0/P1/P2/P3 full regression
- [x] root `architecture.md / planning.md` sync after P3 closeout rebase
- [x] CI #320 full success
- [x] bookkeeping head CI #321 full success
- [x] unified-P4 docs current-head CI Success（merge Gate，PR #54）

Gate：无 atom 文档零语义回归；`TextOffset` 继续严格表示 UTF-8 byte coordinate；P4.1 不引入 sentinel / fake byte；canonical atom placement 建立前非零 ordinal fail closed。

实现事实：

```text
TextPoint
↕ exact while atom_index == 0
InlinePoint(node_id, text_offset, atom_index, affinity)

Core
  InlinePoint::validate
  StepMap::map_inline_point
  ChangeMap::map_inline_point

Runtime
  DocumentPosition::from_inline_point
  DocumentPosition::as_inline_point
  DocumentSelection::from_inline_points

GPUI
  DocumentView::inline_focus_point
  DocumentView::inline_selection_points
```

关键约束：atom seam 两侧可能共享同一个 `TextOffset`，旧 `ReplaceText(TextRange)` 无法区分“在 atom 前输入”和“在 atom 后输入”。后续 mutation / mapping 必须消费 `InlinePoint.atom_index`。

### P4.2 Canonical Inline Atom（Core 事实层 CLOSED；运行时消费进入 P4.3）

- [x] `AtomKind`
- [x] `InlineAtomContent`
- [x] stable `NodeId` atom identity
- [x] `InlineContent` ordered atom placement
- [x] full-tree validation / parent lookup
- [x] `InsertInlineAtom` / `RemoveInlineAtom`（P4.2 transaction slice，PR #56）
- [x] atom-aware text replacement contract（`ReplaceInlineText`，PR #57）
- [x] mapping / inverse（`InlineTextReplaced` StepMap + exact inverse，PR #57）
- [x] adjacent atom placement invariant tests

已成立的 canonical value-layer 事实：

```text
NodeKind::InlineAtom(AtomKind)
NodeContent::InlineAtom(InlineAtomContent)
InlineAtomPlacement(atom NodeId, text_offset)
InlineContent = normalized runs + ordered atom placements
```

P4.2 replacement contract 语义（ADR 0005 Transaction consequence 的落地）：

```text
ReplaceInlineText { at: InlinePoint, end: TextOffset, replacement }
  - 取代 seam 上歧义的裸 ReplaceText；旧 ReplaceText 在含 atom 的
    closed boundary span 上继续 fail closed
  - at.atom_index() 之前的同界 atom 保持锚点；空 range 时其后的
    seam atom 移到插入文本之后
  - 非 empty range 的替换区域含 atom 时 fail closed
    （caller 必须先用 RemoveInlineAtom 显式删除）
  - end 锚定处及其后的 atom 按 byte-length delta 平移
  - StepMap::InlineTextReplaced 消费 seam ordinal 做 mixed-inline
    mapping；inverse 同为 atom-aware 步骤并精确恢复 store
```

P4.2 Gate：相邻两个 atom 可稳定构造、validate、insert/remove、undo/redo；不存在 sentinel text；atom seam 文本 mutation 不丢失 `atom_index`。Gate 已由 PR #56 / #57 的 transaction、mapping 与 undo round-trip 测试覆盖。`SplitNode / JoinNodes` 遇 atom 仍 fail closed（设计允许，规则未证明前不 ad-hoc 修补）。

### P4.3 Runtime Atom Editing

- [x] one-caret-unit Left / Right（Runtime `move_caret` 按 atom ordinal + scalar boundary 步进）
- [ ] atomic Backspace / Delete
- [ ] mixed text + atom selection（存储与 mapping 已升级；编辑语义随后续切片）
- [ ] atom-aware text input

P4.3a 已交付（PR #58）：Runtime `DocumentPosition::Text(TextPoint)` 升级为 `DocumentPosition::Inline(InlinePoint)`，caret 可停在同一 boundary 的任意 canonical gap；`from_inline_point` 不再丢弃 ordinal，ordinal 合法性统一由 `DocumentSelection::validate` 对 snapshot 校验；`DocumentSelection::map_through` 对 inline endpoint 消费 `ChangeMap::map_inline_point`；document-order 排序把 `atom_index` 计入 seam gap。planner（intent/cross_block/paste）仍走 text-only 路径，seam 上的编辑在下一切片切到 `ReplaceInlineText`。
- [ ] structured clipboard atom fragment
- [ ] plain fallback via `fallback_text`
- [ ] IME cannot enter atom
- [ ] history / undo / redo selection regression

### P4.4 GPUI Renderer / Host Capability

- [x] `InlineAtomRendererRegistry`（PR #61：`AtomKind` -> renderer，只消费 canonical 数据）
- [ ] demo atom renderer
- [ ] mixed inline layout / paint
- [ ] hit-test
- [x] accessibility fallback（atom 投影为携带 `fallback_text` 的非可编辑子节点）
- [ ] host capability callback
- [x] missing renderer fallback（确定性回落到 `FallbackAtomRenderer`）

### P4.5 Inline Atom Integration Gate

- [ ] realistic extension fixture
- [ ] multi-editor extension isolation
- [ ] Unicode + adjacent atom matrix
- [ ] inline-atom Windows real-machine Gate
- [ ] P4A docs sync
- [ ] source-size / dependency / fmt / Clippy / tests
- [ ] P4A CI Success

P4.5 通过只代表 **P4A CLOSED**，随后继续 P4B，不关闭整个 P4。

## P4B — Atomic Block / Media

### P4.6 Atomic Block Contract

- [ ] editable text + atomic traversal model
- [ ] `NodeSelection` / atomic position contract
- [ ] HorizontalRule keyboard traversal
- [ ] atomic click/select/delete/copy
- [ ] mapping / selection fallback
- [ ] undo / redo invariant tests

### P4.7 Image Canonical Model / AssetService

- [ ] Image attrs / typed accessor contract
- [ ] frontend-neutral `AssetRef / ImageSource`
- [ ] image insertion command
- [ ] `AssetService` host capability seam
- [ ] host-neutral resolve failure model
- [ ] no local absolute-path canonical identity

### P4.8 GPUI Image / Atomic Interaction

- [ ] async asset resolve
- [ ] loading / error placeholder
- [ ] image layout / paint / hit-test
- [ ] aspect-ratio / intrinsic-size handling
- [ ] click-to-select image
- [ ] text ↔ image keyboard traversal
- [ ] Backspace / Delete / undo / redo
- [ ] accessibility fallback

### P4.9 Clipboard / Markdown / P4 Final Closeout

- [ ] atomic/image structured clipboard
- [ ] semantic plain-text / URL fallback
- [ ] baseline built-in Markdown round-trip
- [ ] HorizontalRule / Image / marks / links preservation
- [ ] unknown extension/image attrs preservation
- [ ] realistic media + extension fixture
- [ ] multi-editor isolation
- [ ] Unicode + atom + atomic matrix
- [ ] Windows final real-machine Gate
- [ ] architecture / planning / progress final sync
- [ ] source-size / dependency / fmt / Clippy / tests
- [ ] final CI Success

## P4 Phase Gate

### Inline Atom

- [ ] demo atom is a true one-caret-unit canonical value
- [ ] two adjacent atoms are navigable and independently deletable
- [ ] atom seam text input preserves `(text_offset, atom_index)` semantics
- [ ] copy/cut/paste/undo/redo preserve atom semantics
- [ ] unknown/missing renderer has deterministic fallback
- [ ] extension action crosses a host capability seam without host business types entering Core/Runtime
- [ ] accessibility always has `fallback_text`

### Atomic / Media

- [ ] text ↔ HorizontalRule ↔ text supports stable keyboard navigation and `NodeSelection`
- [ ] host can insert an Image through public contract
- [ ] canonical Image stores only frontend-neutral / host-neutral semantics
- [ ] `AssetService` resolves asynchronously with loading/error fallback
- [ ] Image supports mouse + keyboard selection
- [ ] text ↔ Image ↔ text navigation is stable
- [ ] Backspace/Delete/copy/cut/undo/redo semantics are explicit for atomic/image
- [ ] structured clipboard carries atomic image payload
- [ ] baseline Markdown round-trip does not silently lose supported built-in semantics
- [ ] unknown extension/image attrs preservation passes

### Regression / integration

- [x] existing P0-P3 behavior remains green on P4.1 heads
- [ ] P4A integration Gate passed
- [ ] P4B integration Gate passed
- [ ] final Windows real-machine Gate passed
- [ ] final three-platform CI Success

只有上述三组 Gate 全部成立，才允许 **P4 = CLOSED** 并进入 P5 Table。
