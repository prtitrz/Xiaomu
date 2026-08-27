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

当前阶段事实：P0 Core contract 已完成，P1 native single-block input 已完成，P2 document tree / structural edit 的功能实现与 Windows 实机 Gate 已完成。Core 已具备 `SplitNode / JoinNodes / SetNodeKind` 等结构 step；Runtime session 已升级为跨 block `DocumentSelection` 并编排 split / join / list Enter / wrap / lift / indent / outdent；GPUI 使用 crates.io 精确 pin 的 `gpui = "=0.2.2"`，通过 `DocumentView` 提供 multi-block 渲染、跨块导航、selection 投影、鼠标 hit-test、IME composition 与 list marker projection；宿主 persistence 通过 `DocumentPersistence` seam 进出 canonical snapshot，harness fixture 只保存它明确支持的语义，未支持的 node kind 必须 fail closed。

## Core 边界

`xiaomu-core` 承载文档语义，不依赖 UI framework、宿主应用、持久化层、网络层或 codec。

当前 Core 模块边界：

```text
document
text
selection
transaction
mapping
history
commands
```

Core 同时公开语义级 `Error` / `Result`，并保持 `#![forbid(unsafe_code)]`。

### Text Boundary

已经实现：

```text
TextBuffer
TextOffset
TextRange
```

`TextBuffer` 当前内部使用 `String`，调用方只通过语义 API 操作，不依赖底层 storage representation。

`TextOffset` 是 opaque UTF-8 byte coordinate。普通外部调用方不能从任意 raw integer 直接构造；通过 `TextBuffer::offset_at` 获取时会校验 bounds 和 UTF-8 character boundary。已有 offset / range 再次用于某个 buffer 时也会重新校验，因为文本修改后旧坐标可能 stale。

`TextRange` 使用半开区间 `[start, end)`。预期非法 offset / range 返回 typed Core error，不 panic。

Core Text Boundary 保证 Unicode scalar safety。Grapheme-cluster caret 行为属于更高编辑层；UTF-16 转换属于 platform adapter，不进入 Core coordinate contract。长期坐标决策见 `docs/adr/0001-core-text-coordinate.md`。

### Document Value Layer

`document/` 已实现：

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

`NodeId` 稳定且 opaque。内部 representation 不属于公开 contract，普通外部 API 不能从 raw integer 任意构造 NodeId。当前确定性 allocator 由 `NodeStoreBuilder` 持有，失败构建不会消耗 ID。

`HeadingLevel` 校验 built-in heading 范围 `1..=6`。`NodeKind` 提供 built-in structural semantics，并支持 extension-defined custom key。

`MarkSet` 使用确定性顺序，完全相同的重复 mark 自动规范化，同一 semantic kind 的冲突值被拒绝。`TextRun` 将非空 `TextBuffer` 与 normalized `MarkSet` 绑定。Run segmentation 不属于 document coordinate。

`InlineContent` 在构造时规范化相邻且 `MarkSet` 相同的 `TextRun`。`NodeAttrs` 使用确定性 key 顺序并 preservation-first 保存未知属性值。

### Canonical Node Tree 与 Snapshot

`Node` 字段私有，对外只提供只读 getter。节点类型与 `NodeContent` shape 在构造时校验。

`NodeStoreBuilder` 是公开的初始文档构建入口，采用 bottom-up 构造；父节点引用的 child 必须已经存在，因此普通 safe construction 无法产生 dangling child reference。

`NodeStore` 对外只读，当前内部结构：

```text
Arc<BTreeMap<NodeId, Arc<Node>>>
```

它实现 node-level structural sharing prototype；公开 API 不依赖这个具体 representation。

`XiaomuDocument` 是 externally immutable canonical snapshot，包含：

```text
DocumentVersion
DocumentRevision
root NodeId
NodeStore
```

公开 API 只允许查询和重新校验，不提供直接 canonical mutation 入口。唯一公开 mutation path 是 `Transaction::apply` / `apply_with_changes`。

完整 snapshot validation 覆盖：

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

### Position 与 Selection

Core `selection/` 实现：

```text
CursorAffinity
TextPoint
NodeGap
TextSelection
NodeSelection
```

`TextPoint` 由 stable `NodeId`、`TextOffset`、`CursorAffinity` 组成。使用时针对具体 snapshot 校验：节点存在、携带 inline content、offset 是拼接文本的合法 UTF-8 scalar boundary。

`NodeGap` 表示 parent child list 的结构边界位置。`TextSelection` 保存 anchor / focus；Core 语义仍要求两端在同一个 inline node。跨 block selection 位于 Runtime `DocumentSelection`。

视觉 caret projection 与 affinity 的视觉解析属于 frontend。

### Transaction Application

`transaction/` 是 canonical mutation 的唯一公开入口。当前 typed step 包括：

```text
ReplaceText
InsertNode
RemoveNode
RestoreSubtree
SetNodeAttrs
SetNodeKind
AddMark
RemoveMark
SplitNode
JoinNodes
```

`Transaction::apply_with_changes(&XiaomuDocument) -> Result<AppliedTransaction>` 原子执行：steps 按顺序作用于内部中间 store，最终状态通过 full-tree validation 后才返回新 snapshot、mapping 与 inverse；任一步失败则原 snapshot 不变。每次成功 apply 推进 `DocumentRevision`。

文本与 mark step 采用 piece-based inline 编辑；range 边界切分后重建并重新规范化 runs。Insert / remove / restore / rekind / split / join 均由 Core 校验 structural invariants。

`SplitNode` 只作用于 inline-bearing node，tail 分配新 NodeId；`JoinNodes` 要求相邻 inline 兄弟，保留 first identity；`SetNodeKind` 保留 NodeId / attrs / content，只替换 kind，并重新检查 shape 与 parent-child compatibility。

metadata seam 使用 `BTreeMap<String, String>`，不携带宿主专用类型。

### Position Mapping

`mapping/` 实现显式 position mapping。映射只由 transaction application 产出，其他子系统不维护并行 offset 修补规则。

```text
StepMap
ChangeMap
MapBias（Start / End）
MappedPosition（Mapped / Deleted）
```

主要 step map 包括文本 replacement、node insert/remove、`NodeSplit`、`NodeJoined`。目标被删除时返回 `Deleted`，不静默 clamp。split 点、插入点等歧义由显式 `MapBias` 决定。

`TextSelection` 映射采用向外 bias；collapsed selection 保持 collapsed。长期 mapping 决策见 `docs/adr/0002-position-mapping-policy.md`。

### Inverse 与 Undo Round-trip

`AppliedTransaction::inverse()` 返回 `System` origin 的逆 transaction。inverse 在 apply 时同步记录 before-state，关键对应关系包括：

```text
ReplaceText   → 恢复旧文本与旧 marks
AddMark       → RemoveMark + 恢复冲突旧值
RemoveMark    → 恢复旧 mark pieces
InsertNode    → RemoveNode
RemoveNode    → RestoreSubtree
SetNodeAttrs  → 恢复旧 attrs
SetNodeKind   → 恢复旧 kind
SplitNode     → JoinNodes
JoinNodes     → 删除追加文本 + RestoreSubtree
```

多 step inverse 按 step 反序组合。随机 valid transaction 测试持续验证 document validity、position mapping validity、单笔 round-trip 与整链 undo。

## Runtime 边界

`xiaomu-runtime` 围绕 Core 类型协调 editing session、command execution、history、clipboard seam 与 persistence seam。它依赖 `xiaomu-core`，不依赖 GPUI 或产品宿主语义。

Runtime 不拥有 App Shell、window、filesystem policy、networking、product configuration 或 codec，并保持 `#![forbid(unsafe_code)]` 与 `#![warn(missing_docs)]`。

### DocumentSession

`runtime/session/` 当前包含：

```text
DocumentSession
DocumentSelection / DocumentPosition
EditIntent
EditPlan / StagedPlan
SelectionUpdate
HistoryStack
SessionOutcome
DocumentChangeListener
```

`DocumentSelection` 是 Runtime 的 document-level selection，两端可落在不同 inline block；公开读取点始终针对当前 snapshot 校验。排序使用 snapshot tree order，并保留 anchor / focus 方向。

当前 `EditIntent` 覆盖：

```text
InsertText
Backspace
Delete
MoveCaret
PlaceCaret
ToggleMark
SplitBlock
JoinWithPrevious
TurnInto
IndentListItem
OutdentListItem
SetSelection
```

编辑流：

```text
intent
  → plan / staged plan
  → Core transaction apply_with_changes
  → intent-specific selection resolution
  → 原子替换 snapshot / selection / history
  → DocumentChangeListener notification
```

任何 Core 拒绝、selection mapping `Deleted`、或 after-selection 校验失败都会让 session 状态保持不变。合法空操作返回 `NoChange`，不推进 revision / history / notification。

主要 `SelectionUpdate`：

```text
CaretAfterReplacement
CaretAtEditStart
MapExisting
CaretAtSplitTail
CaretAtJoinSeam
CaretAtJoinPoint
PreserveFocus
```

结构命令按 intent 明确 selection policy，避免把“目标节点被删”统一解释为失败。

### List 与结构命令

P2 list 编辑不增加 Core 专用 step，使用通用 Core 原语与 Runtime staged plan：

```text
Paragraph → list
    InsertNode(list/item) + RemoveNode + RestoreSubtree

BulletList ↔ OrderedList
    SetNodeKind(list)

list item → Paragraph
    lift out；必要时拆分前后 list

IndentListItem
    移入前一 sibling item；需要时创建 nested list

OutdentListItem
    移入外层 list，清空的 nested list 同笔删除

SplitBlock inside list item
    非空：tail 移入新 sibling ListItem
    空项：嵌套 outdent，顶层 lift out
```

staged plan 的多个 Core transaction 对用户表现为一笔 history。undo 由各阶段 inverse 逆序组合；redo 重放 `inverse(inverse(T))`，从而复用原 identity，而不是重新执行会分配新 NodeId 的原始结构 step。

### Clipboard

`runtime/clipboard.rs` 提供 frontend-neutral `TextClipboard` seam 与 `normalize_paste_text`。平台 clipboard binding 位于 GPUI。

P2 仍只承诺单 inline selection 的 copy / cut / paste；cross-block structured clipboard 属 P3。

### Persistence

`runtime/persistence.rs` 定义 frontend-neutral：

```rust
pub trait DocumentPersistence {
    fn save(&mut self, document: &XiaomuDocument) -> Result<(), PersistenceError>;
    fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError>;
}
```

契约：

```text
store 不存在          → Ok(None)
读取 / parse / adapter failure → Err(PersistenceError)
```

Runtime 不定义 bytes 格式、文件路径、数据库、同步协议或自动保存策略。`save` 语义要求 adapter 对传入 canonical snapshot fail closed；不能在“成功”结果下静默丢失未支持语义。

## GPUI 边界

`xiaomu-gpui` 是第一个 Native Frontend。GPUI-specific input、focus、layout、paint、hit testing、clipboard integration 和后续 virtualization 都属于这一层。GPUI platform type 不能泄漏到 Core 或 Runtime public contract。

GPUI dependency 以精确版本 `gpui = "=0.2.2"` 固定；升级走独立 PR。

当前主要结构：

```text
input/utf16.rs
    平台 UTF-16 code unit ↔ Core UTF-8 byte offset

input/composition.rs
    IME CompositionState；preedit 只存在于 adapter

input/platform_clipboard.rs
    Runtime TextClipboard 的 GPUI adapter

document_view/
    DocumentView multi-block 容器
    navigation.rs 纯逻辑跨块导航 / selection translation
    cache_key.rs layout cache key

block_view/
    ParagraphView：单 inline block 的 input / layout / paint
    ParagraphElement：shape_line、selection/caret paint、hit-test handle

editor.rs
    window / key binding / EditorHooks 装配
    run_document_editor(_with_hooks)
    run_single_block_editor 薄兼容入口
```

### Input / IME

所有文档 mutation 经 Runtime intent 提交。平台 `EntityInputHandler` 的 UTF-16 range 在 GPUI adapter 转换为合法 Core UTF-8 coordinate。

IME composition 的 preedit 保持 frontend-local，不推进 document revision；commit 形成一笔 `InsertText` history entry；cancel 恢复 base selection。P2 composition 仍限制在单 block 内启动。

### Multi-block DocumentView

`DocumentView` 持有共享 session，并按文档序为 inline-bearing block 挂载 `ParagraphView`。焦点跟随 `DocumentSelection` focus node 路由。

跨块 Left / Right / Home / End / Up / Down 与 Shift selection 由 container 层翻译为 `SetSelection`。P2 Up / Down 使用单视觉行模型；目标 byte offset 先 clamp，再 floor 到合法 Unicode scalar boundary。x-preserving visual-line navigation 留给 P3。

鼠标点击 / 拖选使用 paint 期发布的 block bounds 注册表，先确定目标 block，再用 shaped line x hit-test 得到合法 text position。

选区绘制按 `DocumentSelection::ordered` 逐块投影：端点块画局部 range，中间块全选；caret 只绘制在 focus block。

layout cache key = `(node, editing epoch, rounded width)`；composition 期因虚拟文本不经过 document epoch 而绕过缓存。

### Block projection

当前 frontend projection 已区分：

```text
Heading      → 按层级放大 / 加粗
Quote        → 后代缩进 + 左侧竖线
BulletList   → bullet marker
OrderedList  → deterministic ordinal marker
nested list  → marker 与 list depth 对齐
```

list marker 只存在于 frontend projection，不进入 canonical text、TextOffset 或 selection range。

### Clipboard 与键绑定

GPUI 已绑定 Left / Right / Home / End、Shift selection、Backspace / Delete、SelectAll、Undo / Redo、Copy / Cut / Paste、Bold / Italic / Code / Underline / Strike，以及结构编辑 Enter / Tab / Shift-Tab。macOS / Windows 使用平台对应组合键。

### Host hooks

`EditorHooks` 接受 `DocumentPersistence` adapter 与 `DocumentChangeListener`。Ctrl/Cmd-S 触发 `SaveDocument`，把当前 canonical snapshot 交给 persistence adapter。

`examples/editor_harness` 使用 harness-private fixture v2 演示：

```text
create editor
→ load document
→ listen to committed changes
→ edit
→ save canonical snapshot
→ restart / load
```

fixture v2 当前保存 tree shape、inline runs、MarkSet（含 Link attrs）与 NodeAttrs。它不是公共 codec；`HorizontalRule`、`Image`、`Custom` 等尚未编码的 node kind 会返回 `PersistenceError`，不会静默跳过。

## Codec 边界

`xiaomu-codec-markdown` 是 import / export boundary。Markdown 不属于 canonical editing state，Markdown source offset 也不是 document position。

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

宿主专用 business model 不进入晓木 canonical document semantics，除非某个概念已经证明对通用编辑器具有普遍价值。文件、数据库、资产、网络、协作 transport、窗口 / workspace / app shell、产品配置都由 Host 持有。

当宿主便利性与晓木长期 correctness / extensibility 冲突时，由宿主在 adapter boundary 完成适配。

## 仓库级约束

架构通过以下机制持续执行：

- `tools/check_dependency_boundaries.py` 检查 crate dependency direction；
- `tools/check_source_size.py` 执行 source-file size guardrail；
- CI 执行 Rust formatting、Clippy 和 tests；
- `cargo-deny` 检查 dependency source / license policy；
- `engineering-rules.md` 约束实现与文档同步。

只要实现让本文档中的任何描述失真，就必须在同一 PR 中同步更新本文档。
