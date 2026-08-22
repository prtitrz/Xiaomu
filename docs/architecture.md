# 晓木 Xiaomu 架构

本文档只记录仓库中**已经真实成立**的架构事实。未来规划放在 `planning.md`；重要且长期的设计理由放在 `adr/`。

## Workspace 边界

当前 workspace 由五个 library crate 和一个 example harness 组成：

```text
xiaomu-core
xiaomu-runtime
xiaomu-gpui
xiaomu-codec-markdown
xiaomu-testkit
examples/editor_harness
```

生产依赖方向已经作为仓库硬约束：

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

`xiaomu-codec-markdown` 只依赖 canonical Core model。`xiaomu-testkit` 用于测试和辅助能力，不允许成为 production dependency。

当前 `xiaomu-runtime`、`xiaomu-gpui`、codec、testkit 和 example harness 仍处于 bootstrap 阶段。`xiaomu-core` 已进入 P0，Text Boundary、P0.2 Document Model、P0.3 Position / Selection、P0.4 Transaction Application 和 P0.5 Position Mapping 已实现；Inverse / History 仍按 P0 后续切片推进。

## Core 边界

`xiaomu-core` 承载文档语义，不依赖 UI framework、宿主应用、持久化层、网络层或 codec。

当前 P0 模块边界：

```text
document
text
selection
transaction
mapping
history
commands
```

Core 同时公开语义级 `Error` / `Result`。P0 的具体契约与完成标准以 `docs/phases/p0-core-contract/design.md` 为准。

Core 从 P0 开始建立：

```text
versioned document model
text boundary
position / selection
typed transaction
position mapping
history primitives
commands / structural invariants
```

Core 保持 `#![forbid(unsafe_code)]`。

### Text Boundary

已经实现：

```text
TextBuffer
TextOffset
TextRange
```

`TextBuffer` 当前内部使用 `String`，调用方只通过语义 API 操作，不依赖底层 storage representation。

`TextOffset` 是 opaque UTF-8 byte coordinate。普通外部调用方不能从任意 raw integer 直接构造；通过 `TextBuffer::offset_at` 获取时会校验 bounds 和 UTF-8 character boundary。已有 offset / range 再次用于某个 buffer 时也会重新校验，因为文本修改后旧坐标可能已经 stale。

`TextRange` 使用半开区间 `[start, end)`。预期非法 offset / range 返回 typed Core error，不 panic。

Core Text Boundary 保证 Unicode scalar safety。Grapheme-cluster caret 行为属于更高编辑层；UTF-16 转换属于未来 platform adapter，不进入 Core coordinate contract。

文本 replacement 返回新的 `TextBuffer`，保持 immutable snapshot 方向。

长期坐标决策见 `docs/adr/0001-core-text-coordinate.md`。

### Document Value Layer

`document/` 已按职责拆分，并实现：

```text
DocumentVersion
DocumentRevision
NodeId
HeadingLevel
NodeKind
MarkKind
Mark
LinkMark
MarkSet
TextRun
AttrValue
NodeAttrs
InlineContent
NodeContent
Node
NodeStore
NodeStoreBuilder
XiaomuDocument
```

`DocumentVersion` 表示 canonical schema version。`DocumentRevision` 是本地 snapshot metadata，不是 collaboration clock 或 distributed operation identity。

`NodeId` 稳定且 opaque。其内部 storage representation 不属于公开 contract，普通外部 API 不能从 raw integer 任意构造 NodeId。当前确定性 allocator 由 `NodeStoreBuilder` 持有，失败构建不会消耗 ID，因此测试和初始构建保持可预测。

`HeadingLevel` 校验 built-in heading 范围 `1..=6`。`NodeKind` 提供 built-in structural semantics，并支持 extension-defined custom key。

`MarkSet` 使用确定性顺序，完全相同的重复 mark 自动规范化，同一 semantic kind 的冲突值被拒绝。`TextRun` 将非空 `TextBuffer` 与 normalized `MarkSet` 绑定。Run segmentation 不属于 document coordinate。

`InlineContent` 在构造时规范化相邻且 `MarkSet` 相同的 `TextRun`，因此持久化状态不会保留无意义的 run 分段。

`NodeAttrs` 使用确定性 key 顺序并 preservation-first 保存未知属性值，为未来 codec round-trip 和 extension 留出稳定边界。

### Canonical Node Tree 与 Snapshot

`Node` 字段私有，对外只提供只读 getter。节点类型与 `NodeContent` shape 在构造时校验。

`NodeStoreBuilder` 是当前公开的**初始文档构建入口**。它采用 bottom-up 构造：父节点引用的 child 必须已经存在，因此普通 safe construction 无法产生 dangling child reference。

`NodeStore` 对外只读，当前内部以：

```text
Arc<BTreeMap<NodeId, Arc<Node>>>
```

实现 P0 structural-sharing prototype。公开 API 不依赖这个具体 representation，未来可以由 benchmark 驱动替换为更适合的 persistent data structure。

`XiaomuDocument` 是 externally immutable canonical snapshot，包含：

```text
DocumentVersion
DocumentRevision
root NodeId
NodeStore
```

当前公开 API 只允许查询和重新校验，不提供直接 canonical mutation 入口。P0.4 会在 Transaction contract 确定后正式建立唯一公开 mutation path。

完整 snapshot validation 已覆盖：

```text
root 必须存在且为 Document
child NodeId 必须存在
同一 parent 不允许重复 child reference
parent / child kind 必须兼容
node kind / content shape 必须兼容
一个 reachable node 不允许多个 parent
node graph 不允许 cycle
store 不允许存在 root 不可达节点
```

Cycle 会明确返回 `CyclicDocument`，不会被次级的 multiple-parent 错误遮蔽。

P0.2 的 revision regression test 已证明：修改一个节点生成新 snapshot 时，未变化节点的 `Arc<Node>` payload 可以被复用。P0.2 只保留测试级 replacement helper 来证明 structural sharing；production mutation helper 不提前为 P0.4 预埋 dead code。

### Position 与 Selection

`selection/` 模块实现 typed position 与 selection，并对 `XiaomuDocument` snapshot 校验：

```text
CursorAffinity
TextPoint
NodeGap
TextSelection
NodeSelection
```

`TextPoint` 由 stable `NodeId`、`TextOffset` 和 `CursorAffinity` 组成。构造不触碰文档；使用时通过 `validate` 针对具体 snapshot 校验：节点必须存在、必须携带 inline content，offset 必须是该节点拼接文本的合法 UTF-8 scalar boundary（校验逻辑由 `InlineContent::validate_offset` 承担）。stale / deleted node 返回 `UnknownNode`，非 inline 目标返回 `InvalidSelection`。

`NodeGap` 表示 parent child list 上的结构边界位置（`index` 为边界前的 child 数量），只对 Children-shaped node 有效。

`TextSelection` 保存 anchor / focus 以保留用户意图；P0 要求两端落在同一个 inline node 内，跨 block selection 属于后续 session 层。`ordered_range` 返回逻辑排序后的半开 `TextRange`，affinity 不参与排序。`NodeSelection` 表示选中一个完整节点，只校验节点存在。

`NodeStoreBuilder::peek_next_id` 提供确定性的“下一个未分配 NodeId”，供测试构造保证不存在的节点，不开放 raw `NodeId` 构造。

视觉 caret 投影和 affinity 的视觉解析属于 frontend，不进入 Core。

### Transaction Application

`transaction/` 模块实现 canonical mutation 的唯一公开入口：

```text
TransactionOrigin（UserInput / System / Extension）
Transaction（origin + metadata + typed steps）
TransactionStep：
    ReplaceText
    InsertNode
    RemoveNode
    SetNodeAttrs
    AddMark
    RemoveMark
```

`Transaction::apply_with_changes(&XiaomuDocument) -> Result<AppliedTransaction>` 是原子的：steps 顺序应用在内部中间 store 上，最终状态经过 full-tree validation 才返回新 snapshot 和 mapping 数据；任一 step 失败则整体失败且原 snapshot 不变。`Transaction::apply` 是只要新 snapshot 的便捷入口。每次 apply 推进 `DocumentRevision`。

文本与 mark steps 由 piece-based inline 编辑实现：runs 在 range 边界切分、编辑后重建并重新规范化。replacement 继承 `range.start` 所在 run 的 marks；AddMark 在 range 内替换同 kind 冲突 mark；结果不保留空 run，相邻同 mark run 自动合并。

InsertNode 由 snapshot 内部 allocator 分配新 NodeId；RemoveNode 连同整个子树移除；root 不可移除。`NodeStore` 的 replace / insert / remove 原语均为 `pub(crate)`，公开 API 不存在 direct canonical mutation escape hatch。

metadata seam 使用 `BTreeMap<String, String>`，不携带宿主专用类型。

应用过程同时产出 P0.5 的 position mapping 数据；inverse 生成（P0.6）将建立在同一 step 词表上。

### Position Mapping

`mapping/` 模块实现显式 position mapping。映射数据只由 transaction application 产出，其他子系统不允许自行修补 offset：

```text
StepMap（TextReplaced / NodeInserted / NodeRemoved）
ChangeMap（按 application order 组合的 step maps）
MapBias（Start / End）
MappedPosition（Mapped / Deleted）
```

`ChangeMap` 把旧 snapshot 的 position 按 step 顺序折叠映射到新 snapshot 的坐标空间；任一 step 删除目标节点后结果保持 `Deleted`。映射语义：

```text
text replacement：range 之前的 offset 不变；range 终点及之后按长度差平移；
                  range 内部与起点由 MapBias 解析到 replacement 边界
child insertion：插入点之后的 boundary 平移 +1；恰好位于插入点的 boundary 由 MapBias 解析
child removal：仅其后的 boundary 平移 -1；指向被删 child 的 boundary 在前一个兄弟处存活
removed subtree：目标位于被删子树内的 position / selection 显式 Deleted，不静默 clamp
```

`ChangeMap` 没有公开构造入口。映射是纯坐标算术，不校验 snapshot；映射结果与任何 stale 坐标一样需要针对目标 snapshot 重新校验。属性与 mark steps 不移动 position，不产生 step map 条目。`NodeInserted` 的 step map 携带新分配的 `NodeId`，插入后的结构定位不需要重新猜测 identity。

`TextSelection` 的映射采用向外 bias：两端解析向 replacement 外侧，覆盖被替换内容的 selection 仍覆盖 replacement；collapsed selection 保持 collapsed。

## Runtime 边界

`xiaomu-runtime` 负责围绕 Core 类型协调 editing session 和 command execution。它可以依赖 `xiaomu-core`，不能依赖 GPUI 之外的上层宿主语义。

Runtime 不拥有 App Shell。Persistence、file lifecycle、networking、product configuration 和 window ownership 都属于 host responsibility。

Runtime 保持 `#![forbid(unsafe_code)]`。

## GPUI 边界

`xiaomu-gpui` 是第一个 Native Frontend 实现。GPUI-specific input、focus、layout、paint、hit testing、clipboard integration 和 virtualization 都属于这一层。

GPUI platform type 不能泄漏到 Core 或 Runtime public contract。

GPUI dependency 当前尚未正式引入。进入 P1 时按 `planning.md` 固定 explicit revision。

## Codec 边界

`xiaomu-codec-markdown` 是 import / export boundary。Markdown 不属于 canonical editing state，Markdown source offset 也不是 document position。

未来 codec 统一遵守：

```text
external format
      ↕
codec crate
      ↕
xiaomu-core document model
```

Core 永远不反向依赖 codec。

## Host 边界

宿主通过 public API、adapter、capability service 和 extension seam 集成晓木。

宿主专用 business model 不进入晓木 canonical document semantics，除非某个概念已经证明对通用编辑器用户具有普遍价值。

当宿主便利性与晓木长期 correctness / extensibility 冲突时，由宿主在 adapter boundary 完成适配。

## 仓库级约束

架构通过以下机制持续执行：

- `tools/check_dependency_boundaries.py` 检查 crate dependency direction；
- `tools/check_source_size.py` 执行 source-file size guardrail；
- CI 执行 Rust formatting、Clippy 和 tests；
- `cargo-deny` 检查 dependency source / license policy；
- `engineering-rules.md` 约束实现与文档同步。

只要实现让本文档中的任何描述失真，就必须在同一 PR 中同步更新本文档。
