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

当前切片：**P3.2 Visual Navigation / Selection**

前置状态：P0、P1、P2 均已 CLOSED。P3.0 Phase Contract 已通过 CI 并 squash merge；P3.1 Visual-line Geometry / Soft-wrap 已完成代码、Windows 实机 Gate，并通过 PR #41 squash merge 到 `main`（`fe266de4`）。

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
- [x] scroll-to-caret
- [x] `CursorAffinity` 在 soft-wrap boundary 的视觉解析
- [~] final current-head fmt / clippy / workspace tests / three-platform CI
- [ ] Windows 长段落实机 Gate

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
- `DocumentView` 持有一个 GPUI `ScrollHandle`；focused block 用 wrapped caret window bounds 请求最小纵向滚动。collapsed caret、Shift 扩选的 focus endpoint、IME preedit caret 都走同一 scroll-to-caret 路径。

验证证据：

- 在 scroll-to-caret 接入前，PR #42 head `1f5443b5` 的 CI run #159 已达到 policy、fmt、clippy、workspace tests、Windows/macOS/Linux 与 `CI Success` 全绿。
- scroll-to-caret 接入过程的 CI run #160 暴露了一个未创建的 `scroll.rs` module 与两个已删除 P2 helper 的残留调用；已补齐独立 `block_view/scroll.rs` 并改为直接使用当前 `BlockTextLayout / DocumentSelection` geometry。
- 当前 PR #42 仍保持 Draft；最终 current-head CI 与 Windows 实机 Gate 通过前不合并。

## P3.3 Cross-block Editing / Structured Clipboard

- [ ] cross-block delete
- [ ] cross-block copy / cut
- [ ] frontend-neutral structured clipboard payload
- [ ] plain-text fallback
- [ ] structured paste
- [ ] one-command one-history-entry 原子语义
- [ ] list / heading / paragraph 跨结构 selection regression
- [ ] mapping + after-selection regression

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
- [~] visual navigation / selection / hit-test：实现已完成，等待 P3.2 Windows 实机 Gate
- [ ] cross-block copy/cut/delete + structured clipboard 可 undo
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
