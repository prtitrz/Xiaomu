# 晓木 Xiaomu 顶层规划

> Status: **EARLY / INDEPENDENT PROJECT**
>
> Date: 2026-08-21
>
> 定位：独立演进、可嵌入宿主应用的 **Rust Native Structured Rich-Text / Block Editor Engine**。首个原生前端基于 GPUI，但核心架构不绑定 GPUI。

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
   ├─ planning.md
   ├─ architecture.md
   ├─ document-model.md
   ├─ transaction-model.md
   └─ gpui-boundary.md
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

从架构上预留：

```text
TextPoint
NodeGap
NodeSelection
TextSelection
CellSelection
```

文本位置概念上类似：

```rust
pub struct TextPoint {
    pub node_id: NodeId,
    pub offset: TextOffset,
    pub affinity: CursorAffinity,
}
```

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

禁止各模块自行维护 offset 修补逻辑。

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

第一阶段 Cell 仅允许 paragraph-like inline fragment，后续再开放 richer fragment。

Selection：

```text
caret in cell
cell range
row / column axis selection
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

### 13.2 Accessibility scope

Accessibility 是晓木的长期质量要求，但不作为 P0/P1 的阻塞 Gate。Core 必须保留足够的结构语义与文本/selection query 能力，使 frontend 能构建可访问性树；不能把“Canvas/native paint”设计成只有像素、无法恢复语义的单向输出。

GPUI frontend 分阶段要求：

```text
P1/P2  keyboard-only editing path 完整
P2/P3  暴露可访问文本、角色、selection/focus 的 frontend seam
P4+    extension node/atom 提供 accessibility fallback
P7     在 GPUI 支持范围内加入 screen-reader smoke test
```

如果 GPUI 或平台当前缺少必要 accessibility API，应明确记录 limitation，并将兼容工作限制在 `xiaomu-gpui`；不能为了补 UI 框架缺口把平台类型带进 Core。

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

P2/P3 起增加 accessibility projection invariants；P7 在平台能力允许时增加 screen-reader smoke tests。

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

### P0 — Core Contract

目标：无 GPUI。

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

完成：

```text
drag / select all
cross-block copy / cut / delete
structured clipboard
history grouping
composition/history interaction
mapping regression matrix
persistence/change/focus integration fixture
```

Gate：Unicode cross-block + undo/redo invariant 全绿，Host Contract 无需产品专用类型即可完成真实持久化闭环。

### P4 — Inline Atom / Extension Seam

完成：

```text
InlineAtom
atom navigation/delete/copy
renderer registry
block renderer registry
host capability callbacks
extension + capability-service integration fixture
```

Gate：一个 demo atom 作为 one-caret-unit 完整操作，且 Core 无宿主业务类型。

### P5 — Table

完成：

```text
structured table model
cell editing
Tab / Shift+Tab
row / column operations
cell selection
```

Gate：表格中英文连续编辑 + undo/redo。

### P6 — Performance / Long Document

完成：

```text
layout cache
virtualization/windowing
large-document benchmark
memory/profile
multi-editor stress
```

### P7 — Library Stabilization

完成：

```text
public API reduction
semantic versioning
frontend compatibility policy
examples
docs
license / release automation
```

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
