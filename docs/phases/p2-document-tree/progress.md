# P2 Document Tree / Structural Edit 进度

状态：进行中

本文档只记录 P2 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P2 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P2.0 Phase contract**

前置状态：P0 已完成（PR #13）；P1 已全部完成并关闭（PR #14–#20）。

## P2.0 Phase Contract 与阶段骨架

- [x] 创建 `docs/phases/p2-document-tree/design.md`
- [x] 创建 `docs/phases/p2-document-tree/progress.md`
- [x] 记录 P1 移交的 P2 前置依赖归属（见下方决策记录）
- [x] 运行 source-size 与 dependency-boundary guard
- [x] 运行 `cargo fmt --all -- --check`
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 运行 `cargo test --workspace --all-targets`

完成证据：

```text
分支 docs/p2-phase-contract：
uv run python tools/check_source_size.py 全绿
uv run python tools/check_dependency_boundaries.py 全绿
cargo fmt / clippy -D warnings / cargo test 全绿
本 PR 的远端 CI Success 即 P2.0 Gate 证据。
```

## P1 移交的 P2 前置依赖与归属

P1 在 progress.md 与 design.md 中记录了若干"留到 P2"的事项，处置如下：

```text
1. 跨 block selection 属于 session 层
   → P2 引入 runtime DocumentSelection（anchor/focus 端点为 TextPoint 或 NodeGap 级），
     Core TextSelection / NodeSelection 语义不变；渲染投影属 GPUI 层。
2. "Deleted 即原子失败"的 selection fallback 停损
   → P2 结构命令需要显式 AfterSelectionPolicy（join 点 / 邻接位收敛等），
     取代无差别停损；fallback 无法合法化才允许 typed error 原子失败。
3. SplitNode / JoinNodes / MoveNode / list steps 未实现
   → P2.1 落地最小集 SplitNode / JoinNodes；MoveNode 仅在 indent/outdent 组合成本
     过高时评估引入；list 通过 InsertNode/RemoveNode/RestoreSubtree 组合实现。
4. 视觉行导航与 grapheme 光标
   → P2 提供跨 block Left/Right/Home/End/Up/Down 导航（仍按 scalar boundary、
     单行视觉模型）；grapheme cluster 与 BiDi affinity 维持 ADR 0001 后续增强边界。
5. IME composition 跨 block preedit
   → P2 明确停损：composition 只在单 block 内启动，跨块 preedit 属后续增强，
     在实现切片中记录并测试该约束的行为。
6. 宿主集成（planning §13.1 从 P2 开始）
   → P2.6 minimal host-contract harness：load document / listen to changes /
     persist through adapter，不绑定 Markdown codec 生产质量，不引入产品专用类型。
```

## 决策记录

这里只记录影响 P2 执行的决定。长期且难逆转的架构理由应进入 ADR。

### 2026-08-25（P2.0）

- Document selection 放在 runtime session 层（DocumentSelection），不扩展 Core selection 类型——沿用 P0 "跨 block selection 属 session 层"决策，避免 Core contract 为 UI 形态让步。
- 结构编辑 after-selection 用逐 intent 显式 policy 定义，取代 P1 的全局"Deleted 即失败"；这是行为语义变化，P1 回归测试需按新契约审视后迁移。
- Core 最小 step 集 = SplitNode + JoinNodes；WrapList/UnwrapList 不在本阶段引入，list 编辑用既有原语组合，indent/outdent 是否需要 MoveNode 由 P2.4 以实际成本决定。
- GPUI multi-block 采用"保留单块 view 能力 + 外层容器/焦点路由"的渐进替换，不重写渲染路径；所有 block 永远 mounted 只是过渡形态，公开 API 不得固化该假设（planning §10.1）。
- IME composition 在 P2 停损为单块内启动；跨块 preedit 不做。

## P2 Phase Gate

P2 只有在以下条件全部满足后才能完成：

- [ ] SplitNode / JoinNodes 以 Core step 落地，mapping + inverse 满足随机不变量
- [ ] DocumentSelection 成为 session 的 selection 形态，公开读取点全部校验
- [ ] 结构命令 after-selection fallback 显式且可测试
- [ ] list 日常编辑闭环 undo 可还原
- [ ] multi-block 渲染 + 跨块导航 + 跨块 selection 实机可用
- [ ] minimal host-contract harness 完成 load / listen / persist 闭环
- [ ] position mapping regression matrix 建立并通过
- [ ] P1 全部 session 行为回归通过
- [ ] 架构文档与实现一致
- [ ] P2 最终 `CI Success` 全绿

## Regression Log

（空）
