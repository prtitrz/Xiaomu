# P2 Document Tree / Structural Edit 进度

状态：**已完成 / CLOSED**

本文档只记录 P2 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P2 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P2 已完成并关闭。后续功能变更进入 P3，不再扩张 P2 范围。**

前置状态：P0 已完成；P1 已完成并关闭；P2.0–P2.7 功能实现已合入 `main`，PR #38 完成 P2.7 最后一轮功能收口与 Windows 实机 Gate；PR #39 完成 persistence fail-closed 与收官文档同步。

## P2.0 Phase Contract 与阶段骨架

- [x] P2 design / progress 文档建立
- [x] P1 → P2 前置依赖归属明确
- [x] source-size / dependency-boundary guard 接入并持续执行
- [x] fmt / clippy / workspace tests 持续作为 CI Gate

结果：P2 的范围、非目标、分层与 Phase Gate 在实现前固定，后续切片没有把 P3 能力倒灌进 P2。

## P2.1 Core 结构 steps

- [x] `SplitNode`：inline 节点在合法 Unicode scalar boundary 拆分，tail 获得新 NodeId
- [x] `JoinNodes`：相邻 inline 兄弟合并，保留 first identity
- [x] `StepMap::NodeSplit / NodeJoined` 与 parent child-boundary 映射
- [x] inverse 精确还原，随机 valid transaction 不变量覆盖结构 step
- [x] `SetNodeKind` 后续加入 Core，保留 NodeId / attrs / content 并校验 shape 与 parent compatibility

关键结果：结构 mutation 继续只通过 typed transaction；mapping 与 inverse 由 application 产出，没有建立第二套隐式 offset 修补机制。

## P2.2 Runtime DocumentSelection

- [x] `DocumentSelection / DocumentPosition` 成为 session selection 形态
- [x] anchor / focus 可跨 inline block，并保留方向
- [x] `validate / ordered / map_through / as_single_node` 语义有纯逻辑测试
- [x] session 公开读取点始终对当前 snapshot 重新校验 selection
- [x] P1 单块能力经 `text_selection()` 投影保持兼容

关键结果：Core selection contract 没有为了 UI 跨块能力扩张，document-level selection 留在 Runtime。

## P2.3 结构命令编排

- [x] `SplitBlock / JoinWithPrevious / TurnInto` intent
- [x] 结构命令使用显式 after-selection policy
- [x] Enter split / Backspace-at-start join
- [x] Paragraph / Heading / CodeBlock 同 shape 转换
- [x] undo / redo 保留结构 identity 与 recorded selection

主要 selection policy：

```text
SplitBlock        → caret 到 split tail 起点
JoinWithPrevious  → caret 到 join seam
TurnInto          → MapExisting
结构移动          → PreserveFocus / intent-specific fallback
fallback 无法合法化 → typed error，session 原子失败
```

## P2.4 List 编辑

- [x] Paragraph → BulletList / OrderedList
- [x] BulletList ↔ OrderedList
- [x] List item → Paragraph，完成 paragraph → list → paragraph 闭环
- [x] `IndentListItem / OutdentListItem`
- [x] list item 块首 Backspace：前项合并、嵌套 outdent、顶层 lift-out
- [x] Tab / Shift-Tab 的段落与 list 语义形成可理解闭环
- [x] staged plan 把多阶段结构操作合并成一笔 history
- [x] undo / redo 恢复 contracted identity

结论：P2 没有为 list 新增 Core 专用 transaction step。`InsertNode / RemoveNode / RestoreSubtree / SetNodeKind / SplitNode / JoinNodes` 足以表达当前 Gate。

## P2.5 GPUI multi-block

- [x] `DocumentView` 多块容器、滚动与焦点路由
- [x] per-block `ParagraphView` 多实例化与 NodeId 池化复用
- [x] layout cache key = node + editing epoch + rounded width
- [x] Left / Right / Home / End 跨块导航
- [x] Up / Down 跨块导航，最终候选始终解析到合法 Unicode scalar boundary
- [x] Shift keyboard selection 跨块
- [x] mouse drag selection 跨块
- [x] heading / quote 视觉投影
- [x] BulletList / OrderedList marker / ordinal frontend projection
- [x] marker 不进入 canonical text / selection offset

当前边界保持明确：P2 仍是单视觉行模型；soft-wrap、x-preserving vertical navigation、grapheme/BiDi visual affinity 留给 P3 或后续阶段。

## P2.6 Minimal host-contract harness

- [x] Runtime `DocumentPersistence` seam
- [x] `load() -> Result<Option<XiaomuDocument>, PersistenceError>`
- [x] NotFound → `Ok(None)`；I/O / parse failure → `Err`
- [x] GPUI Ctrl/Cmd-S 经 adapter 保存当前 canonical snapshot
- [x] `EditorHooks` 接入 persistence 与 `DocumentChangeListener`
- [x] harness create → load → edit/listen → save 闭环
- [x] fixture v2 round-trip 保留 tree shape、inline runs、MarkSet、Link attrs 与 NodeAttrs
- [x] unsupported atomic / custom node save 时 fail closed，不允许静默丢 canonical 数据

fixture 仍是 harness-private 格式，不是公共 codec。P2 只要求它能可靠证明 host seam；尚未编码的 node kind 必须明确报错，而不是假装保存成功。

## P2.7 Closeout

- [x] list Enter：非空 item 创建 sibling `ListItem`
- [x] empty list item Enter：退出当前 list level
- [x] Unicode Up / Down boundary regression
- [x] Bullet / ordered marker projection
- [x] persistence load error semantics
- [x] fixture marks / attrs fidelity
- [x] session / structural mapping matrix
- [x] structural invariants / history regression
- [x] release 构建中 Gate-era diagnostics 仅保留真实错误
- [x] source-size / dependency-boundary guard 复核
- [x] Windows 最终实机 Gate
- [x] 收官复核补充 unsupported atomic / custom persistence fail-closed
- [x] 收官 PR `CI Success`

## Windows 最终实机 Gate

PR #38 已记录真实 Windows 环境 Gate 完成，覆盖：

```text
multi-block direct input
Microsoft Pinyin in different blocks
Left / Right cross-block
Up / Down cross-block，含中英 / emoji
Shift keyboard selection cross-block
mouse drag selection cross-block
Enter split / Backspace join
paragraph → list
list Enter creates sibling item
empty list item Enter exits current level
bullet / ordered marker 可见且不同
indent / outdent → paragraph
undo / redo structural edits
Ctrl+S save
restart + load
listener observes committed changes
```

## P1 移交事项处置

```text
跨 block selection
→ Runtime DocumentSelection，已完成

Deleted 即全局失败的结构 selection 停损
→ intent-specific after-selection policy，已完成

SplitNode / JoinNodes / list structural capability
→ Core minimal steps + Runtime staged plan，已完成；未引入 MoveNode

视觉行导航 / grapheme caret
→ P2 完成 scalar-safe 单视觉行跨块导航；visual-line / grapheme 留 P3+

IME composition 跨块 preedit
→ P2 明确 composition 仅在单 block 内启动，保持后续增强边界

宿主集成
→ minimal host-contract harness 已完成
```

## 关键决策记录

### DocumentSelection 边界

跨块 selection 属 Runtime session，不扩张 Core `TextSelection / NodeSelection / NodeGap` 的职责。

### List step 语言

P2 未新增 WrapList / UnwrapList / MoveNode。list 编辑通过 Core 已有通用结构原语和 Runtime staged plan 表达。实际实现证明组合成本可控，因此没有为 API 对称性增加 Core step。

### Redo identity

redo 重放 `inverse(inverse(T))`，不直接重放会重新分配 NodeId 的原始 `SplitNode`。因此结构命令 redo 可以恢复记录过的 identity 与 after-selection。

### Persistence seam

Runtime 只定义 canonical snapshot 的 load/save seam；格式、存储介质、触发策略属于 host adapter。harness fixture 不升级为公共 codec。

### Fixture fail-closed

harness fixture 只对它明确支持的 canonical node / mark / attr 表达返回成功。遇到未编码语义必须返回 `PersistenceError`，禁止 silent data loss。

## P2 Phase Gate

P2 关闭条件全部满足：

- [x] SplitNode / JoinNodes 以 Core step 落地，mapping + inverse 满足随机不变量
- [x] DocumentSelection 成为 session selection 形态，公开读取点全部校验
- [x] 结构命令 after-selection fallback 显式且可测试
- [x] paragraph → list → paragraph 日常编辑闭环，undo 可还原
- [x] multi-block 渲染 + 跨块导航 + 跨块 selection Windows 实机可用
- [x] minimal host-contract harness 完成 load / listen / persist 闭环
- [x] position mapping regression matrix 与结构不变量覆盖完成
- [x] P1 session / IME / clipboard / marks 回归保持成立
- [x] list Enter / marker / Unicode navigation 收官缺口关闭
- [x] persistence 不吞 load 错误、不静默丢 unsupported canonical node
- [x] Windows 最终 Gate 完成
- [x] architecture / progress / closeout 文档同步
- [x] 收官 PR 最终 `CI Success` 全绿

P2 自 PR #39 起正式 **CLOSED**。后续能力和回归修复按 P3 及后续 Phase 的边界继续演进。

## Regression Log

P2 收官时无未解决 correctness regression。后续若 P3 变更破坏上述 Gate，应记录为 P3 regression，不回开 P2 范围。
