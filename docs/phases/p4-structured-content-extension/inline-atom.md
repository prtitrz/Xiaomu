# P4A Inline Atom / Extension Seam 设计

## 1. 目标

P4A 用一个真正的 inline atom 验证晓木的 mixed-inline coordinate 与 extension boundary 是否成立。

完成后至少支持一种 demo atom，并满足：

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

典型未来承载对象包括 mention、reference、tag、entity chip、inline embed。P4A 不把任何具体产品实体写入 canonical contract。

P4A 是统一 P4 的前半段；Atomic Block / Image / AssetService 紧随其后进入 P4B，见 `atomic-media.md`。

## 2. 先处理坐标，再处理 atom

P0-P3 的 `TextOffset` 已由 ADR 0001 固定为 validated UTF-8 byte offset。inline atom 需要是不可进入内部、可左右跨越的一单位内容，同时允许多个 atom 相邻。

P4A 明确拒绝：

```text
不把 U+FFFC / 私用字符伪装成 atom 写进 canonical text
不让 atom 占用一个伪 UTF-8 byte
不重载 CursorAffinity 来区分多个相邻 atom
```

P4.1 先建立 mixed-inline coordinate seam；P4.2 再把 atom 写入 canonical document。

## 3. Coordinate contract

保留：

```text
TextOffset = UTF-8 byte offset in concatenated text only
TextRange  = text-only half-open byte range
```

新增：

```text
InlinePoint
  node_id
  text_offset: TextOffset
  atom_index: usize
  affinity: CursorAffinity
```

在一个合法 `text_offset` 上若有 N 个 atom，`atom_index` 表示 caret 前已有多少个同 offset atom，合法范围 `0..=N`。

例如：

```text
A [atom-1] [atom-2] B

(1, 0)  A | atom-1 atom-2 B
(1, 1)  A atom-1 | atom-2 B
(1, 2)  A atom-1 atom-2 | B
```

因此：

- atom 可以相邻；
- 每个 atom 恰好占一个 caret step；
- UTF-8 byte coordinate 保持原义；
- soft-wrap / BiDi 视觉歧义继续只由 `CursorAffinity` 处理；
- text storage 与 platform UTF-16 adapter 保持原边界。

纯文本 inline node 上 N=0，因此 `InlinePoint(text_offset, 0)` 与 P0-P3 caret 一一对应。

P4.1 的兼容 seam：

```text
TextPoint
↕ exact conversion while atom_index == 0
InlinePoint

Core
  InlinePoint::validate
  StepMap::map_inline_point
  ChangeMap::map_inline_point

Runtime
  DocumentPosition::from_inline_point
  DocumentPosition::as_inline_point
  DocumentSelection::from_inline_points

GPUI
  DocumentView::inline_focus_point
  DocumentView::inline_selection_points
```

P4.1 在 canonical placement 尚不存在时对 `atom_index != 0` fail closed。

## 4. Canonical atom representation

P4.2 使用 stable `NodeId` 作为 atom identity，不建立第二套 AtomId allocator。

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

`fallback_text` 是 Core 级通用语义，用于 plain-text clipboard、accessibility fallback 与 unknown renderer fallback，不放进魔法 attr key。

atom 没有 editable child；P4 初版不定义 markable atom。

## 5. InlineContent placement

`InlineContent` 的逻辑模型扩展为：

```text
normalized text runs
+ ordered inline atom references
```

每个 atom reference 包含：

```text
atom NodeId
text_offset anchor
```

同一 `text_offset` 可有多个 atom，vector order 即 canonical order。公开 contract 只保证 deterministic ordered inline iteration，不承诺长期物理 representation。

文档验证把 inline atom reference 当成真实 tree edge：

```text
reference target exists
kind/content shape is InlineAtom
atom has exactly one parent
atom cannot appear in structural Children
atom cannot be document root
unreachable atom is invalid
placement text_offset is a valid UTF-8 boundary
```

`XiaomuDocument::parent_of`、full-tree validation、NodeStoreBuilder validation 都必须覆盖这类 edge。

## 6. Transaction / mapping direction

旧 `ReplaceText` 不能静默穿过 atom，也不能在 atom seam 丢掉 ordinal。

```text
A [atom] B

(1, 0) + X  → A X [atom] B
(1, 1) + X  → A [atom] X B
```

两处 caret 的 `TextOffset` 都是 1，因此裸 `ReplaceText(TextRange::empty(1))` 无法表达全部 mixed-inline mutation。

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

atom 相关 inverse 必须精确恢复：

```text
NodeId
AtomKind
NodeAttrs payload
fallback_text
placement
selection
```

Split / Join 遇到 atom 时也必须有明确 placement migration；在规则未证明前允许 fail closed，不能 ad-hoc 修补。

## 7. Runtime editing semantics

P4.3 负责：

```text
Left / Right 跨 atom 一次一个 unit
Backspace / Delete 在 atom 邻接处原子删除
selection 可跨 text + atom
atom seam text input 使用 atom-aware replacement
copy/cut 产生 detached atom fragment
plain fallback 使用 fallback_text
paste 恢复 identity-independent atom fragment
undo / redo 恢复 selection
IME composition 永远不能进入 atom 内部
```

clipboard fragment 不复制 live `NodeId`，粘贴时重新分配 canonical identity，保持 P3 structured clipboard 的 detached-value 原则。

## 8. GPUI / extension seam

P4.4 增加 host-neutral registry：

```text
InlineAtomRendererRegistry
AtomKind -> renderer
```

renderer 只拿 canonical atom projection 与 frontend context，不持有宿主数据库对象。

宿主动作：

```text
atom action
→ stable kind / payload / action key
→ HostCapability
→ host handles business behavior
```

GPUI renderer 缺失时必须 deterministic fallback，至少显示 `fallback_text`；accessibility projection 使用同一 fallback 语义。

P4B 后续可复用同一 capability 原则扩展 `BlockRendererRegistry` 与 `AssetService`。

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

Gate：无 atom 文档零语义回归；`TextOffset` 仍严格是 UTF-8 byte coordinate；canonical placement 建立前非零 ordinal fail closed。

实现 head `a2edcb35e4634b85294725ea1d19278276e754ae` 的 CI #295 已完整 success。P3 closeout 后 P4.1 已重放到新的 `main` 并完成 root docs 同步；后续 current-head CI 作为 merge Gate。

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

Gate：相邻两个 atom 可稳定构造、validate、insert/remove、undo/redo；无 sentinel text；atom seam mutation 不丢 `atom_index`。

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

### P4.5 Inline Atom Integration Gate

交付：

```text
realistic extension fixture
multi-editor isolation
Unicode + atom matrix
inline-atom Windows real-machine gate
P4A docs sync
CI Success
```

P4.5 只关闭 **P4A**，不会关闭整个 P4。通过后继续 P4B Atomic Block / Media。

## 10. P4A Scope boundary

P4A 不做：

```text
Atomic Block / Image / AssetService（转入 P4B）
Table
collaboration protocol
product-specific mention lookup
rich editable content inside atom
markable atom
virtualization redesign
GPUI upgrade
```

如果 atom implementation 迫使 `TextOffset` 改成混合单位、要求宿主对象进入 Core，或只能靠 frontend ad-hoc 修 placement，应停止实现并重新评审 coordinate / extension seam。
