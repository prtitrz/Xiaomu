# ADR 0005：Mixed Inline 位置保留 UTF-8 TextOffset，并增加 atom ordinal

状态：接受（P4.1）
日期：2026-09-01

## 背景

ADR 0001 已接受 `TextOffset = validated UTF-8 byte offset`，并由 P0-P3 的 selection、transaction、mapping、history、GPUI input/layout 大量验证。

P4 要增加 inline atom。atom 是不可进入内部的一单位 canonical content，允许与文本混排，也允许多个 atom 相邻。

单独的 `TextOffset` 无法区分同一文本 byte boundary 上多个相邻 atom 之间的 caret 位置。如果直接让 atom 占一个伪 byte，`TextOffset` 将不再表示 UTF-8 byte index；如果使用 U+FFFC 等 sentinel，又会把视图占位符写进 canonical text，并让 text/mark/IME 语义依赖特殊字符。

## 决策

ADR 0001 继续有效。

`TextOffset` 和 `TextRange` 保持纯文本坐标：

```text
TextOffset = concatenated canonical text 的 UTF-8 byte index
TextRange  = text-only half-open range
```

Mixed inline caret 使用新的 `InlinePoint`：

```text
InlinePoint {
    node_id,
    text_offset: TextOffset,
    atom_index: usize,
    affinity: CursorAffinity,
}
```

在一个 `text_offset` 上若有 N 个 canonical inline atom，`atom_index` 合法范围为 `0..=N`，表示 caret 前已有多少个同 offset atom。

纯文本节点 N=0，因此现有位置天然对应 `atom_index = 0`。

`CursorAffinity` 继续只处理同一 logical position 的视觉歧义，例如 soft-wrap / BiDi；不承担 canonical atom order。

## 为什么不用 hybrid offset

若定义一个“文本 byte + atom 各占 1”的混合整数：

- 值不再是 UTF-8 byte index；
- Rust string slicing 必须先做第二次 coordinate translation；
- UTF-16 adapter 和 existing mapping contract 都会变得含混；
- ADR 0001 的边界价值被破坏。

因此不采用。

## 为什么不用 sentinel character

U+FFFC 或私用字符方案会让 atom 的存在依赖 canonical text 中的特殊 scalar，并产生这些问题：

- literal sentinel 与 atom sentinel 需要额外区分；
- mark / ReplaceText / IME 可能误操作 sentinel；
- fallback text 与 canonical text 被强行耦合；
- codec 必须知道编辑器内部占位实现。

因此不采用。

## 为什么不用 CursorAffinity 表示 atom 两侧

一个 atom 两侧看似可以借 `Before / After` 表示，但多个相邻 atom 会产生超过两个 canonical gap，二值 affinity 无法唯一表达。

同时 affinity 已经承担视觉 projection 语义，不应再承载 canonical content ordering。

## Transaction consequence

mixed-inline coordinate 也意味着旧的 text-only mutation 不能覆盖全部 atom seam。

例如：

```text
A [atom] B
```

atom 锚定在 text offset 1。caret `(1, 0)` 与 `(1, 1)` 的文本 byte offset 相同，但在两个位置输入 `X` 的 canonical 顺序不同：

```text
(1, 0) + X  → A X [atom] B
(1, 1) + X  → A [atom] X B
```

因此 P4 不允许把这两种操作都降格成同一个 `ReplaceText(TextRange::empty(1))`。P4.2/P4.3 必须建立 atom-aware inline replacement contract，至少让 transaction 能看到 `InlinePoint` boundary；旧 `ReplaceText` 在含 atom 的歧义 seam 上必须 fail closed。

这条约束与坐标决策绑定：`atom_index` 不能只存在于 selection/view 层，最终 mutation/mapping 也必须消费它。

## 影响

正面：

- P0-P3 UTF-8 text contract 保持稳定；
- atom 可相邻并保持 one-caret-unit；
- text storage、IME UTF-16 conversion 与 atom order 解耦；
- mapping 可以独立处理 text delta 与 atom ordinal delta；
- 不需要 canonical sentinel。

成本：

- Runtime/frontend 的 document position 需要从 text-only point 逐步升级到 `InlinePoint`；
- mapping 增加 atom-aware path；
- mixed inline selection 不能继续无条件降格为 `TextRange`；
- text insertion/replacement 在 atom seam 需要 atom-aware transaction；
- clipboard/layout/hit-test 需要认识 atom placement。

## P4.1 已验证范围

P4.1 先建立 coordinate seam，不提前构造尚不存在的 canonical atom：

```text
Core InlinePoint + UTF-8 validation
TextPoint ↔ InlinePoint(atom_index=0) compatibility
StepMap / ChangeMap mixed-inline mapping seam
Runtime mixed-inline conversion / selection entry point
DocumentView mixed-inline selection/focus projection
non-zero atom_index fail closed until canonical placement exists
```

该实现 head `a2edcb35e4634b85294725ea1d19278276e754ae` 的 CI run #295 已通过 Ubuntu fmt/Clippy/workspace all-targets、Windows/macOS workspace all-targets、policy/source-size/dependency-boundary/cargo-deny/advisory 与 aggregate `CI Success`。

## 迁移策略

P4.2 加入 canonical atom representation、placement validation 与 atom transaction，同时定义 atom seam 上的 mutation/mapping 规则。Runtime 内部 position storage 只有在 canonical placement 能验证非零 ordinal 后才升级，避免出现“Runtime 能表达但 document 无法验证”的半状态。

P4.1 保留 `TextPoint` 与现有 P0-P3 navigation/editing path；后续逐步把 mixed-inline editing path 收敛到 `InlinePoint`，text-only API 继续作为兼容 seam。

## 重新评估条件

出现以下情况时重新评估：

- atom ordinal mapping 无法保持稳定、可组合的 transaction mapping；
- atom seam 的 text replacement 需要大量 ad-hoc placement 修补；
- layout/input 需要大量 ad-hoc translation 才能使用该位置模型；
- future inline content（例如 editable inline container）证明 `(text_offset, atom_index)` 无法覆盖通用需求。
