# P4 Structured Content / Extension Progress

## Current status

P0 / P1 / P2 / P3 已关闭。P4 统一为两条连续子线：

```text
P4A Inline Atom / Extension Seam  ← CLOSED
P4B Atomic Block / Media          ← 当前施工
```

截至 2026-09-05：

```text
P4.1 Inline Coordinate Contract   CLOSED
P4.2 Canonical Inline Atom        CLOSED
P4.3 Runtime Atom Editing         CLOSED
P4.4 GPUI Renderer / Capability   CLOSED
P4.5 P4A Integration Gate         CLOSED — P4A CLOSED
P4.6 Atomic Block Contract        CLOSED
P4.7 Image Canonical Model        CURRENT (P4B)
```

P4.3 在 PR #58 / #59 / #60 建立主体能力后，经审计修复 PR #62 与 hierarchical structured-paste 收尾 PR #63 补齐边界矩阵。P4.4 由 #64（display projection）/#65（layout / caret）/#66（runtime selection seam）/#67（hit-test / chip paint / 收尾）/#68（键盘视觉导航）与本切片（host capability + harness demo + 多 editor 隔离）闭合；P4.5 通过即 **P4A CLOSED**。

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

### P4.4 GPUI Renderer / Host Capability — CLOSED

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

- [x] `visual_focus_location / horizontal_target` 全链路保留 `InlinePoint`——键盘视觉导航按块坐标空间换算：纯文本块保持 canonical byte 路径；含 atom 块经 `InlineAtomDisplayProjection` 在 display 空间步进，chip 内部作为一个 caret unit 跳过（永不成为 caret 停点），跨块行走保持 canonical 并在目标块尾保留 end-anchored seam ordinal；`desired_x` 连续性锚点升级为 `InlinePoint`
- [x] host capability action 只传 stable kind / action key / attrs / NodeId——`InlineAtomHostCapability::atom_action(AtomAction)`（`crates/xiaomu-gpui/src/atom_capability.rs`；`ATOM_ACTION_CLICK` 稳定键），plain click 落在 chip 内部时经 `DocumentView` 发出；`EditorHooks.atom_capability` 由 `editor.rs` 注入
- [x] 宿主业务类型不得进入 Core / Runtime——capability 只见 `NodeId + AtomKind + action key + attrs 快照`，宿主侧自行解释（harness 用 attrs 解码 `@handle`）；`build_view` 后 `DocumentView::set_atom_capability` 可替换
- [x] editor harness 接入至少一种 demo atom——harness `MentionChipRenderer` 显示 `@{handle}`；store fixture v3：`atom\t<kind>\t<fallback>` 行 + 行内 `{a#N}` 放置 token（`{` 转义 `\{`；v2 读兼容；未定义 atom 引用 fail closed）
- [x] renderer / capability 多 editor 隔离——registry 与 capability 都是 per-editor 值（同 canonical 文档经不同 registry 投影不同、点击 A 不触发 B 的 recorder），e2e 点击测试 `atom_clicks_activate_only_the_clicked_editor`（`tests/multi_editor_host.rs`）

P4.4 Gate：未知 renderer fail soft；相邻 atom 的 caret、selection、layout 与 hit-test 一致；宿主动作不把 business type 带进 Core / Runtime。**P4.4 CLOSED（#67 / #68 / 本切片）。**

### P4.5 Inline Atom Integration Gate — CLOSED

- [x] realistic extension fixture——`xiaomu-runtime/tests/p4a_integration_gate.rs`：多块文档（CJK 标题 + "A中B" 报告段 + BiDi "مرحبا" 段），mention / reference / cursor 三种 atom kind，相邻同缝 atom（byte 1 双 atom）+ CJK 邻接
- [x] multi-editor extension isolation——#69 的 `atom_clicks_activate_only_the_clicked_editor`（点击 / registry / capability 隔离）+ gate `atom_edits_stay_isolated_across_two_editors`（A 输入 CJK、B 文档与 selection 逐字节不变、typing 不触发 activation）
- [x] Unicode + adjacent atom matrix——gate 覆盖：CJK/BiDi 标量与相邻 atom 缝的全程双向 caret walk（one caret unit per step）、三个 seam gap 输入的 re-anchor 顺序、Backspace/Delete 精确删一个 caret unit、跨 CJK+atom 选区一次性替换、undo/redo 精确恢复（含 fallback payload 与 anchor）
- [x] composition + boundary atom matrix——`CommitComposition`：range 起点=atom 缝（ordinal=count）→ 缝上 atom 存活、纯替换其余文本；range 严格覆盖 atom anchor → fail closed 且文档原子不变；Unicode preedit（你好😀）字节 delta 正确平移；IME commit 独立 history entry
- [x] P4A root docs sync——architecture.md 新增 P4.5 gate 事实、视觉导航 chip 缝 stepping 修正、运行时 `atoms_inside_span` 修正
- [x] source-size / dependency / fmt / Clippy / tests——本地全绿
- [x] inline-atom Windows real-machine Gate——本 PR CI run 34011360127 的 windows-latest job 真机执行含 gate 矩阵的完整测试套件通过
- [x] three-platform `CI Success`——同 run Ubuntu / macOS / Windows / policy / 汇总 `CI Success` 全部通过

P4.5 gate 审计发现并修复两个不一致（随本切片交付）：

1. `DocumentSession` 的 `atoms_inside_span` 在 start/end 共享同一 byte 的分支只比较 ordinal、不比较 atom 自身 anchor——空文本区间的选区（如"只选中两个相邻 atom"）会把**其他 boundary 上 ordinal 恰好落入窗口的无关 atom 一并删除**。修正为该分支要求 `offset == start_raw`（`crates/xiaomu-runtime/src/session/atom_edit.rs`）。
2. GPUI 视觉导航 display 空间步进把"恰好落在 chip 起始 byte"的步进目标当内部处理，导致**标量后紧邻 chip 时一次按键连跨两个视觉停点**（chip 左缝不可达）。修正为 chip 起始 byte 是缝停点、仅严格内部才整体跳过（`visual_navigation.rs::atom_horizontal_target`）；#68 的 Right 链随之多一个 (2,0) 停点——与 runtime canonical walk（ADR 0005 one-caret-unit）逐位一致。

**P4A CLOSED**（P4.5 随 PR #70 通过闭合），随后继续 P4B，不关闭整个 P4。

## P4B — Atomic Block / Media

### P4.6 Atomic Block Contract — CLOSED

- [x] editable text + atomic traversal model——GPUI 侧 `navigation::nav_units` 把文档顺序推广为 text + atomic 序列（`NavUnit::Text / Atomic`），横向步进 `step_horizontal` 返回 `HorizontalTarget::InText / OnAtomic`；atomic selection 上 Up/Down/LineStart/LineEnd 在本切片为 no-op（atomic 块无可走可视行，P4.9 closeout 复核）
- [x] `NodeSelection / atomic position` contract——`DocumentPosition::Atomic(NodeId)`：validate 限定 atomic content（文本/容器节点不可 node-select）；`Slots` 以节点自身 slot 排序（gap-before < atomic < gap-after）；`map_through` 经 `ChangeMap::map_node_selection`；collapsed accessor `as_atomic_node`；公开 seam `DocumentSession::set_atomic_selection`
- [x] HorizontalRule keyboard traversal——`text ↔ HorizontalRule ↔ text` 纯键盘往返：Text 块边界横向步进落在相邻 atomic 单元（节点选择），Atomic 焦点再 Right/Left 跨到相邻文本块的 start/end（end-anchored seam ordinal 保留）；e2e `tests/atomic_block_gpui.rs`
- [x] atomic click / select / delete——atomic 规则条渲染为整块可选元素：节点选择激活时加粗 accent 高亮；plain click 经 `set_atomic_selection` 选中（listener `stop_propagation` 阻止 caret 放置）；Backspace/Delete 删除 + undo/redo（runtime seam）
- [x] atomic copy / paste——`ClipboardNodeContent::Atomic` 变体（kind + attrs 即全部载荷）；collapsed atomic selection 经 `slice_selection` 投影为单 atomic root 的 ClipboardSlice；clipboard metadata wire 升级 **v4**（`WireContent::Atomic` + `WireKind::HorizontalRule / Image`；v3 及以下含 atomic 的载荷仍 fail soft 到 plain text）；paste 全 atomic root 在聚焦块后插入兄弟块（`InsertNode` + `SelectionUpdate::MapExisting`，caret 原地保留）；mixed inline/atomic fragment 层级粘贴 fail closed（新 `SessionError::ClipboardAtomicUnsupported`）。已知边界：跨块文本选区跨越 atomic 块时，flat leaf 投影仍不携带中间 atomic 节点（P4.9 clipboard closeout 复核）
- [x] mapping / selection fallback——`SelectionUpdate::CaretAtGap`（resolve 时对 post-snapshot 验证）；无关文本编辑不干扰 atomic endpoint 的 identity mapping
- [x] undo / redo invariant tests——`tests/atomic_block_selection.rs`：undo 恢复块并重新安装 Atomic selection，redo 再次删除；stale atomic endpoint fail closed

### P4.7 Image Canonical Model / AssetService — CURRENT

- [x] typed Image attrs——Core `document/image.rs`：`ImageAttrs` 类型化视图（source / alt / title / width / height）经 canonical attrs 键 `src` / `asset` / `alt` / `title` / `width` / `height` 读写；校验：source 二选一且非空、alt 非空、尺寸为正；新 `Error::InvalidImageAttrs`。像素、texture handle、宿主文件对象与宿主绝对路径一律不入 canonical document
- [x] frontend-neutral `AssetRef / ImageSource`——`ImageSource::AssetRef(opaque)`（宿主经 capability seam 解析）与 `ImageSource::ExternalUrl(url)`（codec/host 显式导入）；`AssetRef` 为宿主定义的 opaque key，运行时仅校验非空
- [x] image insertion command——`EditIntent::InsertImage { image: ImageAttrs }`：Image 原子块作为聚焦块的下一个兄弟插入（`InsertNode` + `SelectionUpdate::MapExisting`，caret 原地保留，Isolated history entry），undo 可逆
- [x] `AssetService` capability seam——Runtime `assets.rs`：`AssetService::resolve(AssetRef, Rc<dyn AssetSink>)`，宿主拥有存储/网络/缓存/权限；无 async runtime、无文件 API、无 GPUI 类型穿过 seam；`ResolvedAsset` 携带 `revision` 供消费方做 stale-cache 判定，回调不直接改 canonical document
- [x] host-neutral resolve failure model——`AssetError::{InvalidRef, NotFound, PermissionDenied, Unavailable}`；bytes 为 opaque 载荷，解码归前端
- [x] no local absolute-path canonical identity——typed 层不提供路径 source 形态；宿主路径只能作为 `AssetRef` opaque 值进入 attrs（架构 3.1 条款）

### P4.8 GPUI Image / Atomic Interaction — CURRENT

- [x] async asset resolve——`crates/xiaomu-gpui/src/image_block.rs`：paint pass 对 stale 的 Image AssetRef 发起 `AssetService::resolve`；sink 仅落地 cache（`ImageLoadCache`，键 node identity + source key），stale result 按 source 变更丢弃；resolve 回调无前端上下文，re-render 调度归宿主集成（cache 幂等，paint 时重读）
- [x] loading / error placeholder——Image 块渲染状态化占位：无 service / ExternalUrl → 中性占位 + alt；Loading / Resolved / Failed(AssetError) 各自颜色与标签；invalid attrs fail soft 显示错误占位。**真实 texture 解码绘制留待下一切片**（需引入解码依赖）
- [ ] image layout / paint / hit-test（texture 绘制切片）
- [ ] aspect ratio / intrinsic size（占位固定高度；texture 切片按 attrs 宽高比缩放）
- [x] mouse + keyboard selection——Image 块即 P4.6 atomic 单元：plain click 节点选择、Left/Right 横向 traversal、Backspace/Delete/undo/redo 全部复用 P4.6 seam；e2e `tests/image_block_gpui.rs`
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
