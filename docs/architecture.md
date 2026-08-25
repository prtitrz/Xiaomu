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

当前 codec、testkit 和 example harness 的编辑功能处于 bootstrap 阶段。P0（Core contract）、P1（单 block 原生输入）已完成，P2（document tree / 结构编辑）进行中：Core 已有 SplitNode / JoinNodes / SetNodeKind 结构 step；`xiaomu-runtime` 的 session 已升级为跨 block `DocumentSelection`，并编排 SplitBlock / JoinWithPrevious / TurnInto 结构命令；`xiaomu-gpui` 以精确 pin 的 crates.io GPUI `0.2.2` 实现单 Paragraph 编辑闭环（渲染 / 输入 / hit-test / IME composition / copy-paste / 基础 marks / harness）。

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

当前公开 API 只允许查询和重新校验，不提供直接 canonical mutation 入口。查询包括 `node` / `store` / `parent_of`（root 与未知身份返回 `None`）。唯一公开 mutation path 是 `Transaction::apply`。

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

`TextPoint` 由 stable `NodeId`、`TextOffset` 和 `CursorAffinity` 组成。构造不触碰文档；使用时通过 `validate` 针对具体 snapshot 校验：节点必须存在、必须携带 inline content，offset 必须是该节点拼接文本的合法 UTF-8 scalar boundary（校验逻辑由 `InlineContent::validate_offset` 承担；`InlineContent::offset_at` 提供同样校验的 offset 构造，P1.2 为 runtime 层引入的最小扩展）。stale / deleted node 返回 `UnknownNode`，非 inline 目标返回 `InvalidSelection`。

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
    RestoreSubtree
    SetNodeAttrs
    SetNodeKind
    AddMark
    RemoveMark
    SplitNode
    JoinNodes
```

`Transaction::apply_with_changes(&XiaomuDocument) -> Result<AppliedTransaction>` 是原子的：steps 顺序应用在内部中间 store 上，最终状态经过 full-tree validation 才返回新 snapshot 和 mapping 数据；任一 step 失败则整体失败且原 snapshot 不变。`Transaction::apply` 是只要新 snapshot 的便捷入口。每次 apply 推进 `DocumentRevision`。

文本与 mark steps 由 piece-based inline 编辑实现：runs 在 range 边界切分、编辑后重建并重新规范化。replacement 继承 `range.start` 所在 run 的 marks；AddMark 在 range 内替换同 kind 冲突 mark；结果不保留空 run，相邻同 mark run 自动合并。

InsertNode 由 snapshot 内部 allocator 分配新 NodeId；RemoveNode 连同整个子树移除；root 不可移除。`SetNodeKind` 保留 NodeId、attrs 与 content，只替换 kind；新 kind 必须接受现有 content shape，parent 必须仍允许该 child，root 不能改 kind。`RestoreSubtree` 是 `RemoveNode` 的精确逆 step：以原 NodeId 与 payload 整体回插先前移除的子树，要求所有 id 当前不存在，映射数据记为携带子树根的 `NodeInserted`。它不是通用的 copy / move 原语——调用方无法铸造 NodeId，payload 只能来自同一文档 lineage 的历史 snapshot，且 id 冲突时原子失败。`NodeStore` 的 replace / insert / remove 原语均为 `pub(crate)`，公开 API 不存在 direct canonical mutation escape hatch。

metadata seam 使用 `BTreeMap<String, String>`，不携带宿主专用类型。

应用过程同时产出 P0.5 的 position mapping 数据与 P0.6 的 inverse transaction。

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
                  range 内部与起点由 MapBias 解析到 replacement 边界；
                  空 range（纯插入）的插入点同样由 MapBias 解析
child insertion：插入点之后的 boundary 平移 +1；恰好位于插入点的 boundary 由 MapBias 解析
child removal：仅其后的 boundary 平移 -1；指向被删 child 的 boundary 在前一个兄弟处存活
removed subtree：目标位于被删子树内的 position / selection 显式 Deleted，不静默 clamp
```

长期 mapping 决策（显式 bias、显式 Deleted 结果）见 `docs/adr/0002-position-mapping-policy.md`。

`ChangeMap` 没有公开构造入口。映射是纯坐标算术，不校验 snapshot；映射结果与任何 stale 坐标一样需要针对目标 snapshot 重新校验。属性、kind 与 mark steps 不移动 position，不产生 step map 条目。`NodeInserted` 的 step map 携带新分配的 `NodeId`，插入后的结构定位不需要重新猜测 identity。

结构编辑（P2.1）引入两个额外 step map 变体：`NodeSplit`（inline 节点在文本 offset 处拆分，tail 文本进入新分配的兄弟节点；split 点之前的 offset 留在原节点，之后的 offset 平移进 tail，恰好落在 split 点的 offset 由 MapBias 解析；父级 child list 行为同 `NodeInserted`）与 `NodeJoined`（相邻 inline 兄弟合并，被吸收节点连同子树离开文档；其内 offset 平移到存活节点的接缝之后，child list 与删除语义一致）。

`TextSelection` 的映射采用向外 bias：两端解析向 replacement 外侧，覆盖被替换内容的 selection 仍覆盖 replacement；collapsed selection 保持 collapsed。

### Inverse 与 Undo Round-trip

`AppliedTransaction::inverse()` 返回一个 `System` origin 的逆 `Transaction`。inverse steps 由 apply 引擎在应用每个 step 时同步记录，因为只有此时能看到该 step 的 before-state：

```text
ReplaceText   逆 = 恢复旧文本 + 剥离 replacement 继承的 marks + 按旧 piece 重新加回 marks
AddMark       逆 = 整段 RemoveMark + 按旧 piece 恢复原值
RemoveMark    逆 = 按旧 piece 恢复被移除的 mark
InsertNode    逆 = RemoveNode（新分配的节点）
RemoveNode    逆 = RestoreSubtree（原 NodeId 与 payload 整体回插）
SetNodeAttrs  逆 = 换回旧 attrs
SetNodeKind   逆 = 换回旧 kind
SplitNode     逆 = JoinNodes（tail 兄弟并回头部，run 归一化精确还原）
JoinNodes     逆 = 删除追加文本的 ReplaceText + RestoreSubtree（原 NodeId 整体回插）
```

`ReplaceText` 的逆用与 `replace_text` 完全相同的继承规则（首个 end 触及 `range.start` 的 run；run 边界解析到前一个 run）计算需要剥离的 marks，因此跨 run 替换、run 边界编辑与纯删除都能精确还原规范化后的 text / marks。

逆 step group 按 step 反序组合；group 内坐标与其原 step 产生的中间状态一致，多 step transaction 的逆无需重放中间文档。对 `inverse().apply(&applied.document())` 的结果，store 与 root 与原 snapshot 完全相等（不止语义等价，被删子树的 NodeId 也原样恢复）；只有 revision 前进。`NodeStore` 的相等语义按 payload 内容比较，与 structural sharing 无关。

随机不变量测试（确定性 xorshift，无外部依赖）在随机 valid transaction 序列上同时验证：document 始终合法、旧 position 映射后仍落在合法坐标、每笔 transaction round-trip 还原 store、以及整链反序 undo 回到初始 store。

## Runtime 边界

`xiaomu-runtime` 负责围绕 Core 类型协调 editing session 和 command execution。它可以依赖 `xiaomu-core`，不能依赖 GPUI 之外的上层宿主语义。

Runtime 不拥有 App Shell。Persistence、file lifecycle、networking、product configuration 和 window ownership 都属于 host responsibility。

Runtime 保持 `#![forbid(unsafe_code)]` 与 `#![warn(missing_docs)]`。

### DocumentSession

`runtime/session/` 实现编辑会话编排层（P1.2；P2.2 起 selection 升级为 document-level）：

```text
DocumentSession（snapshot + selection + history + notification seam）
DocumentSelection / DocumentPosition（跨 block selection：端点为 TextPoint 或 NodeGap）
EditIntent（InsertText / Backspace / Delete / MoveCaret / PlaceCaret / ToggleMark /
            SplitBlock / JoinWithPrevious / TurnInto）
EditPlan（Core Transaction + runtime SelectionUpdate）
SelectionUpdate（CaretAfterReplacement / CaretAtEditStart / MapExisting /
                 CaretAtSplitTail / CaretAtJoinSeam）
HistoryStack（一笔 transaction 一个 entry，保存 redo/undo transaction 与 before/after selection）
SessionOutcome（DocumentChanged / SelectionChanged / NoChange）
DocumentChangeListener（frontend-neutral 通知 seam）
```

编辑流为 `intent → EditPlan → Transaction::apply_with_changes → resolve after-selection → 原子替换 snapshot / selection / history 并通知`。任何失败（Core 拒绝、selection 映射为 `Deleted`、新 selection 校验失败）都让 session 状态保持不变。raw apply 与 mark 编辑在端点被删时仍返回 `SelectionDeleted`；结构命令使用显式 after-selection policy，不再依赖"Deleted 即失败"：

```text
SplitBlock        → CaretAtSplitTail（caret 落在新 tail 兄弟起点）
JoinWithPrevious  → CaretAtJoinSeam（caret 落在存活节点的接缝 offset）
TurnInto          → MapExisting（kind 变更不移动 position，identity 保留）
fallback 失败     → 收敛后的位置无法合法化才原子失败
```

`SetNodeKind` 是 TurnInto 的 Core 原语：保留 NodeId / attrs / content，只替换 kind；content shape 必须与新 kind 兼容，parent 必须仍允许该 child，root 不能改 kind。Paragraph ↔ Heading ↔ CodeBlock 属于同 shape 转换；把 inline 节点变成 Quote 等 container 会因 shape 不兼容被拒绝，wrapping 留给后续 list 切片。

P2.2 的 selection 形态：session 持有 `DocumentSelection`，两端点可落在不同 block（`TextPoint`）或 child 边界（`NodeGap`），任何公开读取点都针对当前 snapshot 校验。跨 block 排序由 snapshot 上的 pre-order slot 分配解析（节点与 gap 各占单调槽位，文本位置以 byte offset 为子键）。单 inline node 内的 selection 可经 `text_selection()` 投影回 Core `TextSelection`（P1 单块前端继续使用）。内容编辑与 P2.3 结构命令仍要求选区整体位于一个 inline node 内；gap 端点上的 caret 移动返回 `NoChange`，跨块导航属后续切片。

intent-specific after-selection：InsertText 提交后 caret 落在 replacement 之后，Backspace / Delete 落在删除起点，ToggleMark、TurnInto 与 raw apply 经 ChangeMap 向外映射（`DocumentSelection::map_through` 对非折叠选区 head/tail 分别取 Start / End bias，任一端点被删则整体失败）。Backspace 在块首且存在前一兄弟时解释为 JoinWithPrevious；SplitBlock 对非折叠选区先删除再拆分，同属一笔 history。合法空操作返回 `NoChange`：不调用 Core apply、不推进 revision、不发通知、不写 history；raw `apply` 无 no-op 检测，空 transaction 也会提交。

undo 重放 `AppliedTransaction::inverse()`（ADR 0003）并直接恢复记录的 before_selection；redo 重放 `inverse(inverse(T))` 而不是原始 steps，因此像 SplitNode 这样会分配 NodeId 的命令在 redo 时通过 RestoreSubtree 复用原 identity，after_selection 仍然合法。undo 后的新编辑清空 redo 栈。caret 移动按 Unicode scalar boundary，Home / End 到 paragraph 逻辑首尾。session 纯逻辑、无 GPUI 依赖，全部行为在无显示器环境测试。

`runtime/clipboard.rs` 提供 frontend-neutral 纯文本 clipboard seam：`TextClipboard` trait（write_text / read_text，非文本剪贴板内容读为 `None`）与 `normalize_paste_text`（粘贴文本中的行断符折叠为空格——paragraph inline 文本不能包含换行；多 block 粘贴语义随后续 document-level 编辑引入）。平台绑定在 GPUI adapter 实现，编辑层只见该 trait。

## GPUI 边界

`xiaomu-gpui` 是第一个 Native Frontend 实现。GPUI-specific input、focus、layout、paint、hit testing、clipboard integration 和 virtualization 都属于这一层。

GPUI platform type 不能泄漏到 Core 或 Runtime public contract。

GPUI dependency 已按 P1.1 从 crates.io 以精确版本引入：workspace 依赖表 pin `gpui = "=0.2.2"`，升级只走单独 PR（`docs/planning.md` §17）。构建期行为：gpui 的 build script 在 macOS 上用 `xcrun metal` 预编译 Metal shader，本机构建需要 Xcode Metal Toolchain 组件（`xcodebuild -downloadComponent MetalToolchain`）；cargo-deny 的 license allow 列表为此新增 NCSA（经 `libfuzzer-sys` 由 `image → ravif → rav1e` 链带入，permissive 且 OSI 批准）。

### GPUI Adapter（P1.3–P1.5 单块编辑）

`xiaomu-gpui` 当前结构：

```text
input/utf16.rs              UTF-16 code unit ↔ Core UTF-8 byte offset 转换
                            （surrogate 中点解析到所在字符边界，始终是合法 Core 坐标）
input/composition.rs        IME CompositionState 纯状态机（preedit 只存在于 adapter）
input/platform_clipboard.rs runtime TextClipboard seam 的 GPUI 平台绑定
block_view/                 ParagraphView：持有 DocumentSession，键/鼠标 → runtime EditIntent，
                            实现 EntityInputHandler（平台 UTF-16 range 在此转换）
                            ParagraphElement：shape_line 单行渲染、caret / selection 绘制、
                            paint 期注册 handle_input、保存 layout 供 hit-test
editor.rs                   run_single_block_editor：窗口 / 键绑定 / 关窗退出装配（harness 使用）
```

- 所有编辑经 session intent 提交，view 不直接改文档；`replace_text_in_range` 的显式范围用两次 selection-only 的 `PlaceCaret` + 一次 `InsertText` 完成，保持单条 history entry。
- 键位：Left / Right（有选区时先折叠到选区端点）、Shift 选择、Home / End（含 Shift）、Backspace / Delete、SelectAll、Undo / Redo、Copy / Cut / Paste 与 Bold / Italic / Code / Underline / Strike 切换（macOS / Windows 双绑定）；点击与拖拽经 hit-test（`closest_index_for_x` → boundary 校验）定位 caret。
- clipboard：copy / cut 取 session 选区纯文本经 `TextClipboard` 写出；paste 读入后经 `normalize_paste_text` 归一化再走 InsertText intent（一笔 undo entry）；cut = copy + Delete，同为单笔。空剪贴板文本不清除选区。
- 标记渲染：Bold / Italic 映射字重与字形，Underline / Strike 映射装饰线，Code 映射半透明背景色块；Link 需要属性编辑 UI，留待后续切片。
- IME composition（P1.4）：CompositionState 维护 base selection / preedit / virtual projection，composition 全程 document revision 不变，commit 组装为单笔 InsertText intent 入 history，cancel 恢复 base selection。

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
