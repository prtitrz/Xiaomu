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

当前切片：**P3.0 Phase Contract**

前置状态：P0、P1、P2 均已 CLOSED。P2 最终收官 PR #39 已 squash merge，document tree / structural edit / minimal host-contract Gate 已完成。

## P3.0 Phase Contract

- [x] 创建 P3 `design.md`
- [x] 创建 P3 `progress.md`
- [x] 固化 P2 → P3 handoff
- [x] 将 `roadmap-gap-review.md` 中 visual-line / structured clipboard / stored marks / accessibility / multiline 结论并入阶段契约
- [ ] source-size baseline 复核
- [ ] dependency-boundary baseline 复核
- [ ] fmt / clippy / workspace tests / CI Success

P3.0 不改生产代码。其 Gate 是先把 P3 的视觉布局、document-level 编辑与 history 边界定清楚，避免 P3.1 开始后边做边改阶段含义。

## P3.1 Visual-line Geometry / Soft-wrap

- [ ] block `TextLayout` abstraction
- [ ] soft-wrap shaping
- [ ] visual line logical ranges
- [ ] logical offset → visual caret geometry
- [ ] visual point → logical offset hit-test
- [ ] layout cache 从 single `ShapedLine` 升级
- [ ] Unicode/CJK/emoji/combining/BiDi fixture
- [ ] Windows 实机窄宽 paragraph soft-wrap smoke Gate

## P3.2 Visual Navigation / Selection

- [ ] transient `desired_x`
- [ ] x-preserving Up / Down
- [ ] 跨 block visual Up / Down
- [ ] visual Home / End
- [ ] wrapped selection multi-rect projection
- [ ] wrapped mouse click / drag hit-test
- [ ] scroll-to-caret
- [ ] Windows 长段落实机 Gate

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

- [ ] Unicode cross-block matrix
- [ ] history / mapping random invariants
- [ ] P0/P1/P2 regressions
- [ ] Windows 最终实机 Gate
- [ ] source-size / dependency-boundary guard
- [ ] architecture / planning / progress 同步
- [ ] 最终 CI Success

## P3 Phase Gate

P3 只有在以下条件全部满足后才能完成：

- [ ] soft-wrap 是正式 GPUI layout path
- [ ] wrapped paragraph 的 logical/visual coordinates 分层且可双向解析
- [ ] visual navigation / selection / hit-test 实机可用
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

（空）