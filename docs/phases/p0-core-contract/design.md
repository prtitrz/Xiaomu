# P0 Core Contract 设计

状态：进行中

本文档是 P0 的可执行设计。顶层路线以 `docs/planning.md` 为准；已经落地的架构事实记录在 `docs/architecture.md`。

P0 的目标是在任何 Native Frontend 代码进入项目之前，先建立晓木的 canonical document semantics（规范文档语义）和 mutation contract（修改契约）。

## 1. 范围

P0 主要发生在 `xiaomu-core`。`xiaomu-testkit` 可以增加用于验证 Core 不变量的测试辅助能力。

P0 必须交付：

```text
版本化文档 schema
对外不可变的文档 snapshot
稳定且 opaque 的 NodeId
NodeStore 与 structural sharing 原型
TextRun 局部 marks 规范化
TextOffset 与 Unicode 文本边界
position / selection 基础类型
typed transaction 基础能力
StepMap / ChangeMap 原型
文档校验
inverse / change-set 原型
property / regression tests
```

P0 不引入 GPUI 依赖。

## 2. 非目标

P0 不实现：

```text
Native 渲染与输入
IME composition runtime
剪贴板集成
宿主持久化 API
完整 command / keybinding 行为
协作协议
协作 undo
生产级 virtualization
完整表格编辑
InlineAtom 交互语义
Markdown 编辑语义
```

P0 可以预留后续阶段必需的类型或 seam，但不能为了“以后可能用到”提前实现没有现实验证目标的系统。

## 3. Core 不变量

以下约束属于 P0 硬约束。

### 3.1 Canonical document state 必须是结构化文档

Markdown、HTML、JSON 等外部格式都只是 codec。任何 external source offset、Markdown byte range 或 GPUI 类型都不能成为 canonical document identity 的一部分。

### 3.2 文档对外不可变

调用方拿到的是 document snapshot，只能通过受控 API 查询。Canonical node 和 store 不公开允许绕过校验的可变字段。

修改路径统一为：

```text
Document + Transaction
        ↓
      apply
        ↓
new Document + ChangeSet / Mapping + inverse information
```

### 3.3 NodeId 稳定且 opaque

`NodeId` 是 newtype，其内部表示不属于公开语义契约。

P0 要求：

```text
节点未被删除时，编辑前后保持稳定 identity
普通调用方不能任意构造 raw NodeId
测试可以获得确定性 NodeId
NodeId 的数值顺序不得被视作文档顺序
```

第一版允许使用简单 allocator。Wire-format identity 和分布式 ID 分配留到后续阶段。

### 3.4 Canonical document 是树

文档根节点持有结构化 node tree。不能把 `Vec<BlockNode>` 一类扁平结构固化成长期公开 contract。

Node content 至少需要覆盖：

```text
Children
InlineContent
Atomic / Custom payload
```

P0 只实现足够验证模型和 transaction 的 built-in node kind。Paragraph、基础 container 等已经足够起步；后续可以在不破坏契约的前提下逐步增加顶层规划里已经定义的节点类型。

### 3.5 Text offset 必须 typed 且 Unicode-safe

`TextOffset` 是一个 text-bearing node / inline text container 内部的 opaque Core coordinate。

第一版内部采用 UTF-8 byte offset，因为 Rust `str` / `String` 使用 UTF-8；但所有构造和修改入口必须验证 UTF-8 char boundary。普通安全 API 不能产生落在 UTF-8 code point 中间的可操作 range。

UTF-16 转换属于未来 platform adapter，不属于 Core coordinate contract。

必须覆盖：

```text
ASCII
中文
中英混排
emoji / surrogate-pair 对应场景
combining marks
BiDi 文本
```

P0 不承诺 grapheme-cluster 级光标移动，但必须保证 byte boundary 与 Unicode scalar boundary 不混淆。

### 3.6 Marks 存在于 TextRun 上

Canonical mark 存储在 `TextRun` 上，不维护独立的全局 mark range table。

规范化规则：

```text
相邻且 MarkSet 相同的 TextRun 自动合并
禁止持久化空 TextRun
mark 顺序固定
冲突的重复 mark attrs 必须拒绝或明确规范化
```

TextRun 边界只是内部实现细节。Position / selection 不能暴露 run segmentation 作为用户可见坐标。

### 3.7 Transaction 是唯一 canonical mutation 路径

P0 引入 typed steps，不能靠随意 mutator 修改 canonical document。

第一批 transaction 至少覆盖：

```text
ReplaceText
InsertNode
RemoveNode
SetNodeAttrs
AddMark
RemoveMark
```

`SplitNode`、`JoinNodes`、`MoveNode`、list 专用操作和 InlineAtom 操作可以在契约足够稳定后增加，但其完整交互行为属于后续阶段。

每次 transaction apply 必须返回明确的 change information。任何子系统都不能自行偷偷修 offset。

### 3.8 Position mapping 必须显式

每个 applied step / transaction 都必须能够提供足够的 mapping 信息，把旧 position 映射到新的文档坐标空间。

P0 首先覆盖文本替换和节点插入 / 删除。

Mapping API 必须区分：

```text
位置仍存在并成功映射
目标节点已经删除
```

默认行为不能静默 clamp。

### 3.9 Inverse 行为必须可测试

对可逆 P0 operation：

```text
D1 = apply(D0, T)
D2 = apply(D1, inverse(T))
```

`D2` 必须在语义上等价于 `D0`，包括规范化后的 text / marks，以及该 operation 承诺保留的 node identity。

Inverse 可以内部表现为 inverse transaction，也可以是能生成 inverse 的 change set；P0 不提前冻结最终公开表示。

## 4. 初始实现策略

P0 优先保证正确性、可观察性和契约清晰，性能数据不足时不提前选择复杂数据结构。

### 4.1 Text storage

先用 `String` 封装在 `TextBuffer` 后面。

原因：

```text
Unicode 边界验证简单
依赖少
transaction 语义清晰
未来是否切 rope 可以由 benchmark 决定
```

未来 rope 必须适配现有语义边界，而不是反向改变公开 contract。

### 4.2 Node storage 与 snapshot

第一版采用标准库所有权原语和 node-level structural sharing，例如 immutable node 放在 `Arc` 后面，由 snapshot 自己持有 store。

P0.2 必须实际证明：生成新 revision 时，未变化节点的 payload 可以复用。

P0 不要求现在决定永久使用某个 HAMT / persistent-vector crate。如果以后 benchmark 证明 map clone 成为主要成本，可以替换实现，而不改变公开 document contract。

### 4.3 Error model

调用方提供的预期错误输入返回 typed error，不应 panic。

例如：

```text
unknown NodeId
node/content kind 不匹配
非法 TextOffset boundary
range 越界
非法 parent/child 关系
非法 root state
非法 mark operation
```

内部不变量可以使用 debug assertion，但公开 safe API 必须稳定返回可判断错误。

## 5. Position / Selection Surface

P0 只建立后续阶段需要的语义结构，不实现视觉 caret 行为。

初始类型：

```text
TextPoint
TextSelection
NodeSelection
NodeGap 或等价 structural boundary position
CursorAffinity
```

`TextPoint` 包含 stable node identity 和 `TextOffset`。

`CursorAffinity` 先进入类型模型，避免 soft wrap / BiDi 出现后再修改 canonical selection contract。P0 不实现视觉 affinity resolution。

Cell selection 延后到 table 阶段。

## 6. P0 实施切片

### P0.0 Phase contract 与模块骨架

交付：

```text
P0 design / progress 文档
Core 模块边界
public/private 可见性策略
初始 Error / Result 类型
```

Gate：workspace CI 保持全绿，架构边界无需返工。

### P0.1 Text Boundary

交付：

```text
TextBuffer
TextOffset
TextRange
validated slicing / replacement
UTF-8 boundary checks
Unicode regression fixtures
```

Gate：ASCII、中文、中英混排、emoji、combining mark、BiDi 测试通过；非法 byte boundary 返回 typed error，不能 panic。

### P0.2 Document Model

交付：

```text
DocumentVersion / DocumentRevision
NodeId
Node / NodeKind / NodeAttrs / NodeContent
NodeStore
immutable XiaomuDocument snapshot
full-tree validation
node-level structural-sharing prototype
TextRun / Mark / MarkSet normalization
```

Gate：合法树可以构建；dangling child、非法 parent/child、多个 parent、cycle、非法 root、unreachable node 等 malformed state 被拒绝；简单 revision 测试证明未变化 node payload 会共享。

### P0.3 Position 与 Selection

交付：

```text
TextPoint
CursorAffinity
TextSelection
NodeSelection
structural boundary position
selection 对 document snapshot 的校验
```

Gate：无效 node/range position 被拒绝；中文和 emoji position fixture 行为一致。

### P0.4 Transaction Application

第一批 typed steps：

```text
ReplaceText
InsertNode
RemoveNode
SetNodeAttrs
AddMark
RemoveMark
Transaction
TransactionOrigin / metadata seam
```

Gate：所有 mutation 保持 document invariant；不存在公开的 canonical direct-mutation escape hatch。

### P0.5 Position Mapping

交付：

```text
StepMap / ChangeMap prototype
text replacement mapping
node insertion / removal mapping
明确的 deleted-target result
transaction mapping composition
```

Gate：mapping table 覆盖 insertion、deletion、replacement、中文 / emoji offset 和 removed node。

### P0.6 Inverse 与随机不变量测试

交付：

```text
inverse / change-set prototype
transaction round-trip tests
normalized-mark inverse tests
可行范围内的随机 valid transaction sequence tests
```

Gate：可逆 operation 可以恢复语义原状态；随机测试不产生非法 document，不 panic。

### P0.7 Contract Stabilization

交付：

```text
public rustdoc 审查
architecture.md 与真实实现同步
P0 progress evidence 完整
P1 尚未解决的依赖明确记录
```

Gate：顶层 P0 Gate 全部满足，`CI Success` 全绿。

## 7. 测试策略

P0 测试优先断言语义，不绑定无关的内部实现形态。

必须覆盖：

```text
boundary / value type 单元测试
normalization tests
invalid-input tests
transaction result tests
mapping tables
inverse tests
property / randomized tests
regression fixtures
```

除非排序本身属于 contract，否则测试不能依赖偶然的内部排序。

## 8. P0 期间的设计变更规则

小型实现细节可以直接在对应 P0 分支调整。

如果变更影响 P0 contract、切片边界或 Gate，需要同步更新本设计文档。

如果 P0 确定了未来很难逆转、会成为长期公开语义契约的决策，例如 canonical position unit 或 mapping deletion policy，需要创建 ADR。

`docs/architecture.md` 只记录已经真实实现的架构，不记录尚未落地的计划。

## 9. P0 完成定义

只有以下条件全部满足，P0 才算完成：

```text
版本化结构化 document model 可用
snapshot 对外不可变
text boundary Unicode-safe
marks 确定性规范化
position / selection 可针对 document 校验
typed transaction 保持不变量
mapping 显式且可组合
inverse prototype 通过 round-trip
Unicode / CJK / emoji / property tests 全绿
架构文档与实现一致
CI Success 全绿
```

P1 不能用 GPUI-specific offset 或 mutation logic 去补偿 P0 尚未解决的 Core 不变量。
