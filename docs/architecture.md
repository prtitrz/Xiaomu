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

当前阶段事实：P0 Core contract、P1 native single-block input、P2 document tree / structural edit 与 P3 cross-block selection / history 已完成；P4 已启动，P4.1 mixed-inline coordinate contract 与 P4.2 canonical inline atom value / transaction 层（含 atom-aware `ReplaceInlineText` replacement contract）已建立。P3 已覆盖 visual-line geometry / soft-wrap、visual navigation / selection、cross-block editing / structured clipboard、history grouping / StoredMarks / IME、canonical LF HardBreak / CodeBlock multi-line、accessibility projection seam、realistic host integration、Unicode/CJK/emoji/combining/BiDi closeout matrix 与 Windows 最终实机 Gate。P4.1 新增 `InlinePoint(node_id, text_offset, atom_index, affinity)`，保持 `TextOffset` 的 UTF-8 byte contract，不使用 sentinel / fake byte，并在 canonical atom placement 尚未建立时对非零 ordinal fail closed。Core 已具备 `SplitNode / JoinNodes / SetNodeKind` 等结构 step、P4.2 的 `InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom / ReplaceInlineText` atom-aware step，并继续用 `ReplaceText` 承载 canonical LF；Runtime session 已升级为跨 block `DocumentSelection`，编排 split / join / list Enter / wrap / lift / indent / outdent，并承担 cross-block Delete、detached clipboard projection、structured paste、local history grouping 与 semantic line-break command；GPUI 使用 crates.io 精确 pin 的 `gpui = "=0.2.2"`，通过 `DocumentView` 与 `BlockTextLayout` 提供 multi-block 渲染、soft-wrap / multi-logical-line geometry、visual-line navigation、selection 投影、鼠标 hit-test、IME composition、scroll-to-caret、list marker 与 CodeBlock multi-line input；可复用 `EditorInstance` 持有独立 session/history/StoredMarks/listener/persistence，并支持完整 `DocumentSelection` restore 与 native focus routing。宿主 persistence 通过 `DocumentPersistence` seam 进出 canonical snapshot，harness fixture 只保存它明确支持的语义，未支持的 node kind 必须 fail closed。

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

ADR 0004 固化了当前 line-break contract：LF `\n`（U+000A）是晓木唯一赋予 line-break 语义的 inline scalar。Paragraph / Heading 等普通富文本 inline node 中 LF 表示 HardBreak；CodeBlock 中 LF 表示代码 newline；soft-wrap 不产生 canonical byte。LF 继续使用普通 UTF-8 `TextOffset`，因此无需 HardBreak 专用 Core content variant 或 position system。Core 原始 construction 当前仍容忍 CR 作为普通 scalar；平台 adapter / codec 表达 line break 时负责 `CRLF / CR → LF` 规范化。

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
InlinePoint
NodeGap
TextSelection
NodeSelection
```

`TextPoint` 由 stable `NodeId`、`TextOffset`、`CursorAffinity` 组成。使用时针对具体 snapshot 校验：节点存在、携带 inline content、offset 是拼接文本的合法 UTF-8 scalar boundary。

P4.1 引入 `InlinePoint` 作为 mixed-inline canonical coordinate seam：

```text
InlinePoint(node_id, text_offset, atom_index, affinity)
```

`text_offset` 继续严格表示 canonical text 的 UTF-8 byte offset；inline atom 不占 fake byte，也不使用 U+FFFC/private-use sentinel。若同一 text boundary 上未来存在 N 个 atom，则 `atom_index = 0..=N` 表达 N+1 个唯一 caret gap；`CursorAffinity` 仍只处理 visual ambiguity，不承担 canonical atom order。当前文档尚未建立 atom placement，所以 pure-text path 只允许 `atom_index = 0`，非零 ordinal 返回 typed failure。`TextPoint ↔ InlinePoint` 在 ordinal 0 时精确兼容。

`NodeGap` 表示 parent child list 的结构边界位置。`TextSelection` 保存 anchor / focus；Core 语义仍要求两端在同一个 inline node。跨 block selection 位于 Runtime `DocumentSelection`。

视觉 caret projection 与 affinity 的视觉解析属于 frontend。mixed-inline coordinate 的长期决策见 ADR 0005。

### Transaction Application

`transaction/` 是 canonical mutation 的唯一公开入口。当前 typed step 包括：

```text
ReplaceText
ReplaceInlineText
InsertInlineAtom
RemoveInlineAtom
RestoreInlineAtom
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

P4.2 落地了 atom-aware mutation：`InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom` 以 stable `NodeId` 与 `(text_offset, atom_index)` seam 操作 canonical atom；`ReplaceInlineText` 是 mixed-inline text replacement contract，消费 `InlinePoint` boundary 区分 seam 两侧 caret gap。旧 `ReplaceText / AddMark / RemoveMark` 保持 text-only 语义，在含 atom 的歧义 seam / range 上 fail closed；`ReplaceInlineText` 对"替换区域内含 atom"的 step 同样 fail closed，原子删除必须用 `RemoveInlineAtom` 显式表达；`SplitNode / JoinNodes` 遇 atom 仍 fail closed。

### Position Mapping

`mapping/` 实现显式 position mapping。映射只由 transaction application 产出，其他子系统不维护并行 offset 修补规则。

```text
StepMap
ChangeMap
MapBias（Start / End）
MappedPosition（Mapped / Deleted）
```

主要 step map 包括文本 replacement（`TextReplaced` 与 mixed-inline `InlineTextReplaced`）、atom insert/remove（`InlineAtomInserted / InlineAtomRemoved`）、node insert/remove、`NodeSplit`、`NodeJoined`。目标被删除时返回 `Deleted`，不静默 clamp。split 点、插入点等歧义由显式 `MapBias` 决定。

`StepMap::map_inline_point` 与 `ChangeMap::map_inline_point` 在同一 mapping engine 中调整 ordinal：atom insert/remove 平移同界 gap；`InlineTextReplaced` 消费 `seam_atom_index` 重排 seam ordinal——被编辑 gap 由 bias 解析，纯删除时 end 侧 ordinal 合并到保留 seam atom 之后，replacement 场景 end 侧 ordinal 在平移后的自身 boundary 保持。所有 step 共用同一 mapping engine，不存在平行修补逻辑。

`TextSelection` 映射采用向外 bias；collapsed selection 保持 collapsed。长期 mapping 决策见 `docs/adr/0002-position-mapping-policy.md`。

### Inverse 与 Undo Round-trip

`AppliedTransaction::inverse()` 返回 `System` origin 的逆 transaction。inverse 在 apply 时同步记录 before-state，关键对应关系包括：

```text
ReplaceText        → 恢复旧文本与旧 marks
ReplaceInlineText  → 恢复旧文本与旧 marks（atom-aware，seam ordinal 保留）
InsertInlineAtom   → RemoveInlineAtom
RemoveInlineAtom   → RestoreInlineAtom（精确恢复 identity/payload/placement）
AddMark            → RemoveMark + 恢复冲突旧值
RemoveMark         → 恢复旧 mark pieces
InsertNode         → RemoveNode
RemoveNode         → RestoreSubtree
SetNodeAttrs       → 恢复旧 attrs
SetNodeKind        → 恢复旧 kind
SplitNode          → JoinNodes
JoinNodes          → 删除追加文本 + RestoreSubtree
```

多 step inverse 按 step 反序组合。随机 valid transaction 测试持续验证 document validity、position mapping validity、单笔 round-trip 与整链 undo。LF 插入不增加专用 step：它是普通 `ReplaceText`，mapping seam 使用 Start / End bias 区分 LF 前后，并由同一 inverse contract 精确恢复。

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
StoredMarks
SessionOutcome
DocumentChangeListener
```

`DocumentSelection` 是 Runtime 的 document-level selection，两端可落在不同 inline block；公开读取点始终针对当前 snapshot 校验。排序使用 snapshot tree order，并保留 anchor / focus 方向。

P4.1 曾以 ordinal-0 兼容 seam 提供 `DocumentPosition::from_inline_point`、`DocumentPosition::as_inline_point` 与 `DocumentSelection::from_inline_points`。P4.3 完成 Runtime 存储迁移：`DocumentPosition` 的 text endpoint 升级为 `Inline(InlinePoint)`，caret 可落在同一 boundary 的任意 canonical gap；ordinal 合法性由 `DocumentSelection::validate` 对 snapshot 校验（节点存在、UTF-8 boundary、ordinal `0..=N`）；selection mapping 对 inline endpoint 消费 `ChangeMap::map_inline_point`，document-order 排序计入 `atom_index`；Runtime `move_caret` 以 one-caret-unit 步进（atom ordinal 优先、text scalar 其次）。planner 仍以 text-only 路径为主，seam 上的 mutation 在 P4.3 后续切片切到 `ReplaceInlineText`。

当前 `EditIntent` 覆盖：

```text
InsertText
CommitComposition
PasteText
PasteSlice
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

此外提供 `EditIntent::insert_line_break()` 语义构造器。调用方表达 HardBreak / CodeBlock newline 时不依赖其当前内部 variant；P3.5 当前将它编译为 isolated text replacement。

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
CaretAtLastInsertedOffset
PreserveFocus
```

结构命令和 structured paste 按 intent 明确 selection policy，避免把“目标节点被删”统一解释为失败。

### History 与 StoredMarks

Core inverse contract 仍只负责单笔 transaction 的精确反演；history grouping 由 Runtime `HistoryStack` 决定。每个 Runtime history entry 保存 redo / undo transaction、before / after `DocumentSelection` 与显式 `HistoryGroup`。

当前 grouping 规则：

```text
连续 collapsed InsertText
  + 同一 NodeId
  + 前一插入 end == 后一插入 start
  + before/after selection 连续
  + typing group 未被 boundary 关闭
→ 合并为一个 undo unit

caret / selection move
mark command
paste / cut
structural command
IME commit
undo / redo
raw apply
→ 关闭 typing group 或形成独立 history entry
```

Runtime 不使用时间阈值推断 canonical history 语义。合并后的 redo 按原提交顺序拼接，undo 按逆序拼接；entry 保留第一笔 `before_selection` 与最后一笔 `after_selection`，因此 grouped typing 的 undo / redo 可精确恢复 selection。

collapsed caret 的 `ToggleMark` 更新 session-local `StoredMarks`，不写入 `XiaomuDocument`、不推进 revision、也不创建空 `TextRun`。`None` 表示继续使用 Core 的 surrounding-run inheritance，`Some(empty)` 表示显式要求无 mark。普通 `InsertText` 与 `CommitComposition` 共用同一 StoredMarks 应用规则。

StoredMarks 生命周期已经明确：真实 caret / selection movement、undo / redo 与不继承格式的结构命令会清除；`SplitBlock` 保留 pending marks 到新 tail block，但同时关闭旧 typing group；collapsed mark toggle 本身也关闭 typing group，因此切换格式后的后续输入属于新的 undo unit。IME preedit/cancel 不改变 Runtime selection 或 StoredMarks，只有最终 commit 进入 Runtime history。

### HardBreak / CodeBlock line break

Runtime 不建立第二套 line editing engine：

```text
EditIntent::insert_line_break()
→ isolated text replacement
→ ReplaceText("\n")
→ existing mapping / inverse / StoredMarks
```

line break command 与前后普通 typing 明确断组，并在 replacement 后把 caret 放到 LF 后的合法 byte boundary。ordinary rich-text 的结构 Enter 仍使用 `SplitBlock`；是否把 Enter 翻译为 structural split 还是 line break 由 frontend 根据目标 node kind 决定。

Runtime 提供 `normalize_multiline_paste_text` 作为 frontend-neutral line-ending adapter helper：`CRLF / CR → LF`，已有 LF 保持不变。`normalize_paste_text` 则是普通 rich-text 的当前 plain fallback，在先规范化后把 LF 折叠为空格。两者是输入策略，不改变 Core 能表示 LF 的事实。

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

Runtime clipboard 已从 P2 的纯文本 seam 升级为 frontend-neutral structured clipboard：

```text
DocumentSelection
→ ClipboardSlice
   ├─ plain_text
   ├─ ClipboardBlock leaves
   └─ ClipboardNode minimal fragment roots
→ versioned metadata codec
```

`ClipboardSlice` 是 detached value，不携带 canonical `NodeId`。单一 inline leaf 只保留所选 inline fragment；跨多个 inline leaf 时，projection 从 canonical tree 剪出覆盖 selection 的最小 fragment tree，因此 list / quote 等 container 可以保留，同时不会携带未选择的 sibling。

`plain_text` 始终存在，并用 `\n` 表达所选 inline block boundary；若某个 leaf 自身包含 canonical HardBreak / code newline，其 LF 同样原样保留。Runtime metadata codec 当前格式为 `xiaomu.clipboard` v2；它使用私有 serde wire DTO，不给 Core canonical value 增加 serde 依赖。decode 会重新构造临时 document 校验 fragment tree；foreign、malformed、unknown-version 或与系统文本不一致的 stale metadata 均视为不可识别，由 frontend 回退到 plain text。

cross-block Delete / Cut 由 Runtime 统一编排。Delete 保留首个 inline block identity 与未选 prefix，把末 block 未选 suffix 接到 seam，删除覆盖的中间 leaves，并清理因本次操作而变空的 container；Cut 的 clipboard projection 是只读步骤，文档侧仍只提交一次 Delete history change。

structured paste 分两条路径：

```text
leaf-only slice
→ ordinary Core transaction
→ ReplaceText / mark steps / InsertNode

container slice
→ StagedPlan
→ split host at selection seam
→ reconstruct fragment roots / children
→ combine stage inverses
```

两条路径对 session 都是一条 history entry。leaf-only paste 精确恢复 source marks、block kind / attrs，并把宿主 suffix 接到最后 pasted leaf；container paste 通过 hidden staged transaction 解决“新 container 的 NodeId 只有 InsertNode apply 后才存在”的依赖，中间 snapshot 不暴露。after-selection 落在最后 pasted inline leaf 的 paste seam，undo / redo 恢复精确 store、selection 与已分配 identity。

`TextClipboard` 仍保留为最小纯文本 host seam；平台 structured transport 不进入 Runtime/Core 类型系统。

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
store 不存在                 → Ok(None)
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
    plain text + Xiaomu metadata 的 GPUI clipboard adapter

inline_position.rs
    DocumentView mixed-inline focus / selection projection seam

document_view/
    DocumentView multi-block 容器
    navigation.rs document-order / horizontal scalar navigation helper
    visual_navigation.rs wrapped visual-row navigation + desired_x translation
    cache_key.rs layout cache key

block_view/
    ParagraphView：单 inline block 的 input / layout / paint
    ParagraphElement：wrapped selection/caret paint + input handle
    layout.rs：BlockTextLayout、soft-wrap / multi logical-line visual rows、caret affinity、2D hit-test
    scroll.rs：shared ScrollHandle 上的最小 scroll-to-caret 调整

accessibility.rs
    frontend-neutral AccessibilityProjection

editor.rs
    reusable EditorInstance
    window / key binding / EditorHooks 装配
    bind_default_editor_keys
    run_document_editor(_with_hooks)
    run_single_block_editor 薄兼容入口
```

### Input / IME

所有文档 mutation 经 Runtime intent 提交。平台 `EntityInputHandler` 的 UTF-16 range 在 GPUI adapter 转换为合法 Core UTF-8 coordinate。

IME composition 的 preedit 保持 frontend-local，不推进 document revision，也不移动 Runtime canonical selection。composition state 只保存待替换的 canonical byte range、当前 preedit 与 preedit 内 UTF-16 selection；更新与 cancel 都不写 history。cancel 只丢弃 transient projection，因此 pending StoredMarks 不会因伪 caret movement 被清除。最终 commit 通过单个 `EditIntent::CommitComposition { range, text }` 进入 Runtime，使用与普通 typing 相同的 StoredMarks 规则，并形成恰好一个独立 undo unit。P3 composition 仍限制在单 block 内启动；该 byte-range / UTF-16 adapter 按完整 display text 工作，因此 canonical LF 不引入单独平台坐标系。

### Multi-block DocumentView

`DocumentView` 持有共享 session，并按文档序为 inline-bearing block 挂载 `ParagraphView`。焦点跟随 `DocumentSelection` focus node 路由。

P4.1 新增 `DocumentView::inline_focus_point` 与 `DocumentView::inline_selection_points`，将现有 Runtime selection/focus 投影为 `InlinePoint`。当前纯文本路径仍得到 ordinal 0；后续 atom placement 出现后，上层 GPUI API 不需要再次更名或另建平行 position 类型。

Left / Right 保持 Unicode scalar navigation，并在 soft-wrap 共享 logical offset 上先通过 `CursorAffinity` 跨越上一视觉行末尾 / 下一视觉行开头两个 caret state。Home / End 解析当前 visual row 首尾。Up / Down 读取最近一次 `BlockTextLayout` 的 wrapped geometry；`desired_x` 只保存在 `DocumentView` frontend transient state，连续纵向移动保持视觉列，越过 block 边界时在相邻 inline block 的首 / 末 visual row 上按同一 x 求最近合法 Core offset。Shift 版本只改变 selection focus，anchor 继续由 Runtime document selection 持有。

`BlockTextLayout` 同时承载 soft-wrapped visual rows 与 canonical LF 分隔的多个 logical lines。相邻 logical `WrappedLine` 的 coordinate 起点按 `previous.len() + 1` 推进，那个 `+1` 对应真实 LF byte；因此 `a\nb` 的 offset 1 / 2 分别是 LF 前 / 后两个独立 caret。soft-wrap 则没有 canonical byte，只有在前后 visual row 共享同一 offset 时才由 `CursorAffinity` 区分两个视觉位置。

鼠标点击 / 拖选使用 paint 期发布的 block bounds 注册表，先确定目标 block，再用 wrapped layout 二维 hit-test 得到合法 text position；命中 soft-wrap boundary 时同时保留对应 `CursorAffinity`。hard newline 的 hit-test / selection 继续返回 LF 两侧各自的真实 byte boundary。

选区绘制按 `DocumentSelection::ordered` 逐块投影：端点块画局部 range，中间块全选。collapsed selection 绘制 focus caret；非 collapsed selection 虽不绘制 caret，仍使用 focus endpoint 的 wrapped caret geometry 驱动 scroll-to-caret。

`DocumentView` 持有一个 GPUI `ScrollHandle` 并绑定在 document scroll viewport。每个 `ParagraphView` 共享该 handle；focused block 在 prepaint 中根据 canonical focus 或 IME virtual caret 计算 window-space caret bounds，只请求保持 focus 可见所需的最小纵向滚动。滚动写入延迟到 next frame，避免同一 prepaint / paint pass 内各 child 观察到不同 scroll offset。

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

### Accessibility projection

P3.6 已建立 frontend-neutral `AccessibilityProjection`。projection 可读取 editable text、semantic node role/kind、当前 `DocumentSelection` 与实际 focus owner；editor 未激活时，即使 Runtime 仍保留 caret，`focus_owner` 也为 `None`。

当前精确 pin 的 GPUI `0.2.2` 缺少后续版本公开的 `gpui::Role` / `.role()` builder，所以 P3 不伪造平台 AccessKit tree。平台 accessibility adapter 继续限制在 `xiaomu-gpui`，待 GPUI 能力升级后接入；Core / Runtime contract 不承载 GPUI/AccessKit 类型。

### Clipboard 与键绑定

GPUI 已绑定 Left / Right / visual Home / End / Up / Down、Shift visual selection、Backspace / Delete、SelectAll、Undo / Redo、Copy / Cut / Paste、Bold / Italic / Code / Underline / Strike，以及 Enter / Shift+Enter / Tab / Shift-Tab。macOS / Windows 使用平台对应组合键。

普通 rich-text `Enter` 继续结构 `SplitBlock`，`Shift+Enter` 插入 canonical LF HardBreak。CodeBlock 的 Enter / Shift+Enter 都插入 LF；Tab 插入四个可见空格，并绕开 list conversion / list indent，Shift-Tab 当前只保证不触发 list structural command。

Copy / Cut 将 Runtime `ClipboardSlice::plain_text` 写入系统文本，同时在 GPUI `ClipboardItem` metadata 槽写入 Xiaomu v2 structured metadata。外部应用按普通文本消费；晓木 Paste 优先验证 structured metadata，metadata 缺失、过期或非法时自动走 `PasteText` plain-text fallback。普通 rich-text plain paste 当前把 line break 折叠为空格；CodeBlock plain paste 保留多行并规范化为 LF。若剪贴板带有效 Xiaomu structured metadata但目标是 CodeBlock，frontend 主动使用 `ClipboardSlice::plain_text` 而不重建 rich structure，使代码块保持 plain-code destination semantics。structured paste 与 plain-text paste 都是显式 history boundary；平台 adapter 只负责 transport，不进入 Core 类型系统。

### Host hooks / reusable editor instance

`EditorHooks` 接受 `DocumentPersistence` adapter 与 `DocumentChangeListener`。Ctrl/Cmd-S 触发 `SaveDocument`，把当前 canonical snapshot 交给 persistence adapter。

P3.6 引入可复用 `EditorInstance`，每个 instance 独立持有 session/history/StoredMarks/listener/persistence。宿主可以恢复完整 `DocumentSelection`；`DocumentView::focus_selection` 会把 native focus 路由到恢复后 selection 的 focus node。`bind_default_editor_keys` 从 convenience runner 中抽出，真实宿主可以复用同一 key route 而不依赖 demo runner。

`multi_editor_host.rs` 用两个独立 GPUI editor/window 验证 input、selection、accessibility focus owner、listener、Ctrl+S persistence、session/history 均不串状态。`gpui` 的 test-support 只存在于 dev/test 依赖，不扩散到 production contract。

`examples/editor_harness` 使用 harness-private fixture v2 演示：

```text
create editor
→ load document
→ listen to committed changes
→ edit
→ save canonical snapshot
→ restart / load
```

fixture v2 当前保存 tree shape、inline runs、MarkSet（含 Link attrs）与 NodeAttrs，并对 inline LF 使用转义后 round-trip，因此 Paragraph HardBreak / CodeBlock newline 不会在保存时丢失。它不是公共 codec；`HorizontalRule`、`Image`、`Custom` 等尚未编码的 node kind 会返回 `PersistenceError`，不会静默跳过。

## Codec 边界

`xiaomu-codec-markdown` 是 import / export boundary。Markdown 不属于 canonical editing state，Markdown source offset 也不是 document position。

```text
external format
      ↕
codec crate
      ↕
xiaomu-core document model
```

Core 永远不反向依赖 codec。ADR 0004 只规定 canonical LF 语义；Markdown 后续如何把 Paragraph LF 编码为 hard break、如何保留 CodeBlock LF，属于 codec 自身 contract。

## Host 边界

宿主通过 public API、adapter、capability service 和 extension seam 集成晓木。

宿主专用 business model 不进入晓木 canonical document semantics，除非某个概念已经证明对通用编辑器具有普遍价值。文件、数据库、资产、网络、协作 transport、窗口 / workspace / app shell、产品配置都由 Host 持有。

当宿主便利性与晓木长期 correctness / extensibility 冲突时，由宿主在 adapter boundary 完成适配。

## P3 Closeout 事实

P3.7 固定 Unicode matrix 覆盖 ASCII、中文、中英混排、emoji、combining mark、CJK+emoji 与 BiDi。Runtime cross-block invariant 测试验证 scalar boundary、clipboard/delete seam、document/selection validity 与 exact undo/redo；deterministic randomized history/mapping sequence验证全链 undo/redo；GPUI wrapped-navigation fixture 通过真实 `TestAppContext / EditorInstance / DocumentView` 验证同一 Unicode matrix 的 Home/End/Up/Down projection。

code head `5584d57745fa4bd760f15b5ef7d911f23fb9d6ee` 的 CI #282 与 Gate-document head `8cadaa7dba055505379a7c4d9e3a0ca5a5b393fa` 的 CI #283 均在 Ubuntu/Windows/macOS、fmt、Clippy、workspace all-targets、source-size、dependency-boundary、cargo-deny/advisory 与 aggregate `CI Success` 上全绿。2026-09-01 Windows 最终实机 Gate 通过，IME、Unicode、wrapped navigation、cross-block clipboard/history、list structural editing、scroll/focus/keyboard-only 与 persistence 均未发现缺陷。Windows 与输入法具体版本未单独记录。

因此 P3 的 host-neutrality、Unicode/history correctness、realistic host integration 与 final real-machine Gate 已全部满足，**P3 = CLOSED**。

## P4.1 Mixed-inline Coordinate 事实

P4.1 固化 ADR 0005：`TextOffset` 继续严格表示 canonical UTF-8 text bytes，atom 不占 fake byte；`InlinePoint` 用 `(text_offset, atom_index)` 表达 mixed-inline order，并保留 `CursorAffinity` 只处理 visual ambiguity。Core mapping、Runtime compatibility seam 与 GPUI selection/focus projection 已接入同一个类型边界；纯文本现有路径保持 ordinal 0，因此 P0-P3 行为无语义变化。

P4.1 也明确了后续 transaction 约束：同一 `TextOffset` 上 atom 前后是不同 canonical caret gap，因此 mixed-inline text replacement 必须消费 atom ordinal；不能把位置先降格成裸 `TextOffset` 再在 Runtime/GPUI 猜顺序。

## P4.2 Canonical Inline Atom 事实

P4.2 在 Core 中建立了 canonical atom value 层与 transaction 层：

```text
NodeKind::InlineAtom(AtomKind)
NodeContent::InlineAtom(InlineAtomContent { fallback_text })
InlineAtomPlacement(atom NodeId, text_offset)
InlineContent = normalized text runs + ordered atom placements
```

atom 以 stable `NodeId` 为 identity，不建立第二套 AtomId allocator；同一 text boundary 允许多个 atom，vector order 即 canonical order；full-tree validation 把 inline atom reference 当真实 tree edge（target 存在、shape 正确、单一 parent、不可进入 structural children、不可为 root、unreachable 即 invalid、placement 必须是合法 UTF-8 boundary）。`fallback_text` 是 Core 级通用语义，服务 plain-text clipboard、accessibility 与 unknown renderer fallback。

transaction 层提供 `InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom` 与 mixed-inline `ReplaceInlineText`。`ReplaceInlineText { at: InlinePoint, end, replacement }` 消费 seam ordinal：seam 上 ordinal 之前的 atom 保持锚点，纯插入把其后 seam atom 移到插入文本之后，end 及之后的 atom 按 byte delta 平移；替换区域内含 atom 时 fail closed，原子删除必须显式经过 `RemoveInlineAtom`。mapping 由 `StepMap::InlineTextReplaced` 在同一 mapping engine 中重排 ordinal；inverse 同为 atom-aware 步骤并精确恢复 store。`ReplaceText / AddMark / RemoveMark` 保持 text-only contract 并在含 atom 的歧义 seam / range 上 fail closed；`SplitNode / JoinNodes` 遇 atom fail closed，placement migration 规则未证明前不 ad-hoc 修补。

Runtime 自 P4.3 起全链路消费 atom ordinal：session selection 存储 `Inline(InlinePoint)`；`move_caret` 以 one-caret-unit 步进；typing / Backspace / Delete 在含 atom 节点经 `ReplaceInlineText` / `RemoveInlineAtom` 表达（纯文本节点保持 P0-P3 `ReplaceText` 路径）；IME commit 对边界 atom 存活、内部 atom fail closed。structured clipboard 以 detached atom payload（kind / attrs / `fallback_text`）携带 inline atom，plain text 在锚点拼接 fallback，paste 重新分配 canonical identity；携带 atom 的 multi-block / hierarchical paste fail closed。GPUI 渲染层（renderer registry / layout / hit-test / accessibility fallback）属于 P4.4，当前对 seam gap 保持 fail closed 投影。

## P4.4 GPUI Atom Renderer 事实

GPUI 提供 host-neutral 的 inline-atom 渲染 seam：`InlineAtomRendererRegistry` 以 stable `AtomKind` 为 key 解析 `InlineAtomRenderer`，renderer 只消费 canonical 数据（`InlineAtomView`：identity、kind key、`fallback_text`、attrs）。未注册 renderer 的 kind 确定性回落到 `FallbackAtomRenderer`（显示与朗读均为 `fallback_text`），不允许 panic 或丢失 atom。registry 经 `EditorHooks.atom_renderers` → `EditorInstance::new` → `build_view` → `DocumentView::set_atom_renderers` 接入，与 persistence seam 同构。accessibility projection 现在遍历 inline atom placement：每个 atom 投影为携带 `fallback_text` 的非可编辑子节点（`AccessibilityRole::InlineAtom`）。paint 层尚未消费 registry（P4.4 后续切片交付 display splice、chip 样式与 hit-test）。

## 仓库级约束

架构通过以下机制持续执行：

- `tools/check_dependency_boundaries.py` 检查 crate dependency direction；
- `tools/check_source_size.py` 执行 source-file size guardrail；
- CI 执行 Rust formatting、Clippy 和 tests；
- `cargo-deny` 检查 dependency source / license policy；
- `engineering-rules.md` 约束实现与文档同步。
