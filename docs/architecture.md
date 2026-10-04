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

当前阶段事实（2026-10-01 收口）：P0–P5 已完成；P4 inline atom / atomic block / image / baseline Markdown 的原生 Windows Gate 缺口已随 P5.6 补齐，P5 correctness、集成矩阵与 IME 阻塞已闭合。三平台 CI 与原生验收独立记录于 P5 progress；P6 未启动。Core 继续持有 canonical tree、UTF-8 `TextOffset`、mixed-inline `InlinePoint`、transaction/inverse/ChangeMap；Runtime 持有 session、结构命令、selection、clipboard 与 history；GPUI（精确 pin `gpui = "=0.2.2"`）持有 native input/focus、布局、paint、hit-test 与平台剪贴板。`InlineAtomRendererRegistry` / `InlineAtomHostCapability` 已交付，不等同于尚未实现、已归属 P7 的通用 `BlockRendererRegistry` / `LinkOpenService`。`EditorInstance` 保持 per-editor session/history/StoredMarks/listener/persistence 隔离。宿主 persistence 经 `DocumentPersistence` 进出 canonical snapshot；fixture 未支持的 node kind 继续 fail closed。各阶段已实现契约与验收范围见文末和对应 progress。

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

`NodeKind::TaskList` / `TaskItem` 是独立 builtin 容器，不转换成普通列表。TaskList 只接 TaskItem；TaskItem 接普通 block（可嵌套 task/ordinary list、Code、Image、Quote、Table）。Core 允许空容器与任意合法首 block；宿主持久化的 paragraph-first/nonempty 规则归 codec / final SessionPolicy 校验。`checked` 仅存在 NodeAttrs，missing / null / false / true 原样保存，错误类型由 `InvalidTaskItemChecked` 拒绝；读取不补默认值，未知 Core attrs 仍保留。Task clipboard 条件写 v12，显式保存 open/closed，遍历包括 table-cell payload；旧非 Task wire 不变，旧版本拒绝新 kind。默认 PasteSlice 在 policy 之后、任何 fitting/state 改动之前返回 `UnsupportedEdit`，防止 task wrapper 被单段 paste 静默丢失；generic Markdown 明确拒绝 Task。此为 canonical/clipboard 基础，未包含 task 命令、checkbox UI 或原生验收。见 [ADR 0011](adr/0011-typed-task-lists.md)。

`MarkSet` 使用确定性顺序，完全相同的重复 mark 自动规范化，同一 semantic kind 的冲突值被拒绝。`TextRun` 将非空 `TextBuffer` 与 normalized `MarkSet` 绑定。Run segmentation 不属于 document coordinate。

`LinkMark` 以 typed `LinkAttributes` 保存 href / target / rel / class / title，每字段使用 `StringAttribute::Missing / Null / Value(String)`，空字符串不等同于缺失或 null。旧 `new(href,title)` 保留经典含义，`from_attributes` / `attributes` 提供精确保真；`href()` 返回 `Option<&str>`，不以空字符串伪装缺失。`classic_parts()` 只在旧 href/title 两字段无损时返回投影。所有字段参与 mark equality、same-kind conflict 和普通 transaction/inverse；Core 不推断宿主默认值、不执行 URI。详见 [ADR 0007](adr/0007-exact-link-attributes.md)。

`TextStyleMark` 以 typed `TextStyleAttributes` 保存 color / font_family / font_size，复用三态 `StringAttribute`。全 Missing、全 Null、空串或未识别字符串均精确保留；mark 的存在也不因“视觉为空”自动消失。所有字段参与 equality、same-kind conflict、run normalization 和 transaction/inverse；Core 不解析 CSS、不填宿主默认、不施加 Code 排他规则。布局是否支持某个值是独立 frontend/host capability，不能通过替换 canonical 字符串伪装支持。Markdown 普通段落、Code/Heading 及旧 harness fixture 均拒绝 TextStyle，避免静默丢失。详见 [ADR 0008](adr/0008-exact-text-style.md)。

`InlineContent` 在构造时规范化相邻且 `MarkSet` 相同的 `TextRun`。`NodeAttrs` 使用确定性 key 顺序并 preservation-first 保存未知属性值。`AttrValue::Null` 是显式空值，支持 list/object 内递归保留；`get(key) == Some(&AttrValue::Null)` 与缺失 key 的 `None` 不同，不表示删除属性。它不放宽 image 等 typed attrs 校验，也不引入浮点值；canonical document version 仍为 v1。见 [ADR 0006](adr/0006-nullable-node-attrs.md)。

ADR0004 的默认 LF contract 保留：literal LF 使用普通 UTF-8 TextOffset，soft-wrap 不产生 canonical byte。为区分外部结构中的 literal LF 与独立 marked hardBreak，[ADR0009](adr/0009-typed-hard-break.md) 新增 typed builtin atom，沿用现有 NodeId/placement/ordinal，不建立第三套坐标。`AtomKind::new("hardBreak")` 仍是普通 extension，不会被字符串猜测升级。builtin 要求空 attrs、LF fallback；InlineAtomContent 的真实 MarkSet 参与 equality/inverse。Core/Runtime 已接 mixed split/join、范围格式与 exact-gap 输入继承，GPUI 全选包含末尾 inline atoms；宿主命令和真实平台验收仍单独推进。

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

`text_offset` 继续严格表示 canonical text 的 UTF-8 byte offset；inline atom 不占 fake byte，也不使用 U+FFFC/private-use sentinel。同一 text boundary 上的 N 个 atom 由 `atom_index = 0..=N` 表达 N+1 个唯一 caret gap；`CursorAffinity` 仍只处理 visual ambiguity，不承担 canonical atom order。P4.2 已建立 atom placement 与非零 ordinal 的验证，P4.3 起 Runtime editing path 消费完整 mixed-inline coordinate。纯文本路径使用 ordinal 0；`TextPoint ↔ InlinePoint` 在 ordinal 0 时精确兼容。

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

P4.2 的 `InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom` 以 stable NodeId 和 `(text_offset, atom_index)` seam 操作 atom。旧 ReplaceText/AddMark/RemoveMark 保持 text-only 语义，歧义范围 fail closed；ReplaceInlineText 内部含 atom 的范围仍需显式 RemoveInlineAtom。新增 SplitInlineNode 使用完整 InlinePoint 切分，旧 SplitNode 仍拒 atoms；JoinNodes 已保留 mixed atoms，RestoreJoinedNode 的精确 suffix 校验与 split mapping 恢复原右节点 identity/attrs/kind。Core 映射已测，Runtime structural command resolver 的接入仍在进行。

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

链接投影现在以蓝色下划线显示 Link mark，包含无 href 的 inert Link，原始文字、三态属性与其它 marks 不变。颜色/装饰不承担 URL 导航。`DocumentView::apply_edit_transaction` 供宿主把逐 run 合并后的完整 transaction 一次发布：沿既有 composition guard、raw session apply 的最终 policy validator、epoch/child/focus/scroll 路径执行，一次 Undo；明确不执行 typed-intent preflight。`has_active_composition` 只读观察独立输入框的 virtual 状态，不等待或捕获焦点；Linux 普通指针事件仍先 unmark 旧 handler，再分发 MouseDown。

### Per-instance host edit policy（2026-10-03 实验）

`DocumentSession::new_with_policy` 在构造时绑定可选 `SessionPolicy`；旧 `new` 保留无 policy 的通用语义。`EditorInstance::new_with_policy` 把同一能力带到 GPUI，原 `EditorHooks` 字段和 `new` 签名不变。没有运行时更换 policy 的入口，避免旧 Undo 历史受后换规则影响。Core `MarkSet` 不包含任何宿主 schema 或 codec 的规则。

可选 `EditorInstance::with_command_router` 在现有 composition 保护之后、默认 Tab/ShiftTab 规划和普通非 Code 文本 paste 的换行归一化之前，向纯只读 `EditorCommandRouter` 暴露文档、完整选区、stored marks 与原始文本。`Default` 保留原行为；`NoChange` 或错误消费动作而不修改会话；`Intent` 仍经过同一 policy、事务验证和 Undo 发布路径。结构剪贴板、图片、Code paste 及 IME 传输不进入该路由。该 API 不修改 `EditorHooks` 的公开字段，也不授权回调外部副作用或重入借用。已有实例可用 `DocumentView::set_command_router(None)` 恢复默认。

可选 `with_list_marker_provider` / `set_list_marker_provider` 每次渲染读取真实 list/item/index/depth 与当前 attrs，仅向原 GPUI 文本绘制提供视觉标签。默认标签与固定宽度不变；自定义标签列可扩宽，不向正文插入序号、不改变位置或 selection。setter 后已挂载视图须按通常 GPUI 约定通知重绘。

代码块宿主可独立覆盖默认方法 `route_enter`、`route_code_paste`、`route_code_slice` 和 `route_arrow_down`。Enter 来源在 block-kind 映射前区分普通、Shift 与显式绑定的 Ctrl/Cmd；未绑定新增 primary action 的旧宿主不变，Default 向外传播。Code 文本在 CR/LF 规范化前交给 hook，混合图片+非空文字可由宿主选择文字；默认仍保持图片优先及代码块图片拒绝。已验证结构片段通过独立 slice hook 提供完整树，Default 继续保留旧 open 平文行为与 closed fail-closed，不偷偷展开结构。普通平台文字保持独立 raw 来源。Down hook 只在非 Shift、无 composition 时运行，成功只改经过验证的 selection，不增加文档 history。所有 callback 仍为纯只读，失败/NoChange 不污染会话。

`CodeBlockPresentation` 是可选、每实例的前端配置，不改变 `EditorHooks` 或 canonical attrs。代码块使用宿主字体与颜色、.88 倍正文字号、1.55 倍正文行高（对应 CSS pre 最小行框）、14/16px 内边距、8px 圆角与 1px 边框；列表 marker 留在包装外。layout/paint/caret/IME 共享同一有效样式和布局缓存；setter 清除该实例相关几何缓存。未配置宿主保持原样。虚拟 GPUI 测试不证明真实字体像素与浏览器一致，语法高亮计算尚未接入该配置。

宿主结构规划可返回 `SelectionUpdate::PreserveSelection`，把原 anchor、focus、affinity 与 cell range 完整保留，并在最终文档上验证；不做自动位置映射或退化成 caret。失效选区在普通/staged 发布前失败，保留原 marks、typing group、history 与 listener 状态。既有 `PreserveFocus` / `MapExisting` 语义不变。相关门禁是 Runtime 8 项、GPUI 外部 12 项和内部 4 项新增自动测试；这不等于某宿主列表产品行为或原生 GUI 已验收。

宿主把文本 LF 转成零字节 inline atom（或反向转换）时，可使用 `SelectionUpdate::Exact { selection }` 提供最终快照坐标，保留正反范围、双端 affinity 和 atom ordinal。普通与 staged commit 都只在最终快照上验证该选区，非法 UTF-8 边界、已删节点、越界 ordinal 或非 collapsed 选区搭配显式 stored marks 时整体拒绝；不得借中间快照的合法性提前发布。Undo 保留原选区，Redo 恢复精确后选区。新增 11 项 integration 和 3 项 staged unit 回归覆盖后续 typing grouping、redo 与 listener 原子性。此公开 enum 新增 variant，外部穷尽匹配须更新；原 selection policies 不变。

范围剪贴板按真实树叶序选择 Inline 与 Atomic 节点，保留覆盖范围中的图片等原子块及其 attrs、marks 和最小容器；只有真正 collapsed Atomic 选区走单块复制捷径，Atomic 作为前后范围端点不再截断后续内容。未知被选叶节点和 Gap 端点明确拒绝，未选内容不被顺带复制。Cut 在写系统剪贴板及删除之前，先验证结构 metadata 能 encode/decode 且与原 slice 全等，失败不写、不删；普通 Copy 的既有纯文本 fallback 不变。新门禁含 7 项 Runtime 范围/回滚测试与 2 项 lossless-write helper 测试，不能宣称已测试真实 OS Cut。通用 mixed inline/atomic paste 仍明确拒绝，宿主可以通过 policy 接受完整 detached roots 并提供自己的事务规划。通用跨块删除与 Ctrl+A 的首尾原子块范围仍是独立待完善项。

`EditIntent::insert_line_break()` 返回独立 `InsertLineBreak`，让 policy 明确区分键盘换行与同字节 `PasteText("\n")`。只有 policy 返回 Continue 后的私有默认 dispatch 才将它转换为既有 isolated LF 插入；不会二次调用 policy，但最终 candidate 验证仍照常执行。无 policy 的 canonical/marks/selection/Undo 行为保留。**外部自定义 policy 若以前只按 PasteText 匹配换行，必须显式识别新 variant**，不能假定公共 constructor 永远展开为 PasteText。这里仍是命令来源区分，并未把 canonical LF 改成精确保真 TipTap hardBreak 节点。

宿主工具栏可调用 `DocumentView::apply_edit_intent` 复用内建编辑 action 的同一入口：composition guard、session policy、render epoch、child 同步、焦点和 caret scroll 均沿原路径执行，不直接绕过前端对共享 session 操作。该 seam 只公开已有行为，不新增输入协议。

`prepare_intent(SessionContext, &EditIntent)` 在任何 selection / StoredMarks / history mutation 前运行，包括 `PasteSlice` 和 cell-range convergence。只读 context 提供 document、selection、explicit stored marks，以及复用 Runtime 周围 run 继承语义的 `effective_typing_marks`。宿主返回 Continue、完全保留状态的 NoChange、collapsed inline caret 的显式 StoredMarks（区分 None / Some(empty)），或一个 `EditPlan`。宿主可用 `EditPlan::new` / `PrimaryEdit::new` 描述替代 transaction 与 selection policy，一次成功接管只产生一个 isolated Undo 单元，无需可变 session 或递归 `apply_intent`。

前端可用 `DocumentSession::effective_input_marks(node, replacement_start)` 只读查询默认 replacement 的 marks：当前 inline focus 必须属于该节点，offset 须合法；stored marks 优先，否则在替换起点复用同一周围 run 继承规则。它不执行 intent/policy、不改变 session，可供 IME preedit 样式投影；host policy 自行替换计划的特殊行为不由该查询模拟。Cell range 与非 inline focus 明确拒绝。

`EditPlan::with_stored_marks` 可指定成功事务后的 typing marks，供 split 到空块等无法从 canonical run 继承的场景使用。最终 selection 必须是 collapsed inline caret；合法性与 candidate 在同一发布前阶段检查，通过后 marks 与 document / selection 一起安装。失败不能先发布文档再报错。

通用 `EditIntent::SetMark { mark }` / `RemoveMark { kind }` 与 ToggleMark 分离：前者按 semantic kind 替换完整 mark（包括 Link 全部 attrs），后者删除该 kind。Missing 是持久值，不是 patch 信号；若宿主只改某个 link 字段，须先读原属性并构造完整新值。Collapsed inline caret 操作 StoredMarks / 真实周围 run 继承，删除最后一个 mark 留 Some(empty) 防重继承；range 仍要求单节点 text selection，提交一次 AddMark / RemoveMark 事务及 isolated Undo。有效值已相同或目标 kind 已缺失时完全 no-op，保留 revision、listener、pending marks 和 typing group；Code 不获得额外默认排斥规则，宿主通过 policy 定义自己的行为。

平台显式 replacement range 通过 `apply_intent_with_selection` 把目标 selection 与 intent 合为一个原子动作，不先调用 selection setter。Preflight 只读查看目标；不同目标按既有输入规则重新继承 marks，成功仅通知最终结果，Undo 恢复整个回调前的 selection；拒绝或 policy NoChange 不泄露临时选区、marks / grouping 清理或通知。Composition 状态机不因该入口改变。

`validate_document` 检查构造时文档及所有将发布的最终 candidate：普通 commit、隐藏 staged commit、raw apply、Undo、Redo。它不检查 staged 中间 snapshot；最终 candidate 必须先通过 Core、selection 和 host 检查，才记录 history、发布 document 并通知 listener。拒绝以 `SessionError::Policy(PolicyError)` 返回，不允许 listener 事后修补。

失败只恢复轻量 transient checkpoint（Copy selection、StoredMarks、typing-group flag），不克隆整个历史；Undo/Redo 失败归还取出的 entry。文档、revision、selection、stored marks、history depths / grouping 和 listener 均保留。Cell-range navigation 的 collapse 在后续操作成功前不通知，避免 candidate 拒绝后已泄露 selection 通知。Policy callback 必须纯只读、稳定、不可重入、无外部副作用；引擎只保证自己状态的原子性。公开 seam 回归位于 Runtime `tests/session_policy.rs` / `policy_context.rs` 和 GPUI `tests/editor_policy.rs`；不据此宣称真实平台或宿主 schema 的原生验收。

`xiaomu-runtime` 围绕 Core 类型协调 editing session、command execution、history、clipboard seam 与 persistence seam。它依赖 `xiaomu-core`，不依赖 GPUI 或产品宿主语义。

本地验证：2026-10-03，workspace575 tests、vendored decoder8 tests、strict Clippy、fmt、source-size/dependency/provenance guards 和 cargo-deny bans/licenses/sources 通过。独立源码审查发现的平台 range 提前通知问题已修并以 Runtime/真实 GPUI 虚拟平台回归覆盖。日志在 `/workspace/shared/xiaomu-instance-policy-checks`；该验证不是 OS 原生 GUI 或远程发布证明。

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

2026-10-04 的 opt-in 导出增量见 [表格导出契约](table-clipboard-export.md)：
`SessionPolicy::clipboard_export_spec(context, Copy/Cut)` 在投影与平台写入之前运行。
默认仍保持历史 unit/TSV/wire 行为；显式配置可复制几何闭合的含跨度 CellRange，
并独立采用可重算的 LF/LF text-between 文本。CellRange 来源固定 open 1/1、
区分 Rows 与 Table 根；全篇/完整节点来源为 closed 0/0，二者不会互相推断。
新 v14 使用固定传输前缀与严格 JSON；`RejectedNative` 禁止 Text/Image fallback。
投影前借用预算预扫；未知自定义文本语义拒绝；新 CellRange Cut 在写剪贴板前拒绝。
该增量不包含矩形 Paste/clipping 或真实 OS 剪贴板验收。

```text
DocumentSelection
→ ClipboardSlice
   ├─ plain_text
   ├─ ClipboardBlock leaves
   └─ ClipboardNode minimal fragment roots
→ versioned metadata codec
```

`ClipboardSlice` 是 detached value，不携带 canonical `NodeId`。单一 inline leaf 只保留所选 inline fragment；跨多个 inline leaf 时，projection 从 canonical tree 剪出覆盖 selection 的最小 fragment tree，因此 list / quote 等 container 可以保留，同时不会携带未选择的 sibling。

`plain_text` 始终存在，普通片段用 `\n` 表达 inline block boundary；表格矩形使用 TSV，cell 内 block 边界与 tab/CR/LF 扁平化为空格。Runtime metadata 使用私有 serde wire DTO，不给 Core 增加 serde 依赖；不含 Null 时，非表片段写 `xiaomu.clipboard` v4，普通表写 v5，含非空 row attrs 的表写 v6。任一 node、atom、row attrs（包括嵌套 list/object）含 Null 时写 v7，使用 `{"type":"null"}`。decode 保持 v4–v6 兼容，拒绝旧信封中的 Null 和未知 attr variant，重建临时 document 校验 fragment tree；foreign、malformed、unknown-version、旧信封带新特性或与系统文本不一致的 stale metadata 均由 frontend 回退到 plain text。

完整 link attrs 在无法由经典 href/title 保真时按需写 clipboard v8 的 `link_attributes` variant；五字段显式 tagged missing/null/string，保留缺失、null、空串和 Unicode。普通 links 继续使用旧 variant 与 v4–v7 feature 选择；v8 可混合经典 links、node Null、table/row attrs。递归检查覆盖 container 与 table 内所有 runs，pre-v8 信封携带新 variant 拒绝；未知字段、缺少必要状态字段、重复键和错误类型同样 fail closed。历史 v1–v3 仍不支持，v4–v7 旧 Link 的 title null/missing 含义不变。

任一 run 携带 TextStyle 时才写 v9 的 `text_style` variant；三个 attrs 字段显式使用同一 tagged missing/null/string wire，含全 Missing 的 mark 也需要 v9。v9 可混合旧 Link、新 Link attrs、table/row/null；无 TextStyle 的文档继续沿 v4–v8 最低必要版本编码，旧语义不变。Pre-v9 携带 TextStyle、future/duplicate/未知字段/错误类型均拒绝。Runtime 普通输入、paste 与跨段 atom suffix 精确重建共享完整 mark-kind 表，确保目的 run 的 TextStyle 不泄漏到原本无样式的插入内容。

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

layout cache key = `(node, editing epoch, rounded width)`；composition 期因虚拟文本不经过 document epoch 而绕过缓存。缓存复用必须有明确的 `Some(key)`，不能把两个 `None` 当作命中：intrinsic min/max-content 测量没有确定宽度，刚取消的 preedit 也没有缓存身份，误复用会在 composition 已清空后继续绘制下划线拼音。`block_view/element_tests.rs` 在取消后立即走这条真实测量路径，断言 shaped text 回到原正文且 snapshot/selection/history 不变；普通固定宽度窗口不足以覆盖这个回归。

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

Copy / Cut 将 Runtime `ClipboardSlice::plain_text` 写入系统文本，同时在 GPUI `ClipboardItem` metadata 槽写入 versioned Xiaomu structured metadata：含 Null attrs 的片段使用 v7；否则普通片段使用 v4，含 table 使用 v5，含 table row attrs 使用 v6。外部应用按普通文本消费；晓木 Paste 优先验证 structured metadata，metadata 缺失、过期或非法时自动走 `PasteText` plain-text fallback。普通 rich-text plain paste 当前把 line break 折叠为空格；CodeBlock plain paste 保留多行并规范化为 LF。若剪贴板带有效 Xiaomu structured metadata 但目标是 CodeBlock，frontend 主动使用 `ClipboardSlice::plain_text` 而不重建 rich structure，使代码块保持 plain-code destination semantics。structured paste 与 plain-text paste 都是显式 history boundary；平台 adapter 只负责 transport，不进入 Core 类型系统。

### Host hooks / reusable editor instance

`EditorHooks` 接受 `DocumentPersistence` adapter 与 `DocumentChangeListener`。Ctrl/Cmd-S 触发 `SaveDocument`，把当前 canonical snapshot 交给 persistence adapter。

P3.6 引入可复用 `EditorInstance`，每个 instance 独立持有 session/history/StoredMarks/listener/persistence。宿主可以恢复完整 `DocumentSelection`；`DocumentView::focus_selection` 会把 native focus 路由到恢复后 selection 的 focus node。`bind_default_editor_keys` 从 convenience runner 中抽出，真实宿主可以复用同一 key route 而不依赖 demo runner。

`multi_editor_host.rs` 用两个独立 GPUI editor/window 验证 input、selection、accessibility focus owner、listener、Ctrl+S persistence、session/history 均不串状态。`gpui` 的 test-support 只存在于 dev/test 依赖，不扩散到 production contract。

`examples/editor_harness` 使用 harness-private fixture v5 演示：

```text
create editor
→ load document
→ listen to committed changes
→ edit
→ save canonical snapshot
→ restart / load
```

fixture v5 保存 tree shape、inline runs、MarkSet（含 Link attrs）、scalar NodeAttrs、inline atom placement、Image/HorizontalRule 与嵌套 table，并对 inline LF / `{` 使用转义后 round-trip。atom 编码为独立 `atom\t<kind>\t<fallback>` 行（可选 `@` attrs 行），父 leaf 的行内字段用 `{a#N}` token 标记放置位置。reader 兼容 v2/v3/v4，但拒绝低版本信封中的 table 标签与未定义 atom 引用；`Custom` 等未支持的 node kind 或 list/object attr 返回 `PersistenceError`，不会静默跳过。它不是公共 codec，Image 的语义/引用持久化也不等于保存资产字节；资产存储仍由宿主负责。

## Codec 边界

`xiaomu-codec-markdown` 是 import / export boundary。Markdown 不属于 canonical editing state，Markdown source offset 也不是 document position。

```text
external format
      ↕
codec crate
      ↕
xiaomu-core document model
```

Core 永远不反向依赖 codec。ADR 0004 只规定 canonical LF 语义；P4.9 baseline codec 已实现 Paragraph hard break 与 CodeBlock LF 的导入/导出，支持范围及 fail-closed 规则见下方 P4.9 说明。

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

Runtime 自 P4.3 起全链路消费 atom ordinal：session selection 存储 `Inline(InlinePoint)`；`move_caret` 以 one-caret-unit 步进；typing / Backspace / Delete 在含 atom 节点经 `ReplaceInlineText` / `RemoveInlineAtom` 表达（纯文本节点保持 P0-P3 `ReplaceText` 路径）；IME commit 对边界 atom 存活、内部 atom fail closed。structured clipboard 以 detached atom payload（kind / attrs / `fallback_text`）携带 inline atom，plain text 在锚点拼接 fallback，paste 重新分配 canonical identity；hierarchical paste 经 staged planner 重建容器语义。GPUI 渲染层（renderer registry / display projection / layout / hit-test / accessibility fallback）属于 P4.4。

## P4.4 GPUI Atom Renderer 事实

GPUI 提供 host-neutral 的 inline-atom 渲染 seam：`InlineAtomRendererRegistry` 以 stable `AtomKind` 为 key 解析 `InlineAtomRenderer`，renderer 只消费 canonical 数据（`InlineAtomView`：identity、kind key、`fallback_text`、attrs）。未注册 renderer 的 kind 确定性回落到 `FallbackAtomRenderer`（显示与朗读均为 `fallback_text`），不允许 panic 或丢失 atom。registry 经 `EditorHooks.atom_renderers` → `EditorInstance::new` → `build_view` → `DocumentView::set_atom_renderers` 接入，与 persistence seam 同构。accessibility projection 现在遍历 inline atom placement：每个 atom 投影为携带 `fallback_text` 的非可编辑子节点（`AccessibilityRole::InlineAtom`）。

P4.4b 起 paint 层消费 registry：`InlineAtomDisplayProjection` 为每个含 atom 的 inline 节点建立 canonical↔display 双向坐标投影（canonical text 只数 UTF-8 文本字节，renderer display text 只占 display 字节），并携带 `InlineAtomDisplaySpan`（atom NodeId、anchor、ordinal、display byte range）。layout、selection、caret 与 hit-test 一律先经 projection 换算，display byte 永不当作 canonical offset 使用。空 renderer text fail soft 到非空 `fallback_text`，atom 不可能成为零宽 canonical 单元。pointer hit-test 把 chip 内部点击按左右半区落到该 atom 的前/后 seam gap（`inline_point_for_display_hit`），span 边界与文本字节经 `inline_point_for_display_boundary` 精确反投影；atom chip 逐可视行绘制 tinted quad，selection 高亮覆盖其上。Runtime 暴露 `DocumentSession::set_inline_selection(InlinePoint, InlinePoint)` 供前端直接安装精确 seam 选区（`set_selection` 保持 text-only 兼容适配器）；GPUI 的 place / move_focus_to 全链路保留 `InlinePoint`，shift 扩选 anchor 不再丢弃 ordinal。

2026-10-01 原生验收发现旧 IME 路径把 ordinal>0 的 caret 降为 `TextSelection` 时失败，导致 chip 旁只有候选框、没有拼音预编辑；组合输入 layout 还会丢弃 chip。修正后 native selection 从完整 `InlinePoint` 获取，composition 捕获 start ordinal，layout 在 atom-aware display 上插入下划线 preedit、保留并移动 chip 装饰；平台 UTF-16 始终只数 editable text，`ime_geometry` 在 editable virtual text 与 visual layout 之间双向转换，供候选框 bounds / hit query / caret 共用。collapsed IME commit 在当前 caret 保留准确 ordinal，不再一律插到所有同缝 atom 后；取消不改文档，commit 为一个独立 undo entry。非空组合范围覆盖 atom 仍 fail closed，并消费后续 native commit/cancel，不能意外降成普通 typing 删除 atom。`block_view/ime_atom_tests.rs` 覆盖前/中/后 gap、更新/取消/提交、UTF-16 emoji、换行、候选框几何、逆序文本选区和精确 undo/redo。

键盘视觉导航按块坐标空间换算（#68）：纯文本块保持 canonical byte 路径；含 atom 块经 projection 在 display 空间步进，chip 严格内部作为一个 caret unit 跳过（chip 起始 / 结束 byte 是缝停点，标量后紧邻 chip 时先停在 chip 左缝再跨 chip——与 runtime canonical walk 的 one-caret-unit 语义逐位一致）；跨块行走保持 canonical，目标块尾的 end-anchored seam ordinal 经 `seam_ordinal_after` 保留；`desired_x` 连续性锚点升级为 `(InlinePoint, Pixels)`。

## P4.5 Integration Gate 事实

P4A 收口 gate（`crates/xiaomu-runtime/tests/p4a_integration_gate.rs` + `crates/xiaomu-gpui/tests/p4a_integration_gate.rs`）在真实多块、多 kind、CJK/BiDi fixture 上把 runtime seam 与 GPUI 视觉导航逐位对齐：相邻同缝 atom 的双向 caret walk、三个 seam gap 的输入 re-anchor、精确单 caret unit 删除、跨 CJK+atom 选区替换、undo/redo 精确恢复、`CommitComposition` 边界矩阵（起点=缝的 commit 缝上 atom 存活，严格覆盖 anchor 的 commit fail closed）、多 editor 编辑隔离。gate 审计修复两个不一致：runtime `atoms_inside_span` 的同 byte 分支现在要求 atom anchor 位于选区 boundary 上（原先会误删其他 boundary 上 ordinal 落窗的 atom）；GPUI display 空间步进把 chip 起始 byte 视为缝停点而非内部（标量后紧邻 chip 不再一次按键连跨两个停点）。

## P4.4c Host Capability 事实

宿主激活 seam（`crates/xiaomu-gpui/src/atom_capability.rs`）：`InlineAtomHostCapability::atom_action(AtomAction)`，`AtomAction` 只携带 `NodeId + AtomKind + action key + attrs 快照`，不含任何宿主业务类型，Core / Runtime 保持 atom-neutral。plain pointer click 落在 chip 内部时，`DocumentView` 先放置 caret 再经 `atom_action` 发出 `ATOM_ACTION_CLICK`；composition 期间的点击不产生激活。capability 与 renderer registry 同为 per-editor 值：`EditorHooks.atom_capability` → `EditorInstance` → `build_view` 注入，`DocumentView::set_atom_capability` 可在 build 后替换；多 editor 测试验证点击 A 的 chip 只激活 A 的 recorder、且 caret 落在 A 的块内。宿主侧自行解释 attrs（harness demo 从 `handle` attr 渲染 `@xiaomu`）。

## P4.6-P4.8 Atomic Block / Image 事实

GPUI 原子块点击在成功安装 node selection 后必须同时把键盘焦点交给该 `DocumentView`，即便重点击返回 `NoChange` 也重新获焦。Canonical selection 与 native focus 是两个状态；不能依赖此前文本段落碰巧仍拥有焦点。`atomic_focus_gpui` 覆盖冷 Image/HorizontalRule 点击、失焦后的相同选中重点击、Delete/Backspace 与 Undo。该回归不修改平台输入、IME owner 或 composition 等待语义。

Atomic block 进入了统一的 document position 模型：`DocumentPosition::Atomic(NodeId)` 只接受 atomic content 节点，`Slots` 以节点自身 slot 排序（gap-before < atomic < gap-after），mapping 经 `ChangeMap::map_node_selection`，公开 seam 为 `DocumentSession::set_atomic_selection`。Backspace/Delete 在 collapsed atomic selection 上删除整块并把 caret 收敛到该块占据的 gap（`SelectionUpdate::CaretAtGap`），undo 恢复块并重新安装 node selection。GPUI 侧 `navigation::nav_units` 把文档顺序推广为 text + atomic 序列，横向步进以 one-caret-unit 语义跨越 `text ↔ atomic ↔ text`；atomic selection 上 Up/Down/LineStart/LineEnd 为有意 no-op（atomic 块没有可视行）。跨块纯文本选区的 flat leaf 投影不携带中间 atomic 节点：这是 one-caret-unit 模型的有意语义——文本范围删除不吞并 atomic 块，atomic 删除必须经显式 node selection。

Image 走 typed canonical 语义（`crates/xiaomu-core/src/document/image.rs`）：`ImageAttrs` 经 attrs 键 `src`/`asset`/`alt`/`title`/`width`/`height` 读写，`ImageSource::AssetRef`（宿主 opaque 引用）与 `ExternalUrl`（codec/host 显式导入）二选一。`EditIntent::InsertImage` 把 Image 原子块作为聚焦块兄弟插入。`AssetService::resolve(AssetRef, Rc<dyn AssetSink>)`（`crates/xiaomu-runtime/src/assets.rs`）是 host capability seam：宿主拥有存储/网络/缓存/权限，`ResolvedAsset` 携带 `revision` 供 stale 判定，回调无前端上下文、不直接改 canonical document。GPUI 侧（`image_block.rs`）以 node identity + source key 缓存 `Arc<gpui::Image>`，Resolved 状态经 `gpui::ImageSource::Image` 绘制真实纹理（`w_full` + `max_h(320px)` + `ObjectFit::Contain`），Loading/Failed/无 service 状态渲染占位；accessibility 把 Image 投影为 alt 文本、HorizontalRule 投影为 Separator。

Clipboard wire v4 携带 atomic 载荷（`ClipboardNodeContent::Atomic`、`WireContent::Atomic`、`WireKind::HorizontalRule/Image`）；collapsed atomic selection 投影为单 atomic root 的 ClipboardSlice，粘贴为聚焦块后的兄弟块；mixed inline/atomic 层级粘贴 fail closed（`SessionError::ClipboardAtomicUnsupported`）。plain-text fallback 语义化：image copy 在 plain text 中携带 ExternalUrl。

**外部图片导入边界（2026-10-03 实验分支）：** 平台剪贴板现在优先有效 Xiaomu structured metadata，然后 PNG/JPEG encoded pixels，最后普通文本。图片只在非 CodeBlock 的 collapsed inline caret 导入；非空选区、atomic/cell range 和 composition 期间拒绝，保持原文档与历史。`AssetService::import_image` 为默认拒绝的可选同步能力；宿主验证和持久化后返回 `ImageAttrs(AssetRef)`，再发出单一 `InsertImage` history entry。宿主实现应限制尺寸/耗时；没有异步导入或外部文件读取。图片节点仍仅携带引用，不能假设跨宿主复制引用等于复制字节。

官方 harness 使用 `<store-path>.assets/` append-only sidecar，先 decode 验证 PNG/JPEG（≤16 MiB、各维≤4096、decoder allocation≤128 MiB），再同目录临时文件完整写入/sync/rename，最后插入节点。fixture snapshot 同样使用临时文件替换；这不是生产格式、通用 GC 或跨文件事务承诺。Undo 不删资产，Redo 可继续 resolve；保存撤销后的快照不会从 sidecar 扫描复活节点；重开用新 service 从相同 sidecar 读取实际 bytes。移走资产仅产生失败占位，不修改节点。`image_block` 丢弃不同旧 source resolve 并检查返回 asset identity，避免晚到回调污染当前纹理。

自动化覆盖见 `tests/image_paste.rs`、harness `assets/tests.rs` 和 `image_block/tests.rs`。真实 Linux/Windows/macOS 剪贴板与纹理显示需分别原生验收，不从自动化通过推断。详见 [图片实验验收](image-import-experiment.md)。

## P4.9 Markdown Baseline Codec 事实

`xiaomu-codec-markdown` 在 P4.9 建立真实 baseline codec（此前为 bootstrap 桩），只依赖 canonical Core model。覆盖：ATX heading、paragraph、quote、tight bullet/ordered list（含嵌套）、bold/italic/inline code/strikethrough/link、fenced code block（`language` attr）、hard break（backslash 形态导出，backslash 与两空格形态导入）、horizontal rule、ExternalUrl image。

契约是 refuse-instead-of-drop：canonical 文档携带 baseline 无法表示的内容时导出显式报错（`MarkdownCodecError`）——宿主 `AssetRef` 图片（URL 映射归宿主 adapter policy）、unknown attrs（含 image width/height）、inline atom、Custom node kind、empty paragraph、无法 round-trip 的空白。导入同样 fail closed：setext heading、indented code、lazy quote、inline image、reference link 不做静默重解释；unclosed emphasis 按字面读取（与 CommonMark fallback 一致）。canonical 导出形态固定：块间单空行、`- ` 列表、有序列表从 1 重新编号（canonical 无 list-start 属性）。round-trip 以"导出 → 导入 → 再导出"为固定点验证，Unicode（CJK/BiDi/emoji）文本与 alt 独立覆盖。

harness fixture 格式升级 **v4**：`hr\n` / `img\n` 原子行 + 既有 attrs 编码承载 image 语义（含 unknown extension tag）；v2/v3 读兼容；Custom node kind 仍 fail closed。demo fixture 加入 HR 与带 extension tag 的 Image。`xiaomu-runtime/tests/p4_final_gate.rs` 是最终矩阵：CJK+emoji 文本 + inline atom 缝输入、atomic 删除/undo 整节点选择、clipboard wire 往返、InsertImage canonical attrs、gap 经 atomic 删除的 mapping、多 editor 隔离。

## P4 Closeout

P4.9 的实现与三平台 CI 于 2026-09-05 完成。2026-10-01 审计更正了把 Windows CI job 当作原生实机 Gate 的错误；随后在 P5.6 同机验收补齐 Microsoft Pinyin 操作证据，并实际发现、修复 chip 旁预编辑不可见和 Esc 后排版残留。最终代码 `6d09167` 的 chip 前后输入/取消/提交、左右跨 atom、atomic → text 焦点恢复通过原生复测；具体版本、环境、操作者和步骤见 P5 progress。P4 CLOSED，但不据此声称 macOS/Linux IME 或所有 Unicode 组合均经过原生验收。

## P5.1-P5.2 Table 模型与 Cell 编辑事实

表格是普通树节点（`NodeKind::Table / TableRow / TableCell`），canonical 不变量（行数 ≥1、列数一致、cell 非空）由 `validate_tree` 持有，违规为 `Error::InvalidTableStructure`；`allows_child` 禁止 TableRow 进入 Document/Quote/ListItem/TableCell、禁止 cell 直接携带 InlineAtom。cell 支持完整 block 子树与嵌套表。构造不能经 `InsertNode` validated staging 表达，因此 Core 提供 `InsertTable { parent, index, rows, columns }` 与 `InsertTableRow { table, index }`：新 cell 含空 Paragraph，map 指向实际插入的 table/row，Runtime 按需解析新 row 内首个 inline 后代作 caret 目标；inverse 删除整子树，degenerate 输入 fail closed。Runtime `EditIntent::InsertTable` 在聚焦块后插入整表（单 isolated history entry，caret 原地保留）。

Cell 编辑（P5.2）复用既有 intent，无表格特例事务：Tab/Shift+Tab 走 `EditIntent::MoveToNextCell / MoveToPreviousCell`（`session/table.rs`），caret-only 导航——无事务、无 history；Atomic 焦点（cell 内 HR/Image）同样导航，Gap 焦点不导航。表内最后一个 cell 上 Tab 触发 `InsertTableRow` 并把 caret 落在新行首 cell，redo 经 `inverse(undo)` 恢复同一批 node id。cell 内 Enter/Backspace/Delete/typing/IME 全部走既有 parent-generic 路径：Backspace 在 cell 首段起点是有意 no-op（不跨 cell / 跨行 join），Enter 在 cell 内段落 split 留在原 cell，typing 在 cell 内照常 coalesce，IME commit 保持 isolated entry + stored marks 语义。测试：`crates/xiaomu-core/tests/table_model.rs`、`crates/xiaomu-runtime/tests/p5_cell_editing.rs`。

行列操作（P5.3）：插入走 `InsertTableRow { table, index }` 与 `InsertTableColumn { table, index }`。列插入使每行都新增 cell，所以一次 Core step 产生多个 `NodeInserted` map（每行一个，指向实际 cell），保证所有行的 `NodeGap` 都准确平移；不能只报告首行，也不能把插入节点伪装成 descendant paragraph。删除走单事务 `RemoveNode` 组合，Core 只验证最终快照；最后一行/列 fail closed。Runtime 先校验表身份/index，被删子树内 caret/selection 收敛到 `CaretAtGap`，其余 `MapExisting`；undo 恢复同一批 node id（含 atom）。测试：`p5_row_column_ops.rs`、`p5_review_regressions.rs` 与 Core `table_model.rs`。

Cell 选区与 clipboard（P5.5及后续逻辑网格）：`DocumentSelection` 携带 `Option<CellRange>`，端点为同表 cell identity；`CellRange::cells` 保留无跨度矩阵契约。跨度另用 `logical_rect/unique_origins/is_closed_rect`，origin规则排除从矩形上方/左方跨入的cell，不重复覆盖slot。矩形非 collapsed，不暴露成single-node text selection；text端点为合法停靠位置。通用Delete/Backspace清空选中origin的内容、每格留空Paragraph并保cell身份/attrs/形状；Toggle/Set/RemoveMark递归处理选中子树与inline atoms，一次全范围决定和一次历史。typing/plain paste/IME按unique origins替换，在gesture anchor接文本，其它选中cell留空P；空range proxy的IME仅接受0..0，普通inline范围不受影响。partial跨度clipboard与未定义结构命令仍拒绝。产品head-cell输入与整表Backspace删除需宿主显式策略。merge后的CellRange两端按Core身份映射到survivor，Undo恢复原内容、身份和逆向矩形。

Clipboard 捕获完整合法子树（含 quote/list、atomic、嵌套 table、marks、inline atoms）及 table/row/cell/block attrs。`Table { rows, row_attrs }` 的空 row_attrs 表示兼容旧载荷；非空行属性写 wire v6，普通表仍 v5。匹配 range paste 替换 cell 内容与 attrs、保留目标 table/row attrs；1×1 粘入 cell 时在 caret 所属直接 child block 后追加；兄弟表插入完整重建所有层级 attrs。非表片段可填充矩形各 cell 内容；尺寸不符/未定义落点 fail closed。hidden validated stages 统一提交一次 history，任何失败不发布中间文档。Table Markdown 导出仍 fail closed。回归见 `p5_cell_range_clipboard.rs`、`p5_review_regressions.rs`、`p5_rich_table_clipboard.rs`。

GPUI 表格点击先用 cell 全边界限制目标（含 padding、短 cell 空白与嵌套 cell），再按 block 的二维 bounds 命中并投影 caret。Ctrl/Cmd+Shift+Space 选格、Shift+方向键扩展；拖动 cell 左上角选择柄也可建立矩形。普通方向键/Escape 回到 focus cell 首个可导航后代。矩形由一个前端空 `ParagraphView` 代理接收原生输入：不增加 canonical 节点，复用现有 UTF-16/IME 投影，预编辑/取消不修改 snapshot；commit 交给 Runtime，一次历史替换矩形。替换及 undo/redo 后重新建立 child/focus，支持继续输入。代理与 document/history 都按 editor 隔离。

Up/Down 保留窗口坐标 desired-x：同格视觉行 → 同列相邻行 → 外层 cell / 文档。进入不等高行时按完整 cell bounds 选列；离开嵌套表先在外层 cell 查找，不横跳同一行的另一列。atomic-only cell 没有文本视觉行，Up/Down 跳过而 Tab 可访问。表格列 `min-width:0` 约束长文本，使普通 ParagraphView 的 wrapped layout 生效。`table_gpui.rs` 覆盖空格、不等高/换行/嵌套 cell、Unicode、鼠标、选区输入及原子格导航。

Harness writer 现统一写 fixture v5：`table` / `row` / `cell` 与 `end` 容器栈、各层 scalar attrs、rich blocks、atoms 和嵌套 table。无冗余行列数，形状由 Core 验证；reader 向后兼容 v2/v3/v4，但拒绝低版本信封的 table 标签。未支持的 node kind 或 list/object attr 返回错误，不能静默丢失。默认 fixture 已包含 Unicode、mention、引用和嵌套表；自动化 save/load 用 canonical semantics 比较。原生 IME 与三平台 CI 仍须独立验收，不从这些自动化用例推断阶段关闭。

## 仓库级约束

架构通过以下机制持续执行：

- `tools/check_dependency_boundaries.py` 检查 crate dependency direction；
- `tools/check_source_size.py` 执行 source-file size guardrail；
- CI 执行 Rust formatting、Clippy 和 tests；
- `cargo-deny` 检查 dependency source / license policy；
- `engineering-rules.md` 约束实现与文档同步。

## Linux stock-GPUI unmark semantics

Linux `unmark_text` now preserves a nonempty, non-rejected overlay through the existing canonical composition commit path before stock GPUI dispatches its pointer event. Explicit empty cancellation and already-committed input remain no-ops on subsequent unmark. No input-owner protocol, pointer wait barrier or dependency patch is introduced; focus-out and non-Linux callback handling remain unchanged. See [scope, regressions and native evidence boundaries](linux-unmark-preservation.md).


## Stock Linux XIM decoder source

The workspace overrides only `xim-ctext` with an audited local copy of the official Zed Git revision `16f35a2c881b815a2b6cdfd6687988e84f8447d8` (genuine upstream version 0.3.0, not the registry 0.3.0 release). Its small patch preserves literal ASCII and charset-return suffixes and rejects high-bit conversion overflow. The original revision and released 0.4.1 were rejected after reproducible ASCII-prefix regressions. GPUI and `zed-xim` stay stock registry packages; unknown Git sources remain denied, and a provenance/patch-reversal guard verifies the vendor. Downstream hosts must use this audited copy and repeat the root-only Cargo patch; dependency manifests cannot impose it. See [dependency rationale, integration instructions and native limits](linux-xim-decoder.md).
