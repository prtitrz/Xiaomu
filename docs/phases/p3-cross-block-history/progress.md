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

当前切片：**P3.5 HardBreak / CodeBlock Multi-line 收口**

前置状态：P0、P1、P2 均已 CLOSED。P3.0 Phase Contract、P3.1 Visual-line Geometry / Soft-wrap、P3.2 Visual Navigation / Selection、P3.3 Cross-block Editing / Structured Clipboard、P3.4 History Grouping / Stored Marks / IME 均已完成并合入 `main`。P3.5 canonical LF contract、Runtime/GPUI 实现与专项自动化 Gate 已成立，PR #47 进入 docs-only final current-head CI 与 squash merge 收口。

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
- 收口文档 head `44401225` 的 CI run #216：同样全部通过；因 #43 的 Draft 状态无法通过当时 connector 切换，最终 merge PR 为 #44，代码基线相同。
- PR #44 已 squash merge 到 `main`（`8019c03d`）。

P3.3 Gate 已关闭。

## P3.4 History Grouping / Stored Marks / IME

- [x] typing coalescing / history group
- [x] caret / selection move 断组
- [x] structural / paste / cut / mark boundary
- [x] IME updates 不写 history
- [x] IME commit 恰好一次 history commit
- [x] collapsed `StoredMarks`
- [x] normal typing / IME 对 StoredMarks 的一致继承
- [x] undo / redo selection + pending mark regression

实现事实：

```text
InsertText + HistoryPolicy::Typing
→ HistoryGroup::Typing { node, start, end }
→ same node + adjacent range + continuous selection + open group
→ merge redo in forward order / undo in reverse order
→ one undo unit

collapsed ToggleMark
→ Runtime StoredMarks
→ no canonical revision / no history entry
→ break current typing group

IME preedit/update/cancel
→ GPUI transient CompositionState only
→ no Runtime selection mutation / no history

IME commit
→ CommitComposition { range, text }
→ StoredMarks-aware ReplaceText
→ isolated history entry
```

- typing grouping 完全由 Runtime 显式 `HistoryPolicy / HistoryGroup` 决定，不使用隐藏时间阈值。只有 collapsed、非空、同一 node、前后插入 range 相邻且 recorded selection 连续的 typing 才允许合并。
- caret / selection movement 会清除 StoredMarks 并关闭 typing group；paste、cut、mark、structural command、raw apply、undo / redo 都是明确 boundary。`SplitBlock` 关闭 group，但按编辑器格式继承语义保留 StoredMarks 到新 tail block。
- collapsed `ToggleMark` 不制造空 `TextRun`，也不推进 `DocumentRevision`。`StoredMarks = None` 表示采用 Core surrounding-run inheritance；`Some(empty)` 能显式覆盖周围已存在的 mark。
- StoredMarks 的 boundary inheritance 与 Core `ReplaceText` 保持一致：run 边界优先左侧 run，offset 0 使用首 run，末尾使用最后 run。
- normal typing 与 IME commit 共用 exact StoredMarks application。IME cancel 只丢弃 frontend preedit，因为 Runtime selection 从未在 composition 期间移动，所以 pending marks 不会被错误清除。
- grouped history entry 保留第一笔 `before_selection` 和最后一笔 `after_selection`；undo / redo 后 StoredMarks 清空，避免 pending formatting 从历史操作中泄漏。
- plain platform paste 通过独立 `PasteText` intent，不再伪装普通 `InsertText`，因此不会与前后 typing coalesce。

验证证据：

- `history_stored_marks.rs` 覆盖连续 ASCII/CJK/emoji typing coalescing、caret/selection movement、mark boundary、explicit unmark、SplitBlock inheritance、undo/redo pending marks、IME commit 与 plain-text paste boundary。
- 旧 P1 `undo_redo_round_trip_restores_stores_and_selections` 已升级到 P3.4 contract：`A` + `B` 连续 typing 是一个 entry，Backspace 是第二个 isolated entry；undo / redo 继续精确恢复 store 与 selection。
- PR #45 code head `e02dd174` 的 CI run #229：policy、source-size、dependency boundary、fmt、Clippy、workspace tests、Windows/macOS/Linux 与 aggregate `CI Success` 全绿。
- 补充 collapsed mark command 断组专项回归与架构文档后，#45 current head `f495d061` 的 CI run #235 再次全绿。因 Draft → ready connector 的 GraphQL schema 兼容故障，#45 未合并；最终 merge PR #46 的 current head `e151d06d` 在 CI run #238 再次全绿，并 squash merge 到 `main`（`f8820247`）。

P3.4 Gate 已关闭。

## P3.5 HardBreak / CodeBlock Multi-line

- [x] ADR：HardBreak canonical representation
- [x] 明确 soft-wrap / HardBreak / CodeBlock newline 三者边界
- [x] Shift+Enter 语义
- [x] CodeBlock Enter
- [x] CodeBlock multiline paste
- [x] CodeBlock Tab / indentation
- [x] mapping / inverse regression

实现事实：

```text
soft wrap
→ GPUI visual geometry only
→ no canonical byte

Paragraph / Heading LF
→ canonical HardBreak

CodeBlock LF
→ canonical code newline

Enter on ordinary rich-text block
→ SplitBlock

Shift+Enter
→ EditIntent::insert_line_break()
→ one LF through isolated ReplaceText history

Enter on CodeBlock
→ EditIntent::insert_line_break()
→ same stable CodeBlock NodeId
```

- ADR 0004 选择 UTF-8 LF `\n` 作为晓木唯一具有 line-break 语义的 canonical inline scalar。Core 不新增 HardBreak atom、node/content variant 或 transaction step；LF 继续使用既有 `TextOffset / TextRange / ReplaceText / ChangeMap / inverse` contract。Core 原始 construction 仍可承载 CR，但 CR 不获得第二种晓木 newline 语义；平台 adapter / codec 表达 line break 时必须把 CRLF / CR 规范化为 LF。
- `EditIntent::insert_line_break()` 是 Runtime 的语义构造器，当前编译为 isolated text replacement，因此 HardBreak / CodeBlock newline 与前后普通 typing 明确断组，同时继续使用 StoredMarks exact application。
- ordinary rich-text `Enter` 继续结构 split；`Shift+Enter` 插入 LF。CodeBlock `Enter` 与 `Shift+Enter` 都插入 LF，不创建 sibling block。
- CodeBlock `Tab` 当前插入四个可见空格，不触发 list conversion / list item indent；CodeBlock `Shift-Tab` 不执行 list structural command。本阶段只固化正向 indentation，批量/反向 code indentation 可在后续 code-editing 增强中扩展。
- plain platform paste 到 CodeBlock 保留多行并执行 `CRLF / CR → LF`；普通 rich-text plain fallback 仍把 line break 折叠为空格。晓木 structured clipboard 粘入 CodeBlock 时主动降格为其 `plain_text`，丢弃 paragraph/list/mark 结构并保留 LF，防止 rich structure 泄入 code surface。
- `BlockTextLayout` 继续把 logical lines 间的一个 canonical LF byte 计入坐标。hard newline 两侧是两个不同 `TextOffset`；只有 soft-wrap 才允许同一 logical offset 通过 `CursorAffinity` 对应两个视觉 caret state。
- harness fixture v2 已能把 inline LF 转义为 `\n` 并精确恢复，Paragraph HardBreak 与 CodeBlock newline 均可 persistence round-trip。

验证证据：

- `line_break_mapping.rs` 覆盖 LF 插入 seam 的 Start/End mapping、后续 position +1 byte、Paragraph/CodeBlock exact inverse round-trip 与 LF scalar boundary。
- `hardbreak_codeblock.rs` 覆盖同 NodeId HardBreak、CodeBlock newline、Backspace 删除单个 LF、isolated history、undo/redo、StoredMarks 与 clipboard projection。
- GPUI `block_view::tests` 覆盖 LF 原样进入 display projection、style segment byte offset 与 IME 在 HardBreak 后的 virtual projection；`navigation.rs` 覆盖 LF 前后两个合法 caret 以及 Right/Left 1↔2。
- harness fixture regression 覆盖 Paragraph `alpha\nbeta` 与多行 CodeBlock 的 v2 save/parse semantic round-trip。
- PR #47 code head `539c190e` 的 CI run #251：policy、source-size、dependency boundary、fmt、Clippy、workspace tests、Windows/macOS/Linux 与 aggregate `CI Success` 全绿。

P3.5 实现与自动化 Gate 已满足；PR #47 剩余 docs-only current-head CI 与 squash merge。Windows 最终交互实机矩阵仍按阶段设计统一放在 P3.7，不伪造本切片未执行的人工 Gate。

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
- [x] typing history grouping 与 IME history interaction 可预测
- [x] collapsed StoredMarks 生效且不污染 canonical document
- [x] HardBreak / CodeBlock multiline 完成长期 contract 决策
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
- P3.4 第一轮 CI #221 只暴露 `history.rs` / 新测试的 rustfmt 差异；修正后进入真实 workspace regression。
- P3.4 CI #224 / #227 暴露旧 P1 history 测试仍假定“每次 InsertText 都是独立 undo entry”。实际 `A`、`B` 已按 P3.4 contract 正确 coalesce；更新旧回归为 grouped typing + isolated Backspace 后，CI #229 三平台、Clippy、policy 与 `CI Success` 全绿。
- P3.5 CI #242 首先只暴露新增 Core/Runtime/GPUI regression 的 rustfmt 差异；修正后 CI #245 进入 Clippy，并指出两处 test slice 上无意义的 `as_ref()`。移除后继续补 persistence、display projection、structured-to-code flatten 与 LF navigation regression；code head CI #251 三平台、Clippy、policy 与 `CI Success` 全绿。
