# P4 Inline Atom / Extension Seam 设计

## 1. 目标

P4 用一个真正的 inline atom 验证晓木的 extension boundary 是否成立。

阶段完成后，编辑器至少应支持一种 demo atom，并满足：

```text
stable canonical identity
one-caret-unit navigation
atomic delete
copy / cut / paste + plain-text fallback
undo / redo + mapping
GPUI renderer registry
host capability callback
accessibility fallback
no host business type in Core / Runtime
```

典型未来承载对象包括 mention、reference、tag、entity chip、inline embed。P4 不把任何具体产品实体写入 canonical contract。

## 2. P4 先处理坐标，再处理 atom

P0-P3 的 `TextOffset` 已由 ADR 0001 固定为 validated UTF-8 byte offset。`InlineContent` 当前只有 normalized `Vec<TextRun>`，因此文本坐标、selection、ReplaceText、mapping、IME 都建立在纯 UTF-8 inline surface 上。

inline atom 打破这个前提。atom 需要是不可进入内部、可左右跨越的一单位内容，同时允许多个 atom 相邻。

P4 明确拒绝三条捷径：

```text
不把 U+FFFC / 私用字符伪装成 atom 写进 canonical text
不让 atom 占用一个伪 UTF-8 byte
不重载 CursorAffinity 来区分多个相邻 atom
```

这些方案会污染已经成立的文本坐标语义，或在多个相邻 atom 时失去唯一位置。

P4.1 先建立 mixed-inline coordinate seam。该 seam 通过 P0-P3 全量 regression 后，P4.2 才把 atom 写入 canonical document。

## 3. Coordinate contract

保留：

```text
TextOffset = UTF-8 byte offset in concatenated text only
TextRange  = text-only half-open byte range
```

新增 `InlinePoint`：

```text
InlinePoint
  node_id
  text_offset: TextOffset
  atom_index: usize
  affinity: CursorAffinity
```

在某个合法 `text_offset` 上可能锚定 N 个 atom，`atom_index` 表示该位置之前已有多少个 atom，合法范围为 `0..=N`。

例如：

```text
A [atom-1] [atom-2] B
```

两个 atom 都锚定在 text offset 1：

```text
(1, 0)  A | atom-1 atom-2 B
(1, 1)  A atom-1 | atom-2 B
(1, 2)  A atom-1 atom-2 | B
```

因此：

- atom 可以相邻；
- 每个 atom 恰好占一个 caret step；
- UTF-8 byte coordinate 保持原义；
- soft-wrap / BiDi 视觉歧义继续只由 `CursorAffinity` 处理；
- text storage 和 platform UTF-16 adapter 继续保持原边界。

纯文本 inline node 上 N 永远为 0，因此 `InlinePoint(text_offset, 0)` 与现有 P0-P3 caret 位置一一对应。

P4.1 保留 `TextPoint`，并建立兼容 seam：

```text
TextPoint
↕ exact conversion while atom_index == 0
InlinePoint

Core StepMap / ChangeMap
→ map InlinePoint text component
→ preserve ordinal in P4.1

Runtime
→ accept/project InlinePoint ordinal 0
→ reject non-zero ordinal until canonical placements exist

GPUI DocumentView
→ project current text focus/selection as InlinePoint
```

P4.2 有了真实 atom placement 后，再升级 Runtime 内部 position storage 与 GPUI editing/navigation path。

## 4. Canonical atom representation

P4.2 计划采用 stable `NodeId` 作为 atom identity，不建立第二套 AtomId allocator。

atom 自身是 canonical node：

```text
NodeKind::InlineAtom(AtomKind)
NodeContent::InlineAtom(InlineAtomContent)
NodeAttrs = extension payload
```

其中：

```text
AtomKind
  stable extension key

InlineAtomContent
  fallback_text
```

`fallback_text` 是 Core 级通用语义，用于 plain-text clipboard、accessibility fallback、unknown renderer fallback，不放进魔法 attr key。

atom 没有 editable child。P4 初版不定义 markable atom；如果以后出现稳定通用需求，再单独增加 contract。

## 5. InlineContent placement

为了避免 P4 一次性推翻全部 P0-P3 text-run API，`InlineContent` 的逻辑模型扩展为：

```text
normalized text runs
+ ordered inline atom references
```

每个 atom reference 包含：

```text
atom NodeId
text_offset anchor
```

同一 `text_offset` 可有多个 atom，vector order 即 canonical order。

内部具体存储可以先采用 runs + atom placements，公开 contract 只保证 deterministic ordered inline iteration，不承诺长期物理 representation。

文档验证必须把 inline atom reference 当成真实 tree edge：

```text
reference target exists
kind/content shape is InlineAtom
atom has exactly one parent
atom cannot appear in structural Children
atom cannot be document root
unreachable atom is invalid
placement text_offset is a valid UTF-8 boundary
```

`XiaomuDocument::parent_of`、full-tree validation、NodeStoreBuilder child/reference validation 都要覆盖这类 edge。

## 6. Transaction / mapping direction

P4 不让旧 `ReplaceText` 静默穿过 atom，也不允许它在 atom seam 丢掉 ordinal 语义。

先看最小例子：

```text
A [atom] B
```

atom 位于 text offset 1。下面两个 caret 的 byte offset 都是 1：

```text
(1, 0)  A | atom B
(1, 1)  A atom | B
```

两处分别输入 `X` 应得到：

```text
A X atom B
A atom X B
```

因此单纯的 `ReplaceText(TextRange::empty(1))` 无法表达 mixed-inline seam 上的全部文本编辑。

约束：

```text
ReplaceText / AddMark / RemoveMark
  → 保持 text-only contract
  → 在含 atom 的歧义 seam / range 上 fail closed

InsertInlineAtom / RemoveInlineAtom
  → atom 专用 typed step
  → 产生 atom-aware StepMap

mixed-inline text replacement
  → transaction 必须看到 InlinePoint boundary
  → atom_index 参与 canonical order / placement mapping
```

P4.2 在 canonical atom / placement 成立时必须把 atom-aware replacement 的最小 Core contract 定下来；具体 runtime editing orchestration 可以在 P4.3 完成。不能等到 GPUI 层再修 placement。

后续 split / join 遇到 atom 时同样必须有明确 placement mapping。

atom 相关 inverse 必须精确恢复：

```text
NodeId
AtomKind
NodeAttrs payload
fallback_text
placement
selection
```

## 7. Runtime editing semantics

P4.3 负责：

```text
Left / Right 跨 atom 一次一个 unit
Backspace / Delete 在 atom 邻接处原子删除
selection 可跨 text + atom
atom seam text input 使用 atom-aware replacement
copy/cut 产生 detached atom fragment
plain fallback 使用 fallback_text
paste 恢复 atom identity-independent fragment
undo / redo 恢复 selection
IME composition 永远不能进入 atom 内部
```

clipboard fragment 不复制 live `NodeId`，粘贴时重新分配 canonical identity，保持与 P3 structured clipboard 相同的 detached-value 原则。

## 8. GPUI / extension seam

P4.4 增加 host-neutral registry：

```text
InlineAtomRendererRegistry
AtomKind -> renderer
```

renderer 只拿 canonical atom projection 与 frontend context，不持有宿主数据库对象。

宿主动作通过 capability callback：

```text
atom action
→ stable kind / payload / action key
→ HostCapability
→ host handles business behavior
```

GPUI renderer 缺失时必须有 deterministic fallback，至少显示 `fallback_text`，同时 accessibility projection 也使用同一 fallback 语义。

## 9. 分片计划

### P4.1 Inline Coordinate Contract

交付：

```text
InlinePoint + validation
text-only compatibility conversion
Runtime position compatibility seam
Core mapping seam
GPUI focus/selection projection seam
P0/P1/P2/P3 full regression
ADR 0005
```

Gate：没有 atom 的现有文档行为零语义回归；`TextOffset` 仍严格是 UTF-8 byte coordinate；非零 atom ordinal 在 canonical placement 建立前 fail closed。

当前实现 head `a2edcb35e4634b85294725ea1d19278276e754ae` 的 CI run #295 已完整 success。

### P4.2 Canonical Inline Atom

交付：

```text
AtomKind
InlineAtomContent
NodeKind / NodeContent shape
InlineContent atom placement
full-tree validation
InsertInlineAtom / RemoveInlineAtom
atom-aware replacement contract
mapping / inverse
```

Gate：相邻两个 atom 可稳定构造、validate、insert/remove、undo/redo；不存在 sentinel text；atom seam 上的文本 mutation 不丢失 `atom_index`。

### P4.3 Runtime Atom Editing

交付：

```text
one-unit navigation
atomic Backspace/Delete
mixed selection
atom-aware text input
clipboard fragment / fallback
IME exclusion
history regression
```

Gate：`text + atom + atom + text` 可纯键盘编辑，selection/history 始终合法。

### P4.4 GPUI Renderer / Capability

交付：

```text
renderer registry
demo atom
layout / hit-test / paint
accessibility fallback
host capability callback
```

Gate：未知 renderer fail soft 到 fallback；宿主动作不把 business type 带进 Core/Runtime。

### P4.5 Integration / Closeout

交付：

```text
realistic extension fixture
multi-editor isolation
Unicode + atom matrix
Windows real-machine gate
architecture / planning / progress
CI Success
```

## 10. Scope boundary

P4 明确不做：

```text
Table
block embed framework
collaboration protocol
product-specific mention lookup
rich editable content inside atom
markable atom
virtualization redesign
GPUI upgrade
```

如果 atom implementation 迫使 `TextOffset` 改成混合单位、要求宿主对象进入 Core，或只能靠 frontend ad-hoc 修 atom placement，应停止实现并重新评审 coordinate / extension seam。
