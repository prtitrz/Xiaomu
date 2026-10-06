# 晓木 Xiaomu 顶层规划

2026-10-06嵌套溢出修复：measured table 各自拥有隐藏滚动条的横向 viewport，保留原列宽、
真实 child layout 高度及默认关闭 resize 的展示一致性。wheel 最近 owner、边缘传递、可见 hit
clip 与 scrolled resize 的确定性 offset clamp 由专门 virtual 回归覆盖；不改 Core/schema、
原始 oracle 或 stock GPUI，也不称 browser intrinsic sizing / 实际 GUI / IME 候选位置已通过。
详见[measured overflow 契约](measured-column-resize.md#measured-table-overflow-ownership)。

2026-10-06原生回归修复：column resize 保留整数 measured 起点约束，明确将从原 down
计算的 fractional native pointer 目标 clamp minimum 后投影为最近整数（精确 .5 向上）。
stock X11 16.16 decode 的普通40px拖动不再因约40.0006px delta被拒绝；原版 PM 保留小数，
此差异不扩张 canonical schema 或转换原始 JSON。纯计算与 virtual dispatch 回归不替代
修正 candidate 的真实 GUI 验收，见[宽度契约](measured-column-resize.md)。

2026-10-06独立后继：GPUI 在已 paint 的 focused selection-head 几何变化后请求 stock IME
坐标刷新，抑制稳定帧、后台与 hidden input 的重复通知，不改 Core/Runtime/GPUI 或原生
composition 协议。X11 composing 期间的同步旧-layout 查询仍独立保留，不能称候选位置
或整套 IME 原生验收已通过，见 [Input / IME](architecture.md#input--ime)。

2026-10-06增量：GPUI 默认关闭的 measured column-resize capability 将真实列边缘、
临时子树 reflow/caret geometry 与 host guard/单次 commit callback 分开。
Core/Runtime/persistence 不变；整数宽度、嵌套/span、安全取消及原生输入连续性由专门回归覆盖。
未测量 release-only 坐标的帧边界与宿主/真实 GUI 后继验收明确保留，
不称完整 PM 等价或完整产品迁移，见[契约](measured-column-resize.md)。

2026-10-05后继：Core `IsolateTableRect { table, rect }` 以至多五片隔离跨界 cell，
原top-left身份与rich子树保留，新片只建同kind空段落；不做span面积级unit展开。
typed guarded inverse复用原TableEdit，row/table元数据、切列宽度、allocator高水位
与精确Undo/Redo通过20新回归（含225矩形），全库1412/106 binaries+6docs和strict
门禁通过。预算为保守owned-payload会计而非RSS保证；不代表原版PM等价或产品GUI
已通过，宿主source/unsafe位置/defaults/repair仍是后继独立接线。

2026-10-05后继：Core 只读 `can_allocate_node_ids(count)` 供宿主在临时表格骨架构造前
预检真实剩余身份容量；不暴露 ID、不预留或放宽事务验证。两项新回归覆盖零/近 u64 边界、
clone 无修改与 Undo 后高水位/Redo 身份，workspace 1392 tests + 6 docs、fmt/strict Clippy
已过。该查询本身不代表宿主增长粘贴或真实 GUI 已验收，见[模型边界](architecture.md#document-value-layer)。

2026-10-05后继：显式`HistoryTimestamp`与独立typing-delay/selection-only选项，
成功publication才记时，全grouping状态失败回滚；GPUI每EditorInstance共享单调时钟
给所有输入surface。全库1390/106 binaries、Runtime621、GPUI400与strict门禁通过。
默认不变，缺失/倒退时间仅隔离不丢输入，IME/host等旧边界保持；产品原oracle回放/
opt-in/真实GUI另验，不称完整PM history，见[时间契约](timed-history-options.md)。

2026-10-05后继：独立构造期`DefaultTextInputMarks`仅让显式宿主在默认非空inline
InsertText/CommitComposition成功计划后恢复marks继承，默认/空输入/host plans/删除/
split/raw/staged不变；13个public-API回归、全库1320/104 binaries与严格门禁通过。
消费发生在原子publication中，
不拆typing组、不在listener后补修，见[范围](default-text-input-marks.md)。

2026-10-05后继：构造期固定`HistoryOptions`提供默认兼容的successful-traversal选区
capture及独立empty-history保持state选项。13项Runtime public/fault隔离回归、
全库1307测试/strict门禁通过；consumer按真实factory显式接线、GUI另验。时间分组、selection-only分组和非历史
正文bookmark mapping仍未实现，不能合称完整PM history，见[契约](history-traversal-options.md)。

2026-10-05后继独立stage：宿主显式`SessionPolicy::prepare_cut`与同session独占
`PreparedCut`复用现有commit完整预检，平台lossless item全部准备后再write→publish。
全库1294测试、Runtime9状态/历史/allocator回归、6借用compile-fail和7实际虚拟GPUI
协调器测试已过；产品原factory矩阵、严格门禁与原生GUI分开验收。投影-only opt-in
Cut仍拒绝，默认legacy保持原边界，不宣称OS确认或跨系统crash原子，见[契约](prepared-table-cut.md)。

2026-10-05后继：显式 `with_clipped_cell_ranges(NodeAttrs)` 已从恢复源码重建并
重新通过Runtime541、全workspace/all-targets1278、strict Clippy/fmt/source-size/
dependency-boundary门禁。30个新增用例涵盖四边裁切、rich保留/清空、行metadata、
width零值、v14/Closed/默认兼容和借用预算。spec从Copy变Clone且共享默认attrs，
Core/GPUI/default Paste/Cut权限未扩大；详见[表格导出契约](table-clipboard-export.md)。
消费者真实factory20 Copy/42 Paste证据已独立重生成，接入与原生GUI另行验收。

2026-10-05：在精确 v10 基底 `d918ad72` 上重新构建 per-view
`EditorRejection` API。新环境 Rust1.97.1 下全 workspace/all-targets 1248 tests
及strict Clippy/fmt/source/dependency/vendor门禁通过，包含9个公开路由事件回归、
composition静默与排队metadata拒绝stamp回归；不是丢失本地提交
`f60ba48` 或旧原生证据的恢复。宿主可按 Entity 绑定固定、无正文的拒绝反馈，
既有 `EditorHooks` 不变；内容无关的发出时revision供宿主过滤后来编辑已超越的
排队拒绝，精确覆盖与排除边界见 [architecture](architecture.md#multi-block-documentview)。
该拒绝反馈检查点的新原生宿主验收独立进行；其之后的Clipped Copy增量见上文。

2026-10-04：[显式全篇选择](adr/0010-explicit-root-selection.md) 修复host CtrlA与普通全文text range混淆的问题。每实例opt-in root gaps、closed clipboard v11、原生range输入/焦点与失败保护已具844全库测试/strictClippy证据；真实产品GUI待复验。旧默认router和普通open剪贴板拟合保持原边界。

> Status: **EARLY / INDEPENDENT PROJECT**
>
> Updated: 2026-10-01
>
> 当前里程碑（2026-10-01 收口）：**P0–P5 CLOSED；P4 原生 Windows Gate 缺口已随 P5.6 补齐；P6 未启动。** 最终代码 `6d09167` 的三平台 CI 与分版本原生验收见 [P5 progress](phases/p5-table/progress.md)，二者分别记录，前者不能替代后者。
>
> 定位：独立演进、可嵌入宿主应用的 **Rust Native Structured Rich-Text / Block Editor Engine**。首个原生前端基于 GPUI，但核心架构不绑定 GPUI。

本文是**唯一执行主线**：阶段顺序、交付范围、Gate 与未排期事项以此为入口。各阶段 `progress.md` 提供执行和验收证据；`architecture.md` 记录当前实现；已结束的审计进入 `archive/`，不作为另一套路线。文档导航见 [docs/README](README.md)。

2026-10-03 本地宿主迁移增量：[混合字号原型](mixed-font-size-prototype.md) 已通过纯解析/内核检查，仍为 `cfg(test)`，尚未接入生产排版。真实字体否证与下一公共 API 原型边界见该文档，不改变上述正式阶段完成口径。

2026-10-04：[typed HardBreak](adr/0009-typed-hard-break.md) 已具 canonical marks/identity、v10 剪贴板、seam-aware Core/Runtime split/join、mixed range marks/输入继承及 GPUI LF 投影。编辑接入全库829 tests通过；虚拟键盘全选/Copy/Cut/Undo和预编辑/提交marks一致性已覆盖。下一步是宿主完整命令与存储集成、真实字体/X11 IME验收；底层测试不替代产品HardBreak验收。

## 1. 项目定位

晓木解决的是 Rust 原生应用缺少成熟结构化富文本编辑基础设施的问题。

目标结构：

```text
Versioned XiaomuDocument
        ↓
transaction / mapping / selection / history engine
        ↓
DocumentSession / command dispatch
        ↓
frontend boundary
        ↓
GPUI native input + layout + paint
        ↓
Host application
```

晓木不是独立写作产品，也不拥有宿主 App Shell。它是一套可以被多个 Rust 原生应用嵌入的编辑引擎和 Native UI runtime。

宿主负责：

```text
business data
persistence
files / assets
networking / collaboration transport
window lifecycle
workspace / application shell
product configuration
```

晓木负责：

```text
document semantics
selection
transactions
position mapping
history
editing commands
native input semantics
command / session runtime
layout / paint projection
extension boundary
```

### 1.1 独立演进原则

晓木的核心设计以通用编辑器能力、正确性、可维护性和长期扩展性为最高优先级。

下游宿主的具体数据模型、历史兼容格式、业务实体、同步协议和 UI 组织方式不得进入 `xiaomu-core`，也不得迫使晓木形成产品专用分支。

为了降低大型宿主的接入成本，晓木必须主动维护稳定、窄而清晰的：

```text
Host Contract
Adapter Boundary
Extension Registry
Codec Boundary
Capability Services
```

如果宿主便利性与晓木自身架构发生冲突，优先保持晓木通用模型稳定，由宿主 adapter 完成适配。

这不是忽视集成成本。相反，晓木需要把“易嵌入”作为公开 API 的核心质量指标，同时避免用业务耦合换取短期接入便利。

---

## 2. 核心架构原则

### 2.1 文档语义与序列化格式分离

Markdown、HTML、JSON 或其他外部格式都只是 codec。

禁止把任何外部 source offset 当作文档 canonical position：

```text
External format
      ↕ codec
XiaomuDocument
      ↓
typed transaction
```

Core 的真相来源是结构化文档模型。

### 2.2 GPUI 不进入 Core

依赖方向必须保持：

```text
xiaomu-core
    ↑
xiaomu-runtime
    ↑
xiaomu-gpui
    ↑
host application
```

`xiaomu-core` 不允许出现：

```text
Window
App
Context
Entity
FocusHandle
GPUI event types
```

GPUI API 的 breaking change 应被限制在 `xiaomu-gpui`。

### 2.3 Block local editing boundary

一个可编辑 Block 负责：

```text
local text
local caret / selection projection
marked range / IME composition
local layout / hit-test
local edit intent
```

文档层负责：

```text
node tree
structural mutation
cross-node selection
transaction orchestration
history
position mapping
```

结构性操作不得由某个 Block 私自修改全局文档。

### 2.4 Host-neutral by construction

编辑器不拥有：

```text
file path
save / close lifecycle
workspace
application menu
networking
business database
product theme ownership
```

宿主通过 capability service 和 adapter 与晓木交互。

---

## 3. Workspace / crate 结构

```text
Xiaomu/
├─ Cargo.toml
├─ crates/
│  ├─ xiaomu-core/
│  │  ├─ document/
│  │  ├─ text/
│  │  ├─ selection/
│  │  ├─ transaction/
│  │  ├─ mapping/
│  │  ├─ history/
│  │  ├─ commands/
│  │  └─ table/
│  │
│  ├─ xiaomu-runtime/
│  │  ├─ session/
│  │  ├─ command_dispatch/
│  │  ├─ clipboard_model/
│  │  ├─ decorations/
│  │  └─ extension_registry/
│  │
│  ├─ xiaomu-gpui/
│  │  ├─ input/
│  │  ├─ block_view/
│  │  ├─ layout/
│  │  ├─ paint/
│  │  ├─ hit_test/
│  │  ├─ focus/
│  │  ├─ clipboard/
│  │  └─ virtualization/
│  │
│  ├─ xiaomu-codec-markdown/
│  └─ xiaomu-testkit/
│
├─ examples/
│  └─ editor_harness/
└─ docs/
   ├─ README.md
   ├─ planning.md
   ├─ architecture.md
   ├─ engineering-rules.md
   ├─ phases/
   ├─ adr/
   └─ archive/
```

第一阶段允许目录暂时少于上述结构，但依赖方向从第一天固定。

### 3.1 文件规模 guardrail

```text
<= 500 lines   preferred
501–700        review warning
> 700          split required unless generated/test fixture
```

按职责拆分 model / transaction / selection / mapping / input / layout / paint / hit-test / commands / table / history。

---

## 4. Canonical Document Model

第一版就按真正的结构树设计，避免将 `Vec<BlockNode>` 固化为长期 canonical contract。

### 4.0 Snapshot / mutation policy

`XiaomuDocument` 对外是不可变 snapshot。canonical document 的字段不公开可变访问，宿主、runtime、extension 都不能绕过 transaction 直接修改 NodeStore。

概念 API：

```rust
pub struct XiaomuDocument {
    version: DocumentVersion,
    root: NodeId,
    nodes: NodeStore,
    revision: DocumentRevision,
}

pub struct Node {
    id: NodeId,
    kind: NodeKind,
    attrs: NodeAttrs,
    content: NodeContent,
}
```

读取通过受控 getter / iterator / query API；修改只允许：

```text
XiaomuDocument
      + Transaction
      ↓
apply
      ↓
new XiaomuDocument snapshot
+ ChangeSet / Mapping / inverse information
```

内部实现优先采用 structural sharing，避免每次 transaction 深拷贝整棵树。第一阶段允许使用 `Arc`、copy-on-write、path cloning 或其他简单实现逐步验证，不在公开 contract 中绑定某个 persistent-collection crate。

要求：

```text
external immutability
stable NodeId
cheap-enough snapshots
structural sharing where practical
no public mutable NodeStore escape hatch
```

是否采用特定 HAMT / persistent vector / rope 属于性能实现决策，必须由 benchmark 驱动，不能提前写进 canonical API。

`NodeContent` 可以按节点类型表达：

```text
InlineContent
Children
TableContent
Atomic
Custom
```

第一阶段 built-in block：

```text
Document
Paragraph
Heading { level }
Quote
BulletList
OrderedList
ListItem
CodeBlock
HorizontalRule
Image
CustomBlock
```

Table 结构在核心模型中预留，但完整交互延后到独立阶段。

### 4.1 Inline model

第一版区分：

```text
TextRun
InlineAtom
```

`InlineAtom`：

```rust
pub struct InlineAtom {
    pub id: NodeId,
    pub kind: AtomKind,
    pub payload: ExtensionPayload,
    pub fallback_text: String,
}
```

编辑语义：

```text
one caret unit
atomic delete
atomic copy / move
IME cannot enter atom interior
```

可承载 future mention、reference、tag、entity chip、custom embed 等扩展。

### 4.2 Marks

首轮：

```text
Bold
Italic
Code
Underline
Strike
Link
```

Canonical 存储采用 **TextRun-local marks**，不维护独立的全局 mark range table。

概念上：

```rust
pub struct TextRun {
    text: TextBuffer,
    marks: MarkSet,
}
```

同一 inline container 内保持规范化：

```text
adjacent runs with identical MarkSet → merge
empty persistent TextRun → forbidden
mark order → canonicalized
invalid / duplicate mark attrs → rejected or normalized
```

selection、transaction 和 mapping 仍然基于文档位置，不以 TextRun 边界作为用户可见坐标。添加或移除 mark 可以拆分/合并 TextRun，但不能让外部观察者依赖某个 run 的瞬时分段。

IME composition 的临时 marked state 属于 runtime/frontend state，不通过伪造空 TextRun 写入 canonical document。

颜色、字体、对齐等表现属性后置，但 attrs 必须 versioned。

### 4.3 Unknown extension preservation

Versioned document 与 codec 必须 preservation-first。

未知 custom node、atom 或 attrs 在 decode → encode round-trip 中不能静默丢失。

---

## 5. Position / Selection Model

禁止使用裸 `usize` 作为跨层文档坐标。

所有 offset 都是 opaque newtype。

### 5.1 Text boundary

Core 内部使用受控 Unicode text boundary。

```text
TextOffset
TextRange
```

UTF-16 仅允许存在于 platform input adapter。

UTF-8 / UTF-16 转换集中在 text boundary 层。

### 5.2 Position types

单一 `node_id + offset` 不足以覆盖完整结构化编辑器。

当前已建立 / 预留：

```text
TextPoint
InlinePoint
NodeGap
NodeSelection
TextSelection
CellSelection
```

纯文本位置：

```rust
pub struct TextPoint {
    pub node_id: NodeId,
    pub offset: TextOffset,
    pub affinity: CursorAffinity,
}
```

P4.1 已建立 mixed-inline coordinate：

```text
InlinePoint(node_id, text_offset, atom_index, affinity)
```

其中 `text_offset` 继续严格表示 UTF-8 canonical text bytes；同一 text boundary 上 N 个 atom 对应 `atom_index = 0..=N` 的 N+1 个 caret gap。atom 不占 fake byte，不采用 U+FFFC/private-use sentinel，`CursorAffinity` 不承担 canonical atom order。P4.2 已合入 canonical atom placement 与 full-tree validation，Core document 可验证非零 ordinal；Runtime editing path 对 ordinal 的消费在 P4.3 完成，纯文本兼容路径继续使用 ordinal 0。

`CursorAffinity` 用于处理 soft wrap、BiDi 等同一逻辑位置对应多个视觉 caret 位置的情况。

Selection 由 anchor / focus 或专门的结构 selection 表达，跨 Block selection 属于 session/editor 层。

---

## 6. Transaction Model

所有用户编辑最终收敛成 typed transaction。

基础 steps：

```text
ReplaceText
SplitNode
JoinNodes
InsertNode
RemoveNode
MoveNode
SetNodeAttrs
AddMark
RemoveMark
WrapList
UnwrapList
InsertInlineAtom
RemoveInlineAtom
```

后续：

```text
TableInsertRow
TableDeleteRow
TableInsertColumn
TableDeleteColumn
TableSetCellContent
```

一个 transaction 至少携带：

```text
steps
before_selection
after_selection
history_group
origin
metadata
```

Core mutation 返回 inverse transaction 或足够生成 inverse 的 change set。

P4.1 确立、P4.2 落地的 mixed-inline mutation 约束：atom 前后 caret 可能共享同一 `TextOffset`，因此 atom seam 上的文本 replacement 由 `ReplaceInlineText { at: InlinePoint, end, replacement }` 消费 `InlinePoint.atom_index`；旧 `ReplaceText(TextRange)` 只能继续服务其语义无歧义的纯文本路径，在含 atom 的歧义 seam / range 上 fail closed。`ReplaceInlineText` 的替换区域内含 atom 时同样 fail closed，原子删除必须显式经过 `RemoveInlineAtom`。

### 6.1 Position Mapping

Position Mapping 是 P0/P2 之间必须建立的基础能力，不能等到协作或复杂 history 出现后再补。

每个 step / transaction 必须能够把旧位置映射到新文档：

```text
old selection
old decoration
old async anchor
old history anchor
       ↓
   StepMap / ChangeMap
       ↓
new position
```

这一能力服务：

```text
selection stability
undo / redo
async commands
decorations
future collaboration adapters
```

禁止各模块自行维护 offset 修补逻辑。P4.1 已让 `StepMap / ChangeMap` 暴露 `map_inline_point` seam；P4.2 的 atom-changing step（insert / remove / `InlineTextReplaced`）已进入这条同一 mapping engine 并显式调整 ordinal。

### 6.2 Collaboration stance

晓木当前不绑定 OT 或 CRDT。Core 采用 **collaboration-neutral** 立场：先保证单机 transaction、mapping、stable NodeId 和 local history 的语义干净，再允许未来协作层选择适合的同步模型。

从第一版保留这些兼容条件：

```text
stable / opaque NodeId
deterministic typed transactions
explicit ChangeMap / position mapping
versioned document schema
transaction origin / metadata
local history isolated behind a clear seam
no source-offset canonical positions
```

同时明确：

```text
local inverse transaction ≠ collaborative undo contract
local history grouping ≠ remote operation ordering
StepMap ≠ CRDT identifier model
```

未来 OT-style rebase 可以建立在 transaction/mapping 之上；CRDT adapter 也可以复用 document schema、NodeId 和 frontend，但允许它拥有独立的 operation identity、causal metadata、remote-merge 与 collaborative-history 实现。

因此“可接协作”是架构兼容目标，不承诺任何 OT/CRDT backend 可以零改动接入，也不允许尚未确定的协作方案提前污染 canonical document model。

---

## 7. Undo / Redo

History 基于 transaction/change set，不保存 Markdown snapshot。

文本输入支持 coalescing：

```text
continuous typing
→ one history group

structural command / paste / atom op
→ explicit history boundary

IME composition
→ composition state
→ commit enters history once
```

Undo / redo 必须同时恢复合理 selection。

Core invariant：

```text
apply(T)
apply(inverse(T))
≈ semantic original document
```

---

## 8. Input / IME

GPUI Windows 文本输入与 Microsoft Pinyin 的基础路线在项目初始化前已经做过实机可行性验证，因此 P0 不再增加独立 throwaway IME spike。

P1 仍必须重新通过晓木自身实现的完整 IME Gate。这里验证的是晓木的 text boundary、composition state、selection、history 与 GPUI adapter 是否组合正确，而不是重新证明 GPUI 是否存在基本输入能力。

GPUI 层是 platform adapter，不是 canonical editing model。

```text
platform UTF-16 range
        ↓
text boundary conversion
        ↓
local composition / selection state
        ↓
typed edit intent
        ↓
DocumentSession transaction
```

必须覆盖：

```text
Microsoft Pinyin continuous composition
candidate window
Chinese punctuation
mixed CJK / Latin input
emoji / surrogate pair
combining marks
selection replacement
marked text cancel / commit
focus restore
```

长期测试矩阵还需要覆盖 macOS IME 和 Linux ibus/fcitx。

---

## 9. Runtime / Command Boundary

默认结构：

```text
XiaomuDocument
      ↓
DocumentSession
      ↓
CommandDispatcher / extension handlers
      ↓
typed Transaction
      ↓
Frontend projection
```

`DocumentSession` 是 runtime 的唯一 canonical orchestration owner，负责：

```text
current document snapshot
current document-level selection
transaction application
history coordination
position mapping
command context
change notifications
extension command dispatch
```

Block local edit 只产生 intent：

```text
Enter
Backspace
Delete
Tab
Indent
Outdent
InsertText
SetMark
```

结构变化由 `DocumentSession` 解释为 transaction，输入层和 Block view 不直接修改全局树。

### 9.1 No generic BlockRuntime by default

第一阶段不建立一个职责宽泛的通用 `BlockRuntime`。Block 相关状态按性质分别归属：

```text
canonical content / attrs      → XiaomuDocument
document selection / history  → DocumentSession
IME / focus / pointer state    → frontend adapter
layout / hit-test cache        → frontend view state
extension command semantics    → typed handler / registry
```

只有当多个 frontend-neutral block 类型确实出现一组稳定、共享、无法合理归入上述层次的运行时职责时，才允许引入窄定义的 per-node runtime abstraction。不能为了架构图对称预先创建杂物层。

---

## 10. Render / Layout

Core document 与 frontend view state 分离。

GPUI frontend：

```text
DocumentSession
        ↓
BlockViewState
        ↓
TextLayout cache
        ↓
paint / hit-test
```

Block layout cache key 至少包含：

```text
node revision
content width
viewport constraints
typography revision
render-extension revision
```

跨 Block selection 的 visual range 由 frontend 投影到 mounted blocks，不写回 document model。

### 10.1 Virtualization readiness

第一阶段不要求完整虚拟化，但禁止把“所有 Block 永远 mounted”固化到公开架构。

预留：

```text
layout footprint cache
mounted block window
scroll anchor
recheck after measurement
```

### 10.2 Decorations

Decoration 是非 canonical 的瞬时视图信息，例如：

```text
search match
spellcheck / grammar underline
comment highlight
remote presence projection
AI diff / suggestion
debug / diagnostics overlay
```

Decoration 不写入 `XiaomuDocument`，也不参与文档 codec。`xiaomu-runtime` 可以维护 frontend-neutral 的 `DecorationSet` / anchor model，并通过 ChangeMap 随 transaction 映射；具体 paint、z-order、hover 和 hit-test 由 frontend 负责。

如果某种 annotation 需要持久化为文档语义，应显式建模为 mark、node、atom 或 extension payload，不能偷偷借 decoration 存储 canonical 数据。

---

## 11. Inline Atom / Extension Boundary

Atomic inline extension 提前于 Table，用它验证扩展边界是否足够干净。

P4.1 已先固定 coordinate seam，防止 canonical atom 引入后再返工 P0-P3 的 UTF-8 text contract：

```text
TextOffset = text bytes only
InlinePoint = text boundary + atom ordinal + visual affinity
```

第一阶段保留两个 registry：

```text
InlineAtomRendererRegistry
BlockRendererRegistry
```

extension 可以提供：

```text
rendering
hit-test / action
optional command handlers
serialization payload schema
```

extension 不拥有宿主业务数据库。

宿主只将 opaque/stable payload 交给晓木，并通过 capability 回调处理业务动作。

---

## 12. Table

Table 采用结构化模型，不退化为字符串。

```text
TableNode
├─ rows
│  └─ cells
│     └─ CellContent
├─ column metadata
└─ attrs
```

P5 已采用 `Table → TableRow → TableCell → block…` 的普通树模型，cell 至少一个 block；允许 paragraph、heading、quote、list、code、atomic block 和嵌套 table。剪贴板须完整保留合法子树及各层 attrs。列数由每行 cell 数推导，列宽仍属前端布局，不在 canonical 中冗余存储。

Selection：

```text
caret in cell
cell range
row / column axis selection（后续扩展，不作为 P5 已实现能力）
```

Tab / Shift+Tab 属于 table command。

---

## 13. Host Contract

最终宿主 API 应保持小而稳定，概念上类似：

```rust
XiaomuEditor::new(document)
editor.document()
editor.selection()
editor.is_dirty()
editor.apply(command)
editor.undo()
editor.redo()
editor.focus()
```

Host services：

```text
ClipboardService
AssetService
LinkOpenService
ExtensionRegistry
Theme / Typography input
PlatformCapabilities
```

晓木不感知具体数据库、工作区模型、业务实体和同步协议。

### 13.1 Integration quality gate

“宿主中立”不能成为难接入的借口，但真实产品也不能成为 Core 的架构驱动者。

Integration Gate 从 P2 开始，而不是等到 P7 才第一次验证宿主边界：

```text
P1  standalone native input harness
P2  minimal host-contract harness
P3  persistence/change/focus integration fixture
P4  extension + capability-service integration fixture
P7  stabilized realistic host integration harness
```

这些 harness 可以是晓木仓库内的 realistic fixture，不要求任何具体产品在开发阶段反向成为晓木依赖。下游真实应用可以从 P2/P3 开始试接，用实际需求暴露 Host Contract 问题；如果需求与晓木通用性冲突，由下游 adapter 解决。

持续验证：

```text
create editor
load document
listen to changes
persist through adapter
restore selection/focus
multiple editors coexist
apply host extensions
inject theme
resolve assets
```

公开 Host Contract 发生变化时，对应 integration harness 必须同步通过。

P3 已通过可复用 `EditorInstance` 与 multi-editor fixture 验证 load/change/persistence/full-selection restore/native-focus routing、listener 与 history/session isolation，Host Contract 无需产品专用类型即可完成真实闭环。

### 13.2 Accessibility scope

Accessibility 是晓木的长期质量要求，但不作为 P0/P1 的阻塞 Gate。Core 必须保留足够的结构语义与文本/selection query 能力，使 frontend 能构建可访问性树；不能把“Canvas/native paint”设计成只有像素、无法恢复语义的单向输出。

GPUI frontend 分阶段要求：

```text
P1/P2  keyboard-only editing path 完整
P2/P3  暴露可访问文本、角色、selection/focus 的 frontend seam
P4+    extension node/atom 提供 accessibility fallback
P7     在 GPUI 支持范围内加入 screen-reader smoke test
```

P3 已完成 frontend-neutral `AccessibilityProjection`，覆盖 editable text、semantic role/kind、selection 与真实 focus owner。当前精确 pin 的 GPUI `0.2.2` 缺少后续版本公开的角色 builder，因此平台 AccessKit tree adapter 保留在 `xiaomu-gpui` 的未来升级工作中；该限制不允许把平台类型带进 Core。

---

## 14. Codec Policy

### Markdown

仅：

```text
XiaomuDocument ↔ Markdown
```

用于 import / export / interchange。

禁止：

```text
Markdown source offset = canonical editor position
```

### HTML

后续独立 codec。

### External editor formats

任何第三方 editor JSON 或产品历史格式都由外部 adapter 负责，不进入 Xiaomu core。

---

## 15. Test Strategy

### 15.1 Core invariant / property tests

必须覆盖：

```text
every transaction keeps document valid
inverse restores semantic document
selection always points to a valid position
mapping produces valid positions
split / join inverse
list nesting invariants
unknown extension preservation
table rectangular invariants
```

### 15.2 Unicode regression matrix

固定 fixture：

```text
ASCII
中文
中英混输
emoji / surrogate pair
combining marks
CJK + emoji cross-block
BiDi samples
```

任何 byte offset 落到非法 UTF-8 char boundary 都应在 API 层无法构造或返回可控错误，不允许 panic。

P3.7 已将这组固定 matrix 同时落到 Runtime cross-block/history invariants 与真实 GPUI wrapped-navigation fixture，并以 deterministic randomized history/mapping sequence 验证完整 undo/redo。

### 15.3 Native interaction harness

实机 Gate：

```text
IME composition
local selection
cross-block selection
copy / cut / paste
undo / redo
list Enter / Backspace
inline atom navigation/delete
multi-editor focus isolation
keyboard-only operation
```

P3 最终 Windows Gate 已覆盖 IME、Unicode、wrapped navigation、cross-block clipboard/history、list structural editing、scroll/focus/keyboard-only 与 persistence，2026-09-01 PASS，无缺陷。P2/P3 accessibility projection invariants 已建立；P7 在平台能力允许时增加 screen-reader smoke tests。

Table 阶段增加：

```text
cell navigation
Tab / Shift+Tab
row / column operations
table undo
```

### 15.4 Fuzz / random transactions

Core 尽早加入随机 transaction sequence + inverse replay + mapping invariant fuzz。

---

## 16. Roadmap

<a id="delivery-boundaries"></a>

### 交付边界与待排期编辑功能（2026-10-01）

阶段 **CLOSED** 表示对应 contract / Gate 已闭合，不等价于完整写作产品的全部交互已交付。尤其要区分 canonical/API、frontend 接入和宿主资源管理。

| 能力 | 当前已交付 | 尚未交付 / 排期 |
| --- | --- | --- |
| 文本、格式与结构编辑 | 原生输入/IME、基础格式快捷键、跨块选区与 history、列表、代码块、HardBreak | 完整工具栏、菜单等产品 UI 不由阶段 CLOSED 自动承诺 |
| 图片节点与显示 | `InsertImage`、typed image attrs、atomic selection/delete/undo、注入 `AssetService` 后的图片渲染 | harness 尚未注入真实资产服务，示例图片仍为占位；宿主资产导入/存储示例待排期 |
| 图片复制粘贴 | 已有 Image 节点通过 Xiaomu structured clipboard 保留图片语义和引用；资产仍由宿主管理 | 实验分支已接 PNG/JPEG Ctrl+V 与可选 host import；harness 持久化真实字节，原生验收另记；跨宿主 structured copy 不会自动复制资产字节 |
| 图片文件与后续操作 | 复用 image / asset contract 的基础具备 | 文件选择、文件拖入、交互式缩放/裁剪均未交付；不与第一版截图粘贴捆绑 |
| 链接 | canonical Link mark、structured clipboard 与 baseline Markdown 保留链接语义 | 链接添加/编辑 UI 待排期；宿主打开回调 `LinkOpenService` 已列入 P7 |
| 表格 | P5 范围内的 cell 编辑、导航、行列操作、矩形选区与 clipboard | 长文档/复杂表格性能基线由 P6 建立，不据正确性 Gate 宣称性能已达标 |

**当前正式顺序仍是 P5 → P6 → P7。** 新发现的外部图片粘贴缺口不算作 P4 已交付，也不自动推迟到 P7；需单独确认优先级。建议在大规模性能优化前安排一个窄的“图片导入可用闭环”切片，2026-10-03 已授权在独立实验分支先实施此切片，再评估宿主接入；不改变 P0–P5 历史 Gate 或宣称 P6 已启动。实验范围与证据见 [图片实验验收](image-import-experiment.md)。

该切片至少需要在启动时明确并验收：

- 平台读取截图/图片载荷，定义与 Xiaomu structured metadata / 普通文本的粘贴优先级，以及格式、尺寸与失败行为。
- 宿主导入并保存资产，产生稳定 `AssetRef`，通过 `AssetService` 解析；文件/网络/权限策略不进入 Core。
- 粘贴落点与选区策略明确，一次粘贴对应一个 undo/redo 步骤；异步导入期间的编辑、取消及 editor 生命周期不能导致插错位置或跨 editor 写入。
- harness 能粘贴真实图片并显示；保存、关闭、重开后资产仍能解析；错误有反馈，既有文本/表格 clipboard 和 IME 不回归。
- 先通过 Windows 原生截图粘贴 Gate；macOS/Linux 分别记录实际支持与验收情况，不能用三平台编译通过代替剪贴板实机验收。

文件选择/拖入、链接编辑 UI、图片尺寸控制分别作为后续待排期项，不把裁剪、图库或图文环绕混入最小闭环。

### P0 — Core Contract

状态：**CLOSED**

完成：

```text
versioned document schema
externally immutable document snapshot
NodeId / NodeStore + structural-sharing prototype
TextRun-local normalized marks
TextOffset / text boundary
position / selection model
basic transactions
StepMap / ChangeMap prototype
validation
inverse prototype
```

Gate：Unicode / CJK / emoji / property tests 全绿。

### P1 — Single Block Native Input

状态：**CLOSED**

完成：

```text
GPUI adapter
paragraph
caret / local selection
IME composition
copy / paste
basic marks
```

Gate：真实 IME + selection + undo。

### P2 — Document Tree / Structural Edit

状态：**CLOSED**

完成：

```text
multi-block
split / join
heading / quote
list
keyboard navigation
document selection
position mapping stabilization
minimal host-contract harness
```

Gate：paragraph → list → paragraph 日常编辑闭环，且最小宿主可加载、监听变更并保存文档。

### P3 — Cross-block Selection / History

状态：**CLOSED（2026-09-01）**

完成：

```text
soft-wrap / visual-line geometry
visual navigation / selection / hit-test
drag / select all
cross-block copy / cut / delete
structured clipboard
history grouping + StoredMarks
composition/history interaction
HardBreak / CodeBlock multiline contract
accessibility text/role/selection/focus projection seam
realistic EditorInstance host integration
multiple-editor focus / persistence / history isolation
Unicode/CJK/emoji/combining/BiDi cross-block matrix
randomized history / mapping invariants
P0/P1/P2/P3 regression matrix
Windows final real-machine Gate
```

Gate：固定 Unicode cross-block + visual-line matrix、exact undo/redo 与 randomized history/mapping invariants 全绿；Host Contract 无产品专用类型即可完成真实 load/change/persistence/selection/focus 闭环；Windows 最终实机 Gate PASS；三平台 workspace tests、fmt、Clippy、source-size、dependency-boundary 与 policy 全绿。

### P4 — Structured Content / Extension Seam

状态：**CLOSED**。2026-10-01 审计发现的原生 Windows Gate 证据缺口已在 P5.6 补齐，含 chip 两侧 IME 与 atomic → text 焦点恢复；分版本操作记录见 P5 progress。

P4.1 与 P4.2 Core 层已完成：

```text
ADR 0005 mixed-inline coordinate contract
InlinePoint(node_id, text_offset, atom_index, affinity)
TextPoint ↔ InlinePoint ordinal-0 compatibility
StepMap / ChangeMap mixed-inline mapping seam
Runtime DocumentPosition / DocumentSelection compatibility seam
GPUI DocumentView mixed-inline focus / selection projection
P0-P3 zero-semantic-regression gate
InlineAtom canonical model（AtomKind / InlineAtomContent / placement / validation）
InsertInlineAtom / RemoveInlineAtom / RestoreInlineAtom
atom-aware ReplaceInlineText contract + InlineTextReplaced mapping + exact inverse
```

P4.3–P4.9 已实现：

```text
atom navigation/delete/copy（Runtime / GPUI）
renderer registry
host capability callbacks
extension accessibility fallback
extension + capability-service integration fixture
atomic node selection / image contract / asset service / baseline Markdown codec
```

`BlockRendererRegistry` 尚未交付，不能与已实现的 `InlineAtomRendererRegistry` 混记。2026-10-01 明确将通用 block renderer 扩展与 `LinkOpenService` 列入 **P7 Host Extension Contracts**（见 P7 验收项），不计作 P4/P5 已交付。现有内置 atomic renderer 与 image URL fallback 不等价于通用宿主接口。

Gate：一个 demo atom 作为 one-caret-unit 完整操作，文本/atom seam 不污染 UTF-8 `TextOffset` contract，undo/redo 与 mapping 精确，且 Core 无宿主业务类型。

### P5 — Table

状态：**CLOSED（2026-10-01）**。P5.6、review correctness 修正、原生 IME 阻塞修复及独立 Gate 均已完成。

已建立的能力：

```text
structured table model
cell editing
Tab / Shift+Tab
row / column operations
cell selection
```

Gate：表格中英文连续编辑 + undo/redo、所有行列位置映射、富 cell clipboard 无损、用户可操作的矩形选区与视觉列导航、fixture round-trip、多 editor 隔离、三平台 CI、独立 Windows 原生 IME 验收。上述项已闭合，证据和原生验收范围见 `docs/phases/p5-table/progress.md`。

### P6 — Performance / Long Document

状态：**未启动**。先建立长文档、复杂表格与多 editor 的可重复 benchmark/profile 基线，测量编辑延迟、布局/绘制耗时及内存；依据瓶颈决定缓存和 virtualization 的实施顺序，不能以缓存命中率替代 selection/IME/undo 正确性。

阶段目标：

```text
layout cache
virtualization/windowing
large-document benchmark
memory/profile
multi-editor stress
```

### P7 — Library Stabilization

状态：**未启动**。

阶段目标：

```text
public API reduction
semantic versioning
frontend compatibility policy
examples
docs
license / release automation
Host Extension Contracts: BlockRendererRegistry / LinkOpenService
```

P7 Host Extension Contracts 由 GPUI/host 层交付，不下沉 Core/Runtime：`BlockRendererRegistry` 按稳定 kind key 注册并对未知 kind 提供无损 fallback，验收自定义 atomic block 渲染、a11y 与多 editor 隔离；`LinkOpenService` 由宿主显式注入，验收文本 link 与 image URL fallback 调用、未注入时不私自联网/启动应用、URL 保留及多 editor 隔离。规划入口保留到该切片验收完成。

---

## 17. GPUI Dependency Policy

GPUI frontend 单独 pin 明确 revision/version。

规则：

1. `xiaomu-core` 不依赖 GPUI。
2. 尽量保持 `xiaomu-runtime` 也不依赖 GPUI。
3. GPUI compatibility 变化限制在 `xiaomu-gpui`。
4. 每次 GPUI 升级单独 PR。
5. CI 输出或校验 resolved dependency source/revision。
6. 如果 GPUI breaking change 穿透 Core，视为架构回归。

长期允许增加其他 frontend，而不改变 canonical document model。

---

## 18. API / Compatibility Policy

早期 `0.x` 允许快速调整，但仍坚持：

```text
canonical document versioning
explicit migration boundary
no silent data loss
extension payload preservation
public API surface kept small
```

进入稳定阶段后分别管理：

```text
Document format compatibility
Rust public API compatibility
Frontend compatibility
Codec compatibility
```

四者不能混为一个版本问题。

---

## 19. Scope Control / Stop Gates

这是万行级长期基础设施项目，不按“小编辑控件”估算。

阶段 Gate：

```text
P0/P1 Unicode + IME correctness failure
→ STOP / REWORK

P2/P3 transaction + mapping + history complexity uncontrolled
→ reduce scope before adding Table

UI framework API leaks into Core
→ architecture REWORK

product-specific types enter Core
→ remove through adapter boundary

public mutable access bypasses transaction invariants
→ API REWORK

collaboration prototype requires canonical source offsets or rewrites document semantics
→ reject adapter design / reassess seam
```

不要用已经投入的代码量作为继续扩大 scope 的理由。

---

## 20. Design References / Prior Art

晓木独立实现，不以兼容任何既有编辑器为目标，但设计和测试应主动研究成熟系统已经付过成本的地方。优先参考：

```text
ProseMirror  → schema / transaction / step mapping / selection
Lexical      → immutable editor state / update boundary / extension discipline
Slate        → extensible structured document model and normalization lessons
xi-editor    → Rust text architecture、async/edit pipeline 的经验与止损教训
Parley       → Rust text layout / shaping / editing primitives
```

这些项目用于理解问题和建立 conformance/invariant 思维，不复制它们的产品边界，也不要求晓木复刻其 API。新增重大机制前，优先检查成熟实现如何处理 selection mapping、IME、undo、Unicode、BiDi、clipboard 与 schema evolution，减少重复踩坑。

---

## 21. Long-term Direction

目标结构：

```text
                  ┌─ Markdown codec
                  ├─ HTML codec
XiaomuDocument ───┤
       ↓          └─ external adapters
Transaction Engine
       ↓
Position Mapping
       ↓
DocumentSession
       ↓
Frontend Boundary
       ↓
GPUI Native Surface
       ↓
Host
```

长期原则：

> **文档语义独立于序列化格式。**
>
> **编辑引擎独立于 App Shell。**
>
> **UI 框架独立于 Core。**
>
> **宿主需求通过适配边界进入，不能反向定义晓木。**

晓木成功的判断标准不只是“能编辑富文本”，还包括：文档模型稳定、Unicode 与 IME 正确、事务可组合、位置可映射、扩展可保留、宿主易嵌入，并且这些能力不会因为某个具体产品的接入而失去通用性。
