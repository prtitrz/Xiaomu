# P4 Structured Content / Extension Progress

## Current status

P0 / P1 / P2 / P3 已关闭。P4 统一为两条连续子线：

```text
P4A Inline Atom / Extension Seam  ← 当前施工
P4B Atomic Block / Media          ← P4A 后继续
```

截至 2026-09-03：

```text
P4.1 Inline Coordinate Contract   CLOSED
P4.2 Canonical Inline Atom        CLOSED
P4.3 Runtime Atom Editing         CLOSED pending PR #63 merge
P4.4 GPUI Renderer / Capability   CURRENT
P4.5 P4A Integration Gate         NEXT
```

P4.3 在 PR #58 / #59 / #60 建立主体能力后，经审计修复 PR #62 与 hierarchical structured-paste 收尾 PR #63 补齐边界矩阵。PR #63 current-head CI #354 已通过 Ubuntu、macOS、Windows、policy 与汇总 `CI Success`。

## P4A — Inline Atom / Extension Seam

### P4.1 Inline Coordinate Contract — CLOSED

已交付：

- [x] `InlinePoint(node_id, text_offset, atom_index, affinity)`
- [x] `TextOffset` 继续严格表示 UTF-8 text byte offset
- [x] Core `StepMap / ChangeMap` mixed-inline mapping seam
- [x] Runtime `DocumentPosition::Inline`
- [x] GPUI focus / selection compatibility seam
- [x] ADR 0005
- [x] P0-P3 regression

核心约束：同一 text boundary 上的多个 atom 通过 `atom_index` 区分 caret gap；不使用 sentinel、fake byte，也不滥用 `CursorAffinity` 表示 atom order。

### P4.2 Canonical Inline Atom — CLOSED

已交付：

- [x] `AtomKind`
- [x] `InlineAtomContent { fallback_text }`
- [x] stable `NodeId` atom identity
- [x] `InlineContent` ordered atom placements
- [x] full-tree validation / parent lookup
- [x] `InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom`
- [x] `ReplaceInlineText { at: InlinePoint, end, replacement }`
- [x] mixed-inline mapping / inverse
- [x] adjacent atom invariant / undo round-trip

Canonical 事实：

```text
NodeKind::InlineAtom(AtomKind)
NodeContent::InlineAtom(InlineAtomContent)
InlineAtomPlacement(atom NodeId, text_offset)
InlineContent = normalized text runs + ordered atom placements
```

Core `SplitNode / JoinNodes` 遇 atom 继续 fail closed。Runtime 需要结构迁移时必须显式搬运 atom identity 与 placement，不能通过放宽 Core 规则掩盖语义。

### P4.3 Runtime Atom Editing — CLOSED pending PR #63 merge

已交付并通过回归：

- [x] Left / Right one-caret-unit atom navigation
- [x] Home / End 正确处理 leading / trailing atom outer gap
- [x] atomic Backspace / Delete
- [x] mixed text + atom selection
- [x] atom-aware text input
- [x] same-boundary adjacent atom editing
- [x] cross-block atom selection / delete / cut
- [x] detached `ClipboardInline / ClipboardAtom`
- [x] plain-text fallback 使用 `fallback_text`
- [x] single-block structured paste 分配 fresh atom identity
- [x] Unicode / trailing atom paste 使用 post-edit text coordinate
- [x] cross-block clipboard 保留边界 atom
- [x] cross-block paste 使用 atom-aware deletion planner
- [x] hierarchical structured paste 支持 atom-bearing target
- [x] hierarchical clipboard leaf 支持 detached atom materialization
- [x] IME composition 不进入 atom 内部，边界 atom 保持
- [x] undo / redo 精确恢复 selection、store 与 stable identity

P4.3 的结构编辑原则：

```text
Core SplitNode / JoinNodes(atom-bearing) = fail closed

Runtime cross-block / hierarchical command
  → staged transaction
  → RemoveInlineAtom + RestoreInlineAtom 显式搬迁 stable NodeId
  → ReplaceInlineText 处理 text seam
  → intermediate snapshot 不对 session 可见
  → 整个命令仅形成一个 history entry
```

PR 轨迹：

```text
#58 mixed-inline session position / navigation
#59 atom editing semantics
#60 clipboard / IME / history
#62 P4.3 audit regressions + atom-aware cross-block deletion
#63 hierarchical structured paste + atom-aware staged split
```

### P4.4 GPUI Renderer / Host Capability — CURRENT

已交付：

- [x] `InlineAtomRendererRegistry`
- [x] renderer 只消费 canonical `InlineAtomView`
- [x] missing renderer deterministic fallback 到 `fallback_text`
- [x] accessibility fallback：atom 是携带 `fallback_text` 的非编辑子节点
- [x] mixed-inline display projection（#64 `InlineAtomDisplayProjection`）
- [x] caret / selection display mapping（#65 projection 化 caret / selection；#66 runtime `set_inline_selection` seam）
- [x] layout / paint（#65 atom-aware layout；chip quad 随本切片落地）
- [x] atom hit-test（display byte 反投影 + chip 左右半区规则）
- [ ] demo atom renderer
- [ ] host capability callback
- [ ] harness demo

#### P4.4b Mixed-inline display projection

P4.4b 必须先建立显式坐标投影，再把 renderer text 接入 GPUI layout。禁止假设：

```text
display byte index == canonical UTF-8 TextOffset
```

atom 不占 canonical text byte，但 renderer display text 会占 display byte。直接把 `renderer.display_text()` splice 进 paragraph string 后继续使用 canonical byte 做 caret / selection / hit-test，会让第一个 atom 之后的所有几何坐标漂移。

目标 contract：

```text
canonical InlinePoint
        ↕
MixedInlineDisplayProjection
        ↕
display byte boundary
        ↓
GPUI wrapped layout / paint / hit-test
```

建议 projection 至少携带：

```text
DisplayProjection
  text
  styled text segments
  atom display spans
  canonical-gap -> display-boundary mapping
  display-boundary -> InlinePoint mapping

DisplayAtomSpan
  node_id
  canonical text_offset
  atom_index
  display byte range
```

同一 canonical boundary 上有 N 个相邻 atom 时：

```text
ordinal 0 → first atom display span 之前
ordinal 1 → atom 1 / atom 2 之间
...
ordinal N → last atom display span 之后
```

P4.4b 实施顺序：

1. [x] 建立纯 GPUI-local `MixedInlineDisplayProjection` 与映射测试（#64 `crates/xiaomu-gpui/src/inline_atom_display.rs`）。
2. [x] `ParagraphElement` layout 使用 projection text，不再把 canonical byte 直接传给 display geometry（#65 `block_view/display.rs::layout_content`）。
3. [x] caret / selection 先从 `InlinePoint` 投影成 display byte，再访问 `BlockTextLayout`（#65 `display_focus_caret` / `projected_display_selection`；#66 `DocumentSession::set_inline_selection` 公开 seam，text-only `set_selection` 保留为兼容适配器）。
4. [x] pointer hit-test 从 display byte 反投影到 `InlinePoint`，atom span 按点击左右半区落到前/后 gap（`inline_point_for_display_hit`：严格小于 span 中点 → before gap，否则 after gap；span 边界与文本字节经 `inline_point_for_display_boundary`）。
5. [x] platform UTF-16 / input-handler range 明确区分 display range 与 canonical range——设计裁决：input handler / IME 始终消费 canonical editable projection，display range 只存在于 layout / paint / hit-test 内部，永不进入 platform range。
6. [x] projection 稳定后再增加 chip paint / atom bounds registry——chip 按可视行绘制 tinted quad（selection 高亮覆盖其上）；per-atom 几何由 display span + wrapped layout 按需推导，未引入独立 bounds registry。

文档补记：#64 / #65 / #66 合并时漏更本文件，随本切片一并补记。

自定义 renderer 若返回空 display text，projection 必须 fail soft 到非空 `fallback_text`，避免 atom 成为零宽且无法命中的 canonical unit。（已交付：`InlineAtomDisplayProjection::build`。）

#### P4.4c Host capability / demo

- [ ] `visual_focus_location / horizontal_target` 全链路保留 `InlinePoint`
- [ ] host capability action 只传 stable kind / action key / attrs / NodeId
- [ ] 宿主业务类型不得进入 Core / Runtime
- [ ] editor harness 接入至少一种 demo atom
- [ ] renderer / capability 多 editor 隔离

P4.4 Gate：未知 renderer fail soft；相邻 atom 的 caret、selection、layout 与 hit-test 一致；宿主动作不把 business type 带进 Core / Runtime。

### P4.5 Inline Atom Integration Gate

- [ ] realistic extension fixture
- [ ] multi-editor extension isolation
- [ ] Unicode + adjacent atom matrix
- [ ] composition + boundary atom matrix
- [ ] inline-atom Windows real-machine Gate
- [ ] P4A root docs sync
- [ ] source-size / dependency / fmt / Clippy / tests
- [ ] three-platform `CI Success`

P4.5 通过只代表 **P4A CLOSED**，随后继续 P4B，不关闭整个 P4。

## P4B — Atomic Block / Media

### P4.6 Atomic Block Contract

- [ ] editable text + atomic traversal model
- [ ] `NodeSelection / atomic position` contract
- [ ] HorizontalRule keyboard traversal
- [ ] atomic click / select / delete / copy
- [ ] mapping / selection fallback
- [ ] undo / redo invariant tests

### P4.7 Image Canonical Model / AssetService

- [ ] typed Image attrs
- [ ] frontend-neutral `AssetRef / ImageSource`
- [ ] image insertion command
- [ ] `AssetService` capability seam
- [ ] host-neutral resolve failure model
- [ ] no local absolute-path canonical identity

### P4.8 GPUI Image / Atomic Interaction

- [ ] async asset resolve
- [ ] loading / error placeholder
- [ ] image layout / paint / hit-test
- [ ] aspect ratio / intrinsic size
- [ ] mouse + keyboard selection
- [ ] text ↔ image traversal
- [ ] Backspace / Delete / undo / redo
- [ ] accessibility fallback

### P4.9 Clipboard / Markdown / P4 Final Closeout

- [ ] atomic/image structured clipboard
- [ ] semantic plain-text / URL fallback
- [ ] baseline built-in Markdown round-trip
- [ ] unknown extension/image attrs preservation
- [ ] realistic media fixture
- [ ] multi-editor isolation
- [ ] Unicode + atom + atomic matrix
- [ ] Windows final real-machine Gate
- [ ] architecture / planning / progress final sync
- [ ] final three-platform `CI Success`

## P4 Phase Gate

### Inline Atom

- [x] true canonical one-caret-unit atom model
- [x] adjacent atoms independently navigable / deletable at Runtime
- [x] atom seam text input preserves `(text_offset, atom_index)`
- [x] Runtime copy / cut / paste / undo / redo preserve atom semantics
- [x] accessibility always has `fallback_text`
- [ ] GPUI mixed-inline projection / hit-test Gate
- [ ] unknown/missing renderer visual fallback Gate
- [ ] extension host capability Gate

### Atomic / Media

- [ ] text ↔ HorizontalRule ↔ text stable traversal
- [ ] host can insert Image through public contract
- [ ] canonical Image stores host-neutral semantics only
- [ ] `AssetService` async resolve + fallback
- [ ] Image mouse / keyboard selection
- [ ] text ↔ Image traversal
- [ ] atomic/image clipboard + undo/redo
- [ ] Markdown supported built-ins round-trip

### Regression / integration

- [x] P0-P3 regression remains green through P4.3
- [ ] P4A integration Gate
- [ ] P4B integration Gate
- [ ] final Windows real-machine Gate
- [ ] final three-platform `CI Success`

只有上述 Gate 完成，才允许 **P4 = CLOSED** 并进入 P5 Table。
