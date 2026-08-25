# P2 Document Tree / Structural Edit 设计

状态：进行中

本文档是 P2 的可执行设计。顶层路线以 `docs/planning.md` 为准；已经落地的架构事实记录在 `docs/architecture.md`；P0 / P1 的契约与决策见对应 `docs/phases/` 与 `docs/adr/`。

P1 让晓木在真实输入管线中编辑单个 Paragraph。P2 把编辑对象从"一个 inline node"升级为"整棵 document tree"：multi-block 文档、SplitNode / JoinNodes 等结构 transaction、heading / quote / list、跨 block 键盘导航与 document selection、position mapping 稳定化，以及第一个 minimal host-contract harness。

P2 Gate（planning §16）：paragraph → list → paragraph 日常编辑闭环，且最小宿主可加载、监听变更并保存文档。

## 1. 范围

P2 横跨三个 crate：`xiaomu-core` 新增结构 transaction steps；`xiaomu-runtime` 将 session 从"单 inline selection"升级为 document-level selection 并编排结构命令；`xiaomu-gpui` 从单块 view 升级为 multi-block 视图与跨块导航。`examples/editor_harness` 继续作为实机验证入口，并按 planning §13.1 扩展为 minimal host-contract harness。

P2 必须交付：

```text
Core：SplitNode / JoinNodes 结构 step（mapping + inverse 完整）
Core：list 相关结构能力（WrapList / UnwrapList 或等价组合，见 §3.4）
Runtime：document-level selection（跨 block anchor/focus）取代"限单 inline node"约束
Runtime：结构编辑 intent + after-selection fallback policy（取代 P1 的 Deleted 即失败）
Runtime：Enter / Backspace-at-start / Tab / Shift-Tab 等结构命令编排
GPUI：multi-block 渲染（block list 布局、per-block view、滚动）
GPUI：跨 block 键盘导航（Left/Right 越界、Up/Down 视觉行、Home/End）
GPUI：跨 block selection 的渲染投影与鼠标拖选（planning §10：visual range 只投影到 mounted blocks）
heading / quote / list 的日常编辑闭环（含 Markdown 式快捷转换的可选切片）
position mapping 回归矩阵（split / join / remove / restore 组合下的 selection 稳定性）
minimal host-contract harness：create editor → load document → listen to changes → persist through adapter
editor_harness multi-block 编辑实机验证
```

## 2. 非目标

P2 不实现：

```text
cross-block copy / cut / delete 与 structured clipboard（P3）
history grouping / typing coalescing / composition-history 交互（P3）
InlineAtom 及 renderer registry（P4）
表格（P5）
grapheme-cluster 光标与 BiDi affinity resolution（ADR 0001 边界，后续增强；P2 沿用 scalar boundary + 单行视觉模型）
完整 virtualization（保持 planning §10.1 readiness seam，不做 mounted-window 管理）
可访问性 projection（frontend seam 在 P2/P3 起步即可）
协作 / 远程 undo（collaboration-neutral 立场不变）
Markdown codec 生产化（codec-markdown 维持 bootstrap；host harness 的持久化走 adapter fixture，不绑定 Markdown round-trip 质量）
```

## 3. 关键设计决策

以下决策在本阶段契约中确定方向；实现期间如需推翻，必须同步更新本文档并记录理由。

### 3.1 Document selection 属于 runtime session 层

沿用 P0 决策："跨 block selection 属于后续 session 层"。Core 的 `TextSelection` / `NodeSelection` / `NodeGap` 保持现有语义不变；runtime 引入 document-level selection 类型（暂名 `DocumentSelection`），由 anchor / focus 表达，每个端点是 `TextPoint` 或 `NodeGap` 级位置。

```text
DocumentSelection
  ├─ anchor: DocumentPosition
  ├─ focus:  DocumentPosition
  └─ ordered(): 归一化为 head/tail，供渲染与命令使用
```

- 单 block 内的 selection 仍以 Core `TextSelection` 校验语义为准；DocumentSelection 是它的超集，不重复发明 text boundary 校验。
- session 公开读取点校验规则升级为：selection 的每个端点针对当前 snapshot 合法；不再要求两端同属一个 inline node。
- 渲染投影（visual range → mounted blocks）是 GPUI 层职责，DocumentSelection 本身不携带任何视觉信息。

### 3.2 结构编辑的 after-selection 用显式 fallback policy 取代"Deleted 即失败"

P1 的停损策略（导致当前 inline node 被删的 transaction 一律原子失败）在 P2 失效——JoinNodes 天然会删除节点。session 为结构命令定义显式、可测试的 after-selection policy：

```text
JoinNodes        → caret 落在 join 点（前块的接缝 offset）
RemoveNode       → caret 收敛到被删节点的逻辑邻接位（前一兄弟边界或父级 NodeGap）
SplitNode        → caret 落在新节点起点
fallback 失败    → 若收敛后的位置仍无法合法化，才允许原子失败并返回 typed error
```

policy 在 runtime intent 层逐一定义并用 ChangeMap + 显式构造表达，禁止 frontend 自行 clamp。所有 fallback 行为进入集成测试。

### 3.3 Core 结构 steps 最小集先行

P2 向 Core 加入的最小 step 集合：

```text
SplitNode   在指定 child index / text offset 处把一个节点拆成两个，返回新 NodeId
JoinNodes   把相邻兄弟节点合并为一个，明确内容拼接与 marks 合并规则
```

两者的 mapping（StepMap 条目）、inverse（RestoreSubtree / SplitNode 反向组合或等价精确逆）、以及与既有 ReplaceText piece 机制的交互，都必须满足 P0 已建立的随机不变量测试框架（valid 序列 + 整链 undo 还原初始 store）。MoveNode、InsertNode 的批量形态等推迟到有真实命令驱动时再加入，不为对称性提前实现。

### 3.4 List 通过既有原语 + 最小新能力实现

`BulletList / OrderedList / ListItem` kind 已存在于 Core model。P2 不新增 list 专用 transaction step（WrapList / UnwrapList 属 planning §6 的远期集合），而是：

```text
建 list     = InsertNode（list + item 容器）+ 移动段落
拆 list     = RemoveNode 子树 + RestoreSubtree 回插段落（undo 精确性由既有机制保证）
indent/outdent = MoveNode 语义的组合；若组合成本过高，再评估是否值得引入最小 MoveNode step，
              决策记入 progress.md
```

Gate 只要求 paragraph → list → paragraph 的日常编辑闭环正确且 undo 可还原，不追求嵌套列表全功能。

### 3.5 GPUI multi-block 视图遵循 planning §10 分层

```text
DocumentSession → BlockViewState（per-block）→ TextLayout cache → paint / hit-test
```

- block layout cache key 至少包含 node revision / content width / viewport constraints / typography revision（planning §10）。
- 所有 block 永远 mounted 只是过渡形态；不得把该假设固化进公开 API（§10.1 readiness seam）。
- 跨 block selection 由 frontend 投影到各 mounted block 分别绘制，不写回 canonical state。
- 焦点模型：同一时刻至多一个 block 持有键盘焦点；caret 进入相邻 block 的边界导航由 gpui input 层翻译为 session 级 intent，而不是各 view 私自改文档。

### 3.6 Minimal host-contract harness 按 planning §13.1 定义

harness 是仓库内的 realistic fixture，验证宿主四件事：

```text
create editor
load document（从内存 fixture / 简单 adapter 读入，不经 Markdown codec 生产路径）
listen to changes（订阅 DocumentChangeListener）
persist through adapter（把 canonical snapshot 写出，格式为 harness 内部约定，不承诺 codec 质量）
```

它验证的是 Host Contract 方向与 seam 是否够用，不是产品化宿主；不引入任何产品专用类型。

## 4. 初始实现策略

结构正确性与 mapping/inverse 精确性优先，视觉打磨后置；每一步都保持 workspace CI 全绿与依赖 guard 不变。

### 4.1 Core 先行

SplitNode / JoinNodes 先以纯 Core step 落地，配齐：

```text
step 构造校验（kind 兼容、index 合法、offset 合法）
application 引擎接入 + full validation
StepMap 条目（child boundary 平移 / 新 NodeId 记录）
inverse 精确还原（store 相等语义与 P0 一致）
随机不变量测试扩展到新 step
```

### 4.2 Runtime session 升级分两步走

先引入 DocumentSelection 与新的校验规则（纯数据结构 + 测试，不动命令面），再迁移结构 intent。避免一次性重写 session 造成中间态不可测。P1 的全部行为（after-selection、no-op、undo selection 恢复）必须在升级后保持回归通过。

### 4.3 GPUI 渐进替换

单 Paragraph view 不推倒重来：保留其 shape_line / paint / hit-test 能力，外层新增 block list 容器与焦点路由。IME composition 的 base range 可能跨 block 的场景在 P2 明确停损（composition 限制在单 block 内启动，跨块 preedit 属后续增强），并在文档中记录。

### 4.4 Error model

沿用 P1：typed error、不 panic、失败原子回滚。新增结构相关错误（如 join 目标不可合并、list 操作违反容器不变量）进入 Core/Runtime typed error 集。

## 5. Session / Input API Surface（增量）

runtime 层新增 / 变更：

```text
DocumentSelection      跨 block selection（anchor/focus）
DocumentPosition       TextPoint | NodeGap 级端点
EditIntent 扩展        SplitBlock / JoinWithPrevious / TurnInto(kind) / IndentList / OutdentList / …
AfterSelectionPolicy   显式 fallback 规则集（§3.2）
SessionOutcome/Error   保持三分流与 typed error，错误枚举扩展
```

gpui 层新增 / 变更：

```text
document_view/         block list 容器、滚动、焦点路由
block_view/            保留并适配多实例化（per-block state、cache key）
input/                 跨块导航键 → intent；composition 跨块停损
hit_test/              block 级命中 → 定位到具体 block 再定位 offset
```

## 6. P2 实施切片

切片边界可在执行中按实际暴露的问题微调；调整需同步本文件。

### P2.0 Phase contract 与阶段骨架

交付：

```text
P2 design / progress 文档
P1 → P2 前置依赖归属决策记录
guard + fmt + clippy + test 全绿确认
```

Gate：文档合入，workspace CI 全绿。

### P2.1 Core 结构 steps

交付：

```text
SplitNode / JoinNodes step + application + validation
StepMap 条目与 inverse 精确还原
随机不变量测试覆盖新 step
```

Gate：CI 全绿；round-trip store 相等；mapping 语义有测试锚定。

### P2.2 Runtime DocumentSelection 与 session 升级

交付：

```text
DocumentSelection / DocumentPosition 数据结构与校验
session selection 读写点切换到 document-level 校验
P1 全部行为回归通过（after-selection / no-op / undo selection 恢复）
```

Gate：无 GPUI 环境下测试全绿；P1 集成测试不改断言即通过（或变更均有记录的理由）。

### P2.3 结构命令编排

交付：

```text
结构 EditIntent（SplitBlock / JoinWithPrevious / TurnInto 等）+ AfterSelectionPolicy
Enter / Backspace-at-start 命令流（含 marks 继承规则：split 后新块继承原 marks）
undo / redo 对结构命令的语义验证
```

Gate：纯逻辑测试覆盖 split/join 的 selection fallback 与 history 行为。

### P2.4 List 编辑

交付：

```text
list 构建 / 拆解 / indent-outdent 命令流
TurnInto(Paragraph/BulletList/OrderedList) 闭环
undo 还原验证
```

Gate：paragraph → list → paragraph 纯逻辑闭环 + undo 可还原。

### P2.5 GPUI multi-block 渲染与导航

交付：

```text
document_view block list 容器 + 滚动 + 焦点路由
per-block view 多实例化与 layout cache key
跨 block Left/Right/Home/End/Up/Down 导航
跨 block selection 投影绘制与鼠标拖选
heading / quote 的视觉区分
```

Gate：实机（Windows）multi-block 键盘编辑闭环可用。

### P2.6 Minimal host-contract harness

交付：

```text
load document / listen to changes / persist through adapter fixture
editor_harness 接入 multi-block 编辑器与持久化演示
```

Gate：harness 内完成 create → load → edit（变更可见）→ save 流程。

### P2.7 Position mapping 稳定化与收官 Gate

交付：

```text
mapping regression matrix：split/join/remove/restore 组合下的 selection/decoration 映射
会话级随机结构编辑序列不变量（复用 xorshift 思路）
paragraph → list → paragraph 实机手动 Gate 清单执行并记入 progress.md
architecture.md / progress.md 同步，标记 P2 完成
```

Gate：planning §16 P2 Gate 满足——日常编辑闭环 + 最小宿主加载 / 监听 / 保存。

## 7. 测试策略

```text
core：新 step 的单元 + 随机不变量（valid 序列、round-trip store 相等、mapping 合法坐标）
runtime：DocumentSelection 校验、结构 intent → plan、AfterSelectionPolicy、undo/redo、P1 回归
gpui 纯逻辑：导航键翻译、hit-test 分发、cache key 计算
实机：Windows 手动 Gate 清单（multi-block 输入 / IME 单块停损确认 / 导航 / list 闭环 / harness 持久化）
```

Unicode fixture（中文 / emoji / combining marks）继续出现在涉及文本拼接的 split/join 测试中。回归测试随修复进同一 PR。

## 8. P2 期间的设计变更规则

与 P1 相同：小型实现细节直接调整；影响 contract / 切片边界 / Gate 的变更同步更新本文档；形成长期公开语义契约的决策创建 ADR；任何使 architecture.md 过时的实现必须在同一 PR 更新它。

## 9. P2 完成定义

只有以下条件全部满足，P2 才算完成：

```text
SplitNode / JoinNodes 以 Core step 落地，mapping + inverse 满足随机不变量
DocumentSelection 成为 session 的 selection 形态，所有公开读取点针对当前 snapshot 合法
结构命令的 after-selection fallback 显式、可测试，不再依赖"Deleted 即失败"停损
list 日常编辑闭环（建立 / 缩进 / 退出 / 回到 paragraph）undo 可还原
multi-block 渲染 + 跨块导航 + 跨块 selection 实机可用
minimal host-contract harness 完成 load / listen / persist 闭环
position mapping regression matrix 建立并通过
P1 全部 session 行为保持回归通过
架构文档与实现一致
CI Success 全绿
```

P3 不能用 frontend-specific 逻辑补偿 P2 尚未解决的 selection / mapping 不变量。
