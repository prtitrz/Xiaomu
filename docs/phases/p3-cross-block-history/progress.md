# P3 Visual Lines / Cross-block Clipboard / History 进度

状态：进行中

本文档只记录 P3 执行状态和验证证据。阶段设计见 `design.md`；长期架构事实放在 `docs/architecture.md`；顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P3.3 Cross-block Editing / Structured Clipboard 收口**

前置状态：P0、P1、P2 均已 CLOSED。P3.0 Phase Contract、P3.1 Visual-line Geometry / Soft-wrap、P3.2 Visual Navigation / Selection 均已完成并合入 `main`；P3.3 实现与自动化 Gate 已完成，PR #44 进入最终 CI / squash merge 收口。

## P3.0 Phase Contract

- [x] 创建 P3 `design.md`
- [x] 创建 P3 `progress.md`
- [x] 固化 P2 → P3 handoff
- [x] 将 `roadmap-gap-review.md` 中 visual-line / structured clipboard / stored marks / accessibility / multiline 结论并入阶段契约
- [x] source-size baseline 复核
- [x] dependency-boundary baseline 复核
- [x] fmt / clippy / workspace tests / CI Success

P3.0 不改生产代码。其 Gate 已通过 PR #40，并 squash merge 到 `main`（`87888e25`）。

## P3.1 Visual-line Geometry / Soft-wrap

- [x] block `TextLayout` abstraction
- [x] soft-wrap shaping
- [x] visual line logical ranges
- [x] logical offset → visual caret geometry
- [x] visual point → logical offset hit-test
- [x] layout cache 从 single `ShapedLine` 升级
- [~] Unicode/CJK/emoji/combining/BiDi visual 专项 fixture：canonical / coordinate regression 已由既有测试覆盖，完整 visual matrix 并入 P3.7
- [x] Windows 实机窄宽 paragraph soft-wrap smoke Gate

实现事实：

```text
ParagraphElement
→ request_measured_layout
→ shape_text(wrap_width)
→ BlockTextLayout
→ wrapped caret / selection rects / 2D hit-test / IME range geometry
```

soft-wrap 只存在于 GPUI frontend projection，Core `TextPoint / TextOffset` contract 未改变。wrapped selection 已能按视觉行绘制多矩形；pointer drag selection 使用二维 hit-test。

验证证据：

- PR #41 最新 head `191c64f3` 的 CI run #151：policy、fmt、clippy、workspace tests、Windows/macOS/Linux 全部通过。
- Windows 实机 Gate：窄宽长段落 soft-wrap、caret、点击、文字拖选与 IME smoke 均未发现异常。
- PR #41 已 squash merge 到 `main`（`fe266de4`）。
- “拖选”在本 Gate 中指 pointer drag selection；拖动既有 selection 进行文本搬移（selection drag-and-drop editing）不属于 P3 必做范围，作为后续增强能力评估。

## P3.2 Visual Navigation / Selection

- [x] frontend-transient `desired_x`
- [x] x-preserving Up / Down
- [x] 跨 block visual Up / Down
- [x] visual Home / End
- [x] wrapped selection multi-rect projection（P3.1 已建立 geometry）
- [x] wrapped mouse click / drag hit-test（P3.1 已建立 geometry）
- [x] event-driven scroll-to-caret
- [x] `CursorAffinity` 在 soft-wrap boundary 的视觉解析
- [x] final current-head fmt / clippy / workspace tests / three-platform CI
- [x] Windows 长段落实机 Gate

实现事实：

```text
Core TextPoint(node, TextOffset, CursorAffinity)
→ ParagraphView / BlockTextLayout visual projection
→ visual row + pixel caret geometry
→ DocumentView keyboard translation
→ Runtime SetSelection
```

- `desired_x` 只存在于 `DocumentView`，不会进入 Core canonical position；连续 Up / Down 保持目标视觉列，横向移动、Home / End、直接 selection 设置或编辑会清除旧锚点。
- 同一个 soft-wrap logical offset 可通过 `CursorAffinity` 区分上一视觉行末尾与下一视觉行开头。Left / Right 会先跨越这两个视觉 caret state，再跨 Unicode scalar。
- Up / Down 优先在当前 block 相邻 visual row 内移动；越过 block 边界后，在相邻 inline block 的首 / 末 visual row 上按相同 `desired_x` 求最近合法 Core offset。
- Home / End 解析当前 visual row，而非整段 paragraph 的逻辑首尾；Shift 版本沿用同一 focus target 并保留 anchor。
- pointer hit-test 与 keyboard navigation 共享相同 soft-wrap affinity 语义。
- `DocumentView` 持有一个 GPUI `ScrollHandle`；caret 只在编辑、键盘导航、selection focus 移动或 IME preedit 等显式 caret 变化时请求一次最小纵向滚动。纯用户滚轮浏览不会被 caret 强制吸回。
- structural edit 后若 selection focus 落到新建 block，`DocumentView` 会先同步 child views，再路由 platform focus，避免 Enter 后新 block 已创建但输入焦点仍停在旧 view。

验证证据：

- PR #42 head `f2eaba8b` 的 CI run #167：policy、fmt、Clippy、workspace tests、Windows/macOS/Linux 与 `CI Success` 全绿。
- Windows 首轮实机 Gate 暴露两项 P3.2 回归：Enter 分段后新 block 尚未挂载导致 focus 路由失败；scroll-to-caret 每帧纠正 viewport 导致用户无法主动滚离 caret。
- 两项回归已在 PR #42 内修复：结构变更先 `sync_children` 再 `route_focus`；scroll-to-caret 改为 one-shot request，用户主动滚动不触发回拉。
- 修复后 PR #42 current head `e333096a` 的 CI run #171：policy、fmt、Clippy、workspace tests、Windows/macOS/Linux 与 `CI Success` 全绿。
- Windows 复测：Enter 后新段落可直接继续输入；caret 滚出视口后仍可自由滚动，后续键盘/输入才重新触发 caret 可见性；其余 P3.2 visual navigation / selection Gate 未发现异常。
- PR #42 已 squash merge 到 `main`（`47fcb490`）。

## P3.3 Cross-block Editing / Structured Clipboard

- [x] cross-block delete
- [x] cross-block copy / cut
- [x] frontend-neutral structured clipboard payload
- [x] plain-text fallback
- [x] structured paste
- [x] one-command one-history-entry 原子语义
- [x] list / heading / paragraph 跨结构 selection regression
- [x] mapping + after-selection regression

实现事实：

```text
DocumentSelection
→ ClipboardSlice
   ├─ plain_text
   ├─ flat ClipboardBlock leaves
   └─ minimal ClipboardNode fragment roots
→ GPUI ClipboardItem(text + Xiaomu metadata v2)
→ PasteSlice
   ├─ leaf-only：单笔 Core transaction
   └─ container fragment：hidden staged transactions
→ one Runtime history entry
```

- `ClipboardSlice` 是 Runtime 的 detached value，不携带 canonical `NodeId`。单一 inline leaf 只复制所选 inline fragment；跨多个 leaf 时保留覆盖 selection 的最小 container 子树，因此 list / quote 等结构可以保留而不会把未选 sibling 一并复制。
- plain-text fallback 始终以 `\n` 表示所选 inline block boundary。外部应用只看到普通文本；晓木内部通过 GPUI string metadata 携带版本化 `xiaomu.clipboard` v2 fragment tree。
- metadata decode 对 foreign / malformed / unknown-version / stale-text payload fail closed，并自动退回 plain text；Core canonical types 不依赖 serde 或 GPUI platform type。
- cross-block Delete 由 Runtime 编译为 typed Core transaction：保留 head block identity 与 prefix，将 tail suffix 搬到 seam，删除覆盖的中间 leaf，并只清理因本次删除而变空的 container。Cut 先只读投影 clipboard，再执行同一笔 Delete，因此只有一个 history mutation。
- leaf-only structured paste 使用 `ReplaceText / AddMark / RemoveMark / InsertNode`，精确保留 source marks 和插入 block kind/attrs，并把 host suffix 接到最后 pasted leaf。
- 含 container 的 structured paste 使用既有 `StagedPlan`：先在 selection seam 分出 host prefix / suffix，再按 fragment tree 重建 container/leaf。中间 snapshot 不暴露给 session；所有阶段 inverse 合并成一个 undo entry，redo 继续复用已分配 identity。
- staged paste 的 after-selection 落在最后 pasted inline leaf 的 paste seam；cross-block Delete 则落在 surviving head seam。undo / redo 均恢复精确 store 与 selection。

验证证据：

- `cross_block_editing.rs` 覆盖跨 paragraph → nested list 的 Delete / Cut、单 history entry、surviving seam、精确 undo / redo。
- `clipboard_metadata.rs` 覆盖 kind / attrs / runs / marks 与最小 list fragment tree 的 metadata v2 round-trip，以及 foreign / stale / unknown-version fallback。
- `structured_paste.rs` 覆盖单 leaf exact marks、multi-block kind / attrs / suffix / caret、cross-block replacement 原子 undo。
- `hierarchical_paste.rs` 覆盖 list fragment 插入普通 paragraph seam、跨 block target 替换、container reconstruction、last-leaf caret 与 staged undo / redo。
- PR #43 hierarchy implementation head `bbb61a01` 的 CI run #214：policy、source-size、dependency boundary、fmt、Clippy、workspace tests、Windows/macOS/Linux 与 `CI Success` 全绿。
- 收口文档 head `44401225` 的 CI run #216：同样全部通过；因 #43 的 Draft 状态无法通过当前 connector 切换，最终 merge PR 为 #44，代码基线相同。

P3.3 设计 Gate 已满足；PR #44 仅剩 current-head CI 与 squash merge。

## P3.4 History Grouping / Stored Marks / IME

- [ ] typing coalescing / history group
- [ ] caret / selection move 断组
- [ ] structural / paste / cut / mark boundary
- [ ] IME updates 不写 history
- [ ] IME commit 恰好一次 history commit
- [ ] collapsed `StoredMarks`
- [ ] normal typing / IME 对 StoredMarks 的一致继承
- [ ] undo / redo selection + pending mark regression

## P3.5 HardBreak / CodeBlock Multi-line

- [ ] ADR：HardBreak canonical representation
- [ ] 明确 soft-wrap / HardBreak / CodeBlock newline 三者边界
- [ ] Shift+Enter 语义（若 ADR 选择在 P3 实施）
- [ ] CodeBlock Enter
- [ ] CodeBlock multiline paste
- [ ] CodeBlock Tab / indentation
- [ ] mapping / inverse regression

## P3.6 Accessibility / Realistic Host Integration

- [ ] accessibility projection：text
- [ ] semantic node role/kind
- [ ] selection projection
- [ ] focus projection
- [ ] restore selection/focus integration fixture
- [ ] multiple editors coexist
- [ ] focus isolation
- [ ] persistence / listener / save 不串状态

## P3.7 Closeout

- [ ] Unicode cross-block + visual-line matrix
- [ ] history / mapping random invariants
- [ ] P0/P1/P2 regressions
- [ ] Windows 最终实机 Gate
- [ ] source-size / dependency-boundary guard
- [ ] architecture / planning / progress 同步
- [ ] 最终 CI Success

## P3 Phase Gate

P3 只有在以下条件全部满足后才能完成：

- [x] soft-wrap 是正式 GPUI layout path
- [x] wrapped paragraph 的 logical/visual coordinates 分层且可双向解析
- [x] visual navigation / selection / hit-test 实机可用
- [x] cross-block copy/cut/delete + structured clipboard 可 undo
- [ ] typing history grouping 与 IME history interaction 可预测
- [ ] collapsed StoredMarks 生效且不污染 canonical document
- [ ] HardBreak / CodeBlock multiline 完成长期 contract 决策
- [ ] accessibility projection seam 成立
- [ ] realistic persistence/change/focus + multi-editor fixture 成立
- [ ] Unicode cross-block + undo/redo invariants 全绿
- [ ] Host Contract 无产品专用类型
- [ ] 最终 CI Success 全绿

## Regression Log

- P3.1 初版先后暴露 GPUI wrapped-layout API、旧单行 `EntityInputHandler` geometry、rustfmt 与 private-interface / dropping-reference Clippy 问题；均在 PR #41 内修复，最终 CI run #151 全绿。
- P3.2 初版先由 rustfmt 校正 wrapped geometry / visual navigation 排版，再由 Clippy 暴露 P2 block-level vertical helper dead code；清理后 CI run #159 全绿。
- P3.2 scroll-to-caret 接入时 CI run #160 暴露未落盘 module 与旧 helper 残留调用；修复后 scroll path 统一依赖 shared `ScrollHandle`、`BlockTextLayout` 与当前 document selection。
- P3.2 Windows 首轮实机 Gate 暴露 structural focus routing 时序与每帧 scroll-to-caret 抢占用户滚动；两项均在 #42 内修复，复测通过。
- P3.3 初版先建立 flat clipboard slice，再逐步补 cross-block Delete / Cut、metadata transport 与 structured paste；history regression 确认一次命令只产生一个 entry。
- P3.3 hierarchy 收口将 clipboard metadata 从 flat leaf list 升级到 v2 minimal fragment tree。首轮 CI #208 仅暴露 rustfmt 机械差异，macOS workspace tests 已通过；按日志格式化后 CI #214 在三平台、Clippy、policy 与 aggregate `CI Success` 全绿。
