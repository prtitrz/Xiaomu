# ADR 0004: HardBreak 与 CodeBlock 使用 canonical LF

Status: Accepted
Date: 2026-08-29

## Context

P3.1 已将 GPUI 文本布局升级为 visual-line / soft-wrap 模型，但 canonical inline content 仍只有 `TextRun`。普通 block 的 `Enter` 目前执行结构 split，`Shift+Enter` 尚无文档语义；`CodeBlock` 也仍沿用 paragraph 式单行编辑，因此 Enter 会错误地拆成两个 block，plain-text paste 中的换行也会被折叠为空格。

P3 必须长期区分三类“换行”：

```text
soft wrap          纯视觉折行，不进入 canonical document
HardBreak          用户在普通富文本 block 中显式插入的文档内换行
CodeBlock newline  代码块正文中的真实多行文本
```

需要选择一种 canonical 表达，使 position、mapping、inverse、clipboard、persistence、codec 与 GPUI layout 可以复用现有基础，同时不能为了 HardBreak 提前把 P4 的完整 `InlineAtom` / extension system 倒灌进 P3。

## Decision

采用 UTF-8 LF scalar `\n`（U+000A）作为 inline content 内唯一**具有晓木 line-break 语义**的 canonical 表达。

它的语义由所在 inline-bearing node 的 kind 决定：

```text
Paragraph / Heading 等普通富文本 inline node
    LF → HardBreak

CodeBlock
    LF → 代码正文 newline

GPUI soft-wrap
    不产生 LF，不修改 canonical content
```

Core 当前仍允许普通 `TextRun` 承载其他合法 Unicode scalar，包括调用者直接构造的 CR。ADR 0004 不新增“全局禁止 CR”的 Core validation invariant；它规定的是晓木编辑命令、平台 adapter 与 codec 在表达 line break 时只生成/识别 LF。若未来需要把 CR 禁止升级为 schema invariant，应另行增加明确的 Core validation contract，而不能假设本 ADR 已经做到。

### 输入语义

普通富文本 block：

```text
Enter        → 结构 `SplitBlock`
Shift+Enter  → 插入一个 canonical LF（HardBreak）
```

CodeBlock：

```text
Enter        → 插入一个 canonical LF
Shift+Enter  → 插入一个 canonical LF
Tab          → 插入可见缩进，不触发 list conversion / list indent
```

插入 line break 是显式 editing command，形成独立 history boundary，不与前后普通 typing coalesce。它仍通过已有 `ReplaceText` transaction 实现，因此无需增加 Core 专用 transaction step。

### 坐标与编辑

LF 是一个 UTF-8 byte、一个 Unicode scalar，因此继续使用既有 `TextOffset`：

```text
... "a\nb" ...
     ^ ^
     | +-- LF 后 caret boundary
     +---- LF 前 caret boundary
```

Backspace / Delete 对 LF 与其他 scalar 一样工作，一次删除一个 LF。`ReplaceText` 的 mapping / inverse 规则原样适用；不建立第二套 line coordinate。

`TextRun` 可以包含 LF。mark 可以跨过 LF；LF 本身没有独立可见 glyph，frontend 对两侧文本继续按 run marks 渲染。未来若某种 inline atom 需要独立 selection / payload，它仍由 P4 的 `InlineAtom` 模型解决，不复用 HardBreak。

### Line ending normalization

晓木的 line-break 语义只生成 LF。平台输入 adapter 或 codec 在需要保留多行文本时执行：

```text
CRLF → LF
CR   → LF
LF   → LF
```

因此由晓木正常编辑路径产生的 HardBreak / CodeBlock newline 都是 LF；原始 Core construction 若显式放入 CR，只会被视为普通文本 scalar，不获得第二种 newline 语义。

当前 plain-text paste 策略按目标 node 区分：

```text
CodeBlock
→ 保留多行，只规范 line ending 为 LF

普通富文本 block
→ 继续把平台 plain-text line breaks 折叠为空格
```

后者只是当前 fallback paste policy，不代表 Paragraph 无法承载 HardBreak。`Shift+Enter` 已可以在 Paragraph / Heading 中产生 canonical LF；未来 codec 或 richer paste policy 可以显式生成 HardBreak。

structured clipboard 继续保存 canonical inline text，因此 LF 可原样 round-trip。

### Frontend layout

GPUI `BlockTextLayout` 继续负责 visual projection。`shape_text` 返回多个 logical `WrappedLine` 时，相邻 logical line 的 canonical coordinate 中间恰好跳过一个 LF byte；现有 layout 代码的 `line.len() + 1` 累计规则与本 ADR 对齐。

soft-wrap boundary 只由 shaping geometry 产生，没有 canonical byte；HardBreak / CodeBlock newline 则占用真实 LF byte。视觉 Home / End、Up / Down、hit-test、selection projection 必须保持这一区分。

## Alternatives considered

### A. built-in inline break piece / atom

例如把 `InlineContent` 从纯 `TextRun` 扩成 `TextRun | HardBreak`。

优点：HardBreak 在类型层非常显式，也不会和文本 newline 混淆。

拒绝原因：当前 Core position 是连续 UTF-8 byte coordinate，transaction / mapping / marks / clipboard / persistence 都围绕文本 range 成熟。单独 piece 会立即要求定义 atom 宽度、atom 前后 position、mark 边界、ReplaceText 与 atom 的组合规则，并与 P4 `InlineAtom` 扩展 seam 大量重叠。仅为 HardBreak 提前支付这套复杂度没有收益。

### B. LF scalar in inline text

优点：与现有 `TextOffset`、`ReplaceText`、mapping、inverse、structured clipboard、fixture persistence、GPUI multi-line geometry 天然组合；CodeBlock 本来就需要真实 newline；没有新增 Core step 或 extension dependency。

这是本 ADR 采用的方案。

### C. UI-only HardBreak

例如 Shift+Enter 只在 GPUI 保存一个 transient break。

拒绝。保存/重载、copy/paste、undo/redo、codec 后都会丢失，且会把 visual state 冒充 document semantics。

### D. CodeBlock 使用 LF，但 Paragraph HardBreak 延后为 InlineAtom

这会让相同的 line-oriented geometry 同时维护两套 canonical representation，并增加 clipboard / codec 转换分叉。除非未来出现 LF 无法满足的实际语义需求，否则不采用。

## Consequences

- P3.5 无需增加新的 Core node/content variant 或 transaction step。
- `TextOffset`、mapping 与 exact inverse contract 保持不变。
- Paragraph/Heading 从此允许 canonical inline text 包含 LF；调用方不能继续假设一个 block 等于一个 logical line。
- CodeBlock 可以在同一个 stable NodeId 内承载多行文本。
- frontend navigation / hit-test 必须正确跨 logical newline，不得把 LF 当 soft-wrap boundary。
- 平台/codec 若从 CR/CRLF 源文本导入 line break，必须先规范化为 LF；Core 原始 construction 的 CR 容忍度不属于 line-break contract。
- Markdown codec 后续可把 Paragraph LF 编码为 Markdown hard break，把 CodeBlock LF 原样编码到 fenced code body；codec source representation 仍不反向定义 Core。
- P4 InlineAtom 仍用于 mention/reference/chip 等真正 atomic inline object，不承担 HardBreak。

## Revisit when

仅在出现以下证据时重新评估：

- HardBreak 需要独立 payload、stable identity 或 atom-level selection；
- mark / annotation 模型证明 LF scalar 无法表达必要语义；
- BiDi / accessibility / platform IME 对 inline LF 暴露无法在 adapter 层解决的结构性问题；
- P4 InlineAtom 已成熟，并能在不破坏现有 text coordinate contract 的前提下显著简化 HardBreak。

在这些条件出现前，不为 HardBreak 引入第二套 inline coordinate system。
