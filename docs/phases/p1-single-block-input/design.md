# P1 Single Block Native Input 设计

状态：进行中

本文档是 P1 的可执行设计。顶层路线以 `docs/planning.md` 为准；已经落地的架构事实记录在 `docs/architecture.md`；P0 的契约与决策见 `docs/phases/p0-core-contract/` 与 `docs/adr/`。

P1 的目标是让晓木第一次"活起来"：在一个 GPUI 窗口里，通过真实的键盘与 IME 输入编辑单个 Paragraph block，具备 caret / local selection / undo / redo / copy / paste / 基础 marks。P1 验证的是 P0 Core contract 在真实输入管线下的可用性，而不是渲染完成度。

## 1. 范围

P1 主要发生在 `xiaomu-runtime` 与 `xiaomu-gpui`，`examples/editor_harness` 作为实机验证入口。`xiaomu-core` 原则上不动；只有当 session 层暴露出 Core contract 的真实缺口时，才允许最小化扩展，且必须在 progress.md 记录理由。

P1 必须交付：

```text
GPUI 依赖以 pinned crates.io 版本引入（单独 PR）
DocumentSession：snapshot + selection + history 编排（runtime 层）
typed 编辑 intent → Core transaction 的组装
intent-specific after-selection + ChangeMap fallback
基础 undo / redo 栈（一笔 transaction 一个 undo 单元；history grouping 留到 P3）
单 Paragraph 的 GPUI block view（渲染 + caret / selection 绘制）
键盘输入管线（Left / Right、Shift 选择、段首 / 段尾、Backspace / Delete、Enter 阻断）
最小 hit-test（点击定位 offset）
IME composition（marked text 瞬态、commit / cancel）
纯文本 clipboard（copy / cut / paste）
基础 marks 在选区上的切换（Bold / Italic / Code 等）
editor_harness 单段编辑器实机验证
```

P1 引入 GPUI 依赖，但 GPUI 类型不得进入 `xiaomu-core`，也不得进入 `xiaomu-runtime` 的公开契约。

## 2. 非目标

P1 不实现：

```text
多 block / 跨 block selection（P2 document selection）
SplitNode / JoinNodes / MoveNode / list 操作（P2/P4）
InlineAtom 交互（P4）
表格（P5）
grapheme-cluster 光标移动与视觉 affinity resolution（ADR 0001 边界，后续增强）
typing coalescing / history grouping / composition 跨组规则（P3）
富文本 / 结构化 clipboard（P3 structured clipboard）
宿主持久化与 workspace 集成（P2+ host-contract harness）
协作 / 远程 undo
virtualization
可访问性 projection（P2/P3 起步，P1 只保持 keyboard-only path 完整）
macOS / Linux IME 矩阵（长期测试矩阵，P1 先 Windows 实机）
```

P1 可以预留 seam（如 clipboard_model、decoration anchor），但不提前实现没有现实验证目标的系统。

## 3. Session 不变量

以下约束属于 P1 硬约束。

### 3.1 DocumentSession 是唯一的编排 owner

遵循 planning §9：session 持有当前 snapshot、当前 selection 与 history；所有编辑收敛为 `intent → EditPlan(Transaction + SelectionUpdate) → apply → 新 snapshot + 合法 after-selection + history 记录`。frontend 与 block view 不直接持有 canonical 状态，也不绕过 session 修改文档。

### 3.2 selection 永远指向合法位置

session 持有的 selection 在任何公开读取点必须针对当前 snapshot 校验合法。编辑 intent 不能只返回一个 Core `Transaction`，还必须携带 runtime 侧的 `SelectionUpdate`，由命令语义决定 apply 后的 selection：

```text
InsertText / paste / IME commit  → replacement 之后的 collapsed caret
Backspace / Delete              → deletion 起点的 collapsed caret
ToggleMark                      → 保留并映射原 selection
纯 caret move                   → 直接更新 selection，不产生 Transaction
非 intent 驱动的普通 apply       → MapExisting（使用 ChangeMap）
```

Core 的 `ChangeMap::map_text_selection` 用于保持旧 selection 覆盖关系，不等价于编辑命令的 after-selection：collapsed selection 在纯插入点采用 Start bias，而非空 selection 会向外覆盖 replacement。因此 runtime 必须显式表达 intent-specific selection policy，不能用一次统一映射猜测所有命令的结果。

P1 不包含结构编辑，任何会删除当前 inline node、导致 selection 映射为 `Deleted` 的 transaction 都不允许提交到 `DocumentSession`，返回 typed session error。P1 不尝试把 `TextSelection` 收敛成父级 `NodeGap`；邻近 block fallback 与 document-level selection 在 P2 统一定义。

selection resolution 和针对新 snapshot 的校验成功后，session 才原子替换 current snapshot。禁止把 stale 或未校验 selection 暴露给调用方。

### 3.3 undo 恢复精确状态与合理 selection

undo/redo 基于 `AppliedTransaction::inverse()`（ADR 0003）：round-trip 后 store 与 root 完全相等，NodeId 原样恢复。P1 的每个 `HistoryEntry` 对应一笔实际改变文档的 transaction，并保存该笔编辑的 undo/redo transaction、`before_selection` 与 `after_selection`。

undo 精确恢复旧 store 后，直接恢复并校验记录的 `before_selection`；redo 同理恢复 `after_selection`。这里不再次通过 ChangeMap 猜测 selection，因为历史 entry 已保存两个 snapshot 坐标空间里的明确结果。undo 之后发生新编辑必须清空 redo 栈。

连续输入 coalescing、history grouping 以及跨 composition 的分组边界属于 planning §16 的 P3 范围，不进入 P1 contract。IME commit 在 P1 中天然产生一笔 transaction、一个 undo 单元。

### 3.4 IME composition 不写 canonical document

marked text（composition 中的临时文本）是 frontend 瞬态，不通过伪造 TextRun 写入 `XiaomuDocument`。P1 的 `CompositionState` 位于 `xiaomu-gpui` adapter，至少包含：

```text
base_selection       composition 开始时的 canonical TextSelection
preedit_text          当前 marked text
preedit_selection     marked text 内的平台 UTF-16 selection
virtual projection    canonical prefix + preedit + canonical suffix
```

GPUI `InputHandler` 的 selected / marked / text / bounds 查询都针对 virtual projection 回答；连续 preedit update 只替换 `CompositionState`，不得增加 document revision。adapter 必须针对 P1.1 pin 的 GPUI 版本记录并测试 begin / update / commit / cancel / focus-loss callback 到状态转移的映射，不能从“收到 unmark”之类的单一信号猜测跨平台语义。

commit 将 `base_selection` 与最终文本组装为一次 `InsertText` intent，一次进入 history；cancel 丢弃 `CompositionState`，canonical document 与 session selection 回到 composition 开始前的状态。候选窗位置通过 virtual projection 中的 UTF-16 range 与布局 bounds 计算。

### 3.5 平台坐标在 adapter 边界转换

GPUI / platform 的 UTF-16 range、物理像素坐标只在 `xiaomu-gpui` 内存在，进入 session 前一律转换为 Core 类型（`TextOffset` / `TextSelection`）。UTF-8 ↔ UTF-16 转换集中在 gpui adapter 的 text boundary 转换层，不散落在 view 代码里。

### 3.6 依赖方向不变

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
examples / host
```

GPUI breaking change 只允许影响 `xiaomu-gpui`。穿透 runtime 或 core 即架构回归（planning §17）。

## 4. 初始实现策略

P1 优先保证输入管线正确、可测试、可回退，渲染与性能打磨后置。

### 4.1 DocumentSession（runtime 层）

`xiaomu-runtime` 新增 session 模块：

```text
DocumentSession
  ├─ current snapshot (XiaomuDocument)
  ├─ current selection (TextSelection，限单 inline node)
  ├─ history 栈（基础 undo / redo，一笔 transaction 一个 entry）
  └─ change notification seam（frontend-neutral 回调 trait）
```

- apply 走 `Transaction::apply_with_changes`；编辑 intent 先组装 runtime `EditPlan { transaction, selection_update }`，再用 `AppliedTransaction` 与 `SelectionUpdate` 原子产出新 snapshot、合法 after-selection 与 history entry。
- 编辑 intent（`InsertText` / `Backspace` / `Delete` / caret 移动 / mark 切换）和 `SelectionUpdate` 定义在 runtime，不进入 Core Transaction contract。
- P1 history 不做 coalescing；paste、mark 操作和 IME commit 都各自产生一个 undo entry。
- no-op intent 返回 `SessionOutcome::NoChange`，不调用 Core apply、不增加 revision、不发送 document-change notification、也不写 history。Core 的空 Transaction 会正常增加 revision，不能被当成 session no-op。
- session 纯逻辑、无 GPUI 依赖，全部行为可单元测试（CI 无显示器环境也能全绿）。

### 4.2 GPUI 依赖引入

按 planning §17：`xiaomu-gpui/Cargo.toml` 以精确版本 pin crates.io 的 gpui（`gpui = "=x.y.z"`），单独 PR，升级走单独 PR。引入时核对：

```text
tools/check_dependency_boundaries.py 的 ALLOWED 表（xiaomu-gpui → xiaomu-core + xiaomu-runtime 已声明）
cargo-deny 许可证 / advisory 是否被 gpui 传递依赖触发
docs/architecture.md 的"GPUI 尚未引入"表述同 PR 更新
```

### 4.3 Block view 与输入管线

单 Paragraph block view 负责局部渲染与 hit-test。可打印文本必须通过 GPUI `InputHandler` 进入，命令键在 gpui 层翻译为 intent 后交给 session，避免 raw key event 与 IME 形成两条文本输入路径。

P1 自动行为明确限制为：Left / Right 与 Shift+Left / Shift+Right 按 Unicode scalar boundary 移动，Home / End 到 Paragraph 逻辑首尾，Backspace / Delete 删除一个 Unicode scalar，Enter 阻断。视觉行 Up / Down、soft-wrap Home / End 与完整 keyboard navigation 属 P2。P1 对 combining sequence 只承诺 offset 合法且不 panic，不承诺按用户感知 grapheme 一次移动或删除；移动逻辑集中在输入模块，便于后续替换为 grapheme-aware 实现。

文本布局优先使用 GPUI 自带 text 能力，不提前自建 shaping 层。

### 4.4 IME 与 clipboard

IME：按 §3.4 维护 virtual projection 与 composition 状态机，platform UTF-16 range 只在 GPUI adapter 内转换；commit 才组装单笔 runtime intent。clipboard：P1 只做纯文本，经 runtime 的 clipboard seam 抽象，GPUI 侧只做平台绑定；结构化 clipboard 属 P3。

### 4.5 Error model

session 公开 API 返回 typed error，不 panic；frontend 输入路径上的合法空操作（如空 Paragraph 起点 Backspace）返回 `SessionOutcome::NoChange`。非法 transaction、`SelectionUpdate` 无法解析、selection 映射为 `Deleted` 或新 selection 校验失败属于 typed error，且 session 状态保持原子不变。

## 5. Session / Input API Surface

runtime 层初始类型：

```text
DocumentSession          编排 owner（snapshot / selection / history / notifications）
EditIntent               typed 编辑意图（InsertText / Delete / CaretMove / ToggleMark / …）
EditPlan                 Core Transaction + runtime SelectionUpdate
SelectionUpdate          intent-specific after-selection policy
HistoryStack             基础 undo / redo（一笔 transaction 一个 entry）
SessionOutcome           Changed / SelectionChanged / NoChange
SessionError             typed session 错误
```

gpui 层初始模块：

```text
input/       键盘 + IME → EditIntent；UTF-16 ↔ TextOffset 转换
block_view/  单 Paragraph 渲染 + caret / selection 绘制
hit_test/    点击 → offset
```

Cell selection、NodeSelection 编辑、跨 block selection 不进入 P1 surface。

## 6. P1 实施切片

### P1.0 Phase contract 与阶段骨架

交付：

```text
P1 design / progress 文档
P1 前置依赖归属决策记录
```

Gate：文档合入，workspace CI 全绿。

### P1.1 GPUI 依赖引入

交付：

```text
xiaomu-gpui pin crates.io gpui 精确版本
cargo-deny 策略核对 / 必要豁免
编译级 smoke（不依赖窗口环境）
architecture.md 同步
```

Gate：workspace CI 全绿（含 cargo-deny），依赖方向 guard 不变。

### P1.2 DocumentSession（runtime 编排层）

交付：

```text
DocumentSession：原子 apply + SelectionUpdate resolution + notification seam
EditIntent → EditPlan（Core transaction + after-selection policy）
HistoryStack：基础 undo / redo + before/after selection
SessionOutcome：document change / selection-only change / no-op 分流
session 单元测试（含插入/替换后的 caret、Deleted 拒绝、no-op、redo 清空、undo/redo selection 恢复）
```

Gate：session 全部行为在无 GPUI 环境下测试通过；selection 任何公开读取点合法。

### P1.3 GPUI 单块编辑基础

交付：

```text
App / window 装配
单 Paragraph block view（渲染 + caret / selection 绘制）
InputHandler 文本输入 + 命令键 → intent 管线
Left / Right、Shift 选择、Paragraph Home / End、Backspace / Delete
最小 hit-test
editor_harness 接入单段编辑器
```

Gate：实机（Windows）完成键盘编辑闭环；纯逻辑部分 CI 自动化。

### P1.4 IME composition

交付：

```text
UTF-16 range → TextOffset 转换层
CompositionState + virtual text projection
selected / marked / text / bounds 查询与连续 preedit update
begin / update / commit / cancel / focus-loss 状态转移测试
marked text 瞬态渲染（不写 canonical document）
commit 一次入历史 / cancel 恢复 composition 前状态
planning §8 Windows 矩阵：Microsoft Pinyin 连续 composition、候选窗、中文标点、中英混排、emoji / surrogate、combining marks、选区替换、焦点恢复
```

Gate：Windows 实机 IME 矩阵全过，composition 全程 document revision 不变、commit 后单笔 undo 可还原。

### P1.5 Copy/Paste 与基础 marks

交付：

```text
纯文本 clipboard（copy / cut / paste，经 clipboard seam）
选区 mark 切换（Bold / Italic / Code 等，复用 Core AddMark / RemoveMark）
undo 集成（paste / mark 操作各为一个 history entry）
```

Gate：中英文复制粘贴 + mark 切换 + undo/redo 实机闭环。

### P1.6 收官 Gate

交付：

```text
undo/redo 全链路验证（before/after selection 恢复、redo 清空）
会话级随机编辑序列不变量（复用 P0.6 xorshift 思路：随机序列保持 valid、整链 undo 回初始 store）
真实 IME + selection + undo 手动 Gate 清单执行并记入 progress.md
architecture.md / progress.md 同步，标记 P1 完成
```

Gate：planning §16 P1 Gate 满足——真实 IME + selection + undo。

## 7. 测试策略

P1 测试分两层：

```text
session / runtime 纯逻辑：单元 + 集成测试，CI 全自动化
  SelectionUpdate resolution 与 ChangeMap fallback
  intent-specific after-selection
  Deleted transaction 原子拒绝
  no-op 不增加 revision / notification / history
  undo 后新编辑清空 redo
  undo / redo round-trip（store 完全相等 + selection 合理）
  intent → transaction 组装正确性
  随机编辑序列不变量

gpui adapter：纯逻辑部分（UTF-16 ↔ TextOffset 转换、intent 翻译）自动化；
  窗口 / 实机行为走 editor_harness 手动 Gate 清单，证据记入 progress.md
```

Unicode fixture（中文 / emoji / combining marks / BiDi）必须出现在转换层与 session 测试中，与 P0 text boundary 矩阵对齐。回归测试随修复进同一 PR（CONTRIBUTING 规则）。

## 8. P1 期间的设计变更规则

小型实现细节可以直接在对应 P1 分支调整。

如果变更影响 P1 contract、切片边界或 Gate，需要同步更新本设计文档。

如果 P1 确定了未来很难逆转、会成为长期公开语义契约的决策（例如 session notification seam 或 `SelectionUpdate` 的公开形态），需要创建 ADR。

`docs/architecture.md` 只记录已经真实实现的架构；任何使其过时的 PR 必须在同一 PR 内更新它。

## 9. P1 完成定义

只有以下条件全部满足，P1 才算完成：

```text
GPUI 依赖 pinned 引入且依赖方向 guard 全绿
DocumentSession 编排 snapshot / selection / history，无绕过路径
selection 在任何公开读取点针对当前 snapshot 合法
undo / redo 恢复精确 store 状态与合理 selection
每笔文档编辑记录明确的 before/after selection，undo 后新编辑清空 redo
InsertText / paste / IME / delete / mark 的 after-selection 符合 intent 语义
Deleted selection update 原子失败，no-op 不增加 revision / notification / history
IME composition 不触碰 canonical document，commit / cancel 语义正确
单 block 键盘编辑 + hit-test + clipboard + 基础 marks 实机可用
真实 IME（Microsoft Pinyin）+ selection + undo 手动 Gate 通过
session 纯逻辑测试在 CI 无显示器环境全绿
架构文档与实现一致
CI Success 全绿
```

P2 不能用 frontend-specific 逻辑去补偿 P1 尚未解决的 session 不变量。
