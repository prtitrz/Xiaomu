# 历史归档：晓木路线查缺补漏审计（P2 收官前）

日期：2026-08-27

> **历史审计，不是当前执行路线。** 正文中的“当前”“尚未实现”和阶段建议均指 2026-08-27 的仓库状态。执行主线以 [planning](../planning.md) 为准，当前代码事实见 [architecture](../architecture.md)，文档入口见 [README](../README.md)。
>
> 2026-10-01 对照说明：P0–P5 已收口；本文保留原始判断以便追溯，不再逐段改写为当前状态。未完成项必须进入 planning / phase contract，不能只留在此页。

## 后续归属（2026-10-01 核对）

| 原审计项 | 后续归属 / 当前边界 |
| --- | --- |
| Soft-wrap、StoredMarks、CodeBlock、HardBreak | 已进入 P3 并完成，见 [P3 progress](../phases/p3-cross-block-history/progress.md) |
| Inline atom、atomic navigation、Image、HorizontalRule | 已进入 P4 并完成对应引擎契约，见 [P4 progress](../phases/p4-structured-content-extension/progress.md)；外部截图粘贴仍是 [未排期缺口](../planning.md#delivery-boundaries) |
| Markdown bootstrap 桩 | P4.9 已交付 baseline codec；支持范围与显式拒绝项见 [architecture](../architecture.md) |
| Accessibility | P3/P4 已建立 projection / fallback；平台接入限制仍存在，screen-reader smoke test 留在 P7 范围 |
| Renderer registries / link | Inline atom registry 已交付；通用 `BlockRendererRegistry` / `LinkOpenService` 已归属 P7；链接编辑 UI 另列 planning 待排期项 |
| 阶段顺序 | 当前正式路线及调整入口只在 [planning](../planning.md)；下文 §5 保留历史建议 |

以下为原审计正文。

本文档是对 P0/P1 已完成状态、P2 当前实现和 `docs/planning.md` 后续路线的一次查缺补漏。它不替代阶段 design；结论应在对应阶段启动或收官时并入顶层 planning / phase contract。

## 1. 总体结论

P0 / P1 的底层方向成立，P2 已经完成大部分主体实现。当前没有发现需要推翻 canonical document / transaction / mapping / session 分层的架构性问题。

真正需要补的是两类内容：

```text
A. P2 收官 correctness / persistence contract
B. 后续路线漏列的“成熟富文本编辑器基础能力”
```

P2 的具体 blocker 及后续处置见 [P2 closeout 审计归档](p2-closeout-audit.md)。

## 2. 已经覆盖得比较完整的能力

### P0 / Core

```text
immutable snapshot
stable opaque NodeId
typed transaction
explicit mapping + Deleted / bias
exact inverse / identity restore
Unicode scalar-safe coordinates
normalized runs / marks
full-tree validation
random invariant testing
```

这些能力已经形成后续阶段可以继续扩展的地基，不建议为了 UI 便利改成裸 offset、直接 mutable tree 或 frontend-owned canonical state。

### P1 / Native input

```text
DocumentSession orchestration
single-block native GPUI editing
Windows Microsoft Pinyin composition
UTF-16 adapter boundary
caret / local selection / hit-test
plain clipboard
basic mark toggle
undo / redo
```

IME 的 preedit 保持 frontend transient，commit 才进入 canonical transaction，这个边界应继续保持。

### P2 / Document tree

```text
SplitNode / JoinNodes
DocumentSelection
structural after-selection policy
list wrap / lift / indent / outdent
multi-block DocumentView
cross-block keyboard / mouse selection projection
minimal persistence + listener seam
```

P2 的主体已完成，当前主要是收官而非继续扩大结构编辑 scope。

## 3. 路线中已经确认的遗漏

### 3.1 Image / atomic block 没有明确实施阶段

顶层 model 已有：

```text
NodeKind::Image
NodeContent::Atomic
AssetService
BlockRendererRegistry
```

但原 roadmap P4 只列 InlineAtom / extension，没有图片从插入到展示的完整闭环。

处置：P4 调整为 **Atomic Node / Image / Extension Seam**，详见：

[P4 structured content 总览](../phases/p4-structured-content-extension/overview.md) 与 [atomic media 设计](../phases/p4-structured-content-extension/atomic-media.md)（原设计路径已调整）。

### 3.2 Soft-wrap / visual-line editing 没有明确阶段

P2 当前 deliberately 使用“一个 block = 一条视觉行”的简化模型；真实富文本段落必须支持：

```text
soft wrap
visual line measurement
x-preserving Up / Down
visual Home / End
selection rectangles across wrapped lines
hit-test across wrapped lines
scroll-to-caret
```

这不能一直拖到 P6 virtualization。建议作为 **P3 前置切片**：先把真实视觉行几何建立起来，再完成 cross-block clipboard / history grouping。

原因：P3 的拖选、跨块 selection、structured clipboard 都建立在稳定的 visual selection / hit-test 上；先做数据语义再返工整套布局会增加重复成本。

### 3.3 Markdown codec 有 crate 和 policy，但 roadmap 没有真正实现 slice

当前 `xiaomu-codec-markdown` 仍是 bootstrap marker。顶层架构明确 `XiaomuDocument ↔ Markdown`，但 P0–P7 没有一个清晰阶段要求 paragraph / heading / list / marks / image 的 round-trip baseline。

建议：

- P3 仍以 structured clipboard / editing correctness 为主，不强塞生产 codec；
- P4.5 随 Image / extension Gate 建立 **Markdown baseline codec**，至少覆盖当时全部 built-in 文档语义；
- P7 做 API / compatibility stabilization，而不是到 P7 才第一次实现 codec。

公共 codec 与 P2 harness-private persistence format 必须继续分开。

### 3.4 Link 有 Core mark，但缺少用户操作闭环

Core 已有 `Mark::Link(LinkMark)`，Host Contract 也预留 `LinkOpenService`，但目前只有 Bold / Italic / Code / Underline / Strike 的 UI path。

建议在 P4 capability-service 阶段补：

```text
set / edit / remove link attributes
link hit-test / activation
LinkOpenService callback
copy fallback
codec preservation
```

网络打开策略归宿主，Core 只保存 href/title。

### 3.5 collapsed caret 的“stored/pending marks”尚未规划

P1 当前 collapsed `ToggleMark` 是 NoChange。这对基础 Gate 足够，但成熟富文本编辑器通常允许：

```text
caret collapsed
Ctrl+B
继续输入
→ 新文本带 Bold
```

建议在 P3 history/selection runtime 里引入 frontend-neutral `StoredMarks` / pending mark state（名称待设计），并明确：

```text
selection move 是否清除
IME commit 如何继承
undo/history 是否记录
split block 如何继承
```

不要把 pending marks 写成空 TextRun 或伪造 canonical state。

### 3.6 Accessibility seam 需要从“原则”落到明确交付项

planning 已要求 P2/P3 开始暴露 accessibility seam，但当前阶段实现没有对应明确切片。

建议 P3 至少交付 frontend-neutral projection seam：

```text
text content
semantic role / node kind
selection
focus
```

P4 的 Image / InlineAtom 再补 alt / fallback semantics。P7 根据 GPUI 平台能力做 screen-reader smoke test。

### 3.7 CodeBlock 已有节点类型，但没有专门编辑语义

`NodeKind::CodeBlock` 已进入 Core，当前结构命令却仍把所有 inline-bearing block 按同一套 Paragraph 式语义处理。长期至少要明确：

```text
Enter        → code block 内换行还是 split block
Tab          → indentation / literal spaces
paste newline → 保留多行而不是折叠为空格
monospace / line-height / optional no-wrap
退出 code block 的键盘语义
```

这不应落到 P2 收官里临时补。建议放进 P3 visual-line / block-specific editing slice，与多行布局一起完成；否则 `CodeBlock` 会长期只有 schema、没有可用编辑体验。

### 3.8 HardBreak / 显式换行没有 canonical 语义

当前 Paragraph inline text 明确不接收换行，paste 会把 line break 折叠为空格；路线里又没有 `HardBreak` / Shift+Enter 的交付项。

成熟富文本至少需要区分：

```text
Enter        → 结构性 split block
Shift+Enter  → 当前 block 内显式 hard break
soft-wrap    → 仅视觉换行，不改 canonical document
```

建议 P3 在建立 visual-line contract 时一并确定 HardBreak 的 canonical 表达。不要把 hard break 和 soft-wrap 混成一个概念，也不要依赖裸 `\n` 是否“恰好能画出来”来定义文档语义。

### 3.9 HorizontalRule 已有 Atomic 节点，但缺少编辑闭环

`HorizontalRule` 与 `Image` 都属于 `NodeContent::Atomic`，目前 GPUI 只把所有 Atomic 内容画成通用占位线，导航序列又只枚举 inline-bearing block。

P4 的 atomic-node contract 应同时覆盖 HorizontalRule，至少证明：

```text
text ↔ HorizontalRule ↔ text 键盘导航
NodeSelection
Backspace / Delete
copy / cut / undo
```

Image 再在同一 atomic 基线上增加 AssetService 和异步渲染。这样 P4 得到的是通用 atomic block seam，而不是图片专用分支。

## 4. 目前不建议提前加入的能力

这些能力可以后置，不应打断 P2/P3/P4 主线：

```text
floating image / text wrap
image crop / free transform
multi-image gallery
collaborative OT / CRDT implementation
full spellcheck engine
comment system
block drag-and-drop product UI
complex page layout
real-time multiplayer presence UI
```

它们可以建立在已经规划的 extension / decoration / capability / collaboration-neutral seams 上。

## 5. 建议的后续阶段顺序

保持现有编号，避免路线频繁重排：

```text
P2.7  closeout correctness
      - Unicode navigation boundary
      - persistence load error + fidelity
      - mapping/session random invariants
      - final Windows Gate

P3    Visual Lines + Cross-block Selection / Clipboard / History
      - soft-wrap / visual-line geometry
      - HardBreak contract / Shift+Enter
      - CodeBlock multi-line editing semantics
      - x-preserving vertical navigation
      - cross-block copy/cut/delete
      - structured clipboard
      - stored/pending marks
      - typing/history grouping
      - composition/history interaction
      - accessibility projection seam
      - realistic persistence/change/focus fixture

P4    Atomic Node / Image / Extension Seam
      - generic atomic traversal + HorizontalRule
      - block Image
      - AssetService
      - atomic NodeSelection/navigation/delete/copy
      - InlineAtom
      - renderer registries
      - LinkOpenService + link editing seam
      - capability-service integration
      - Markdown baseline codec

P5    Table
P6    Performance / Long Document / Virtualization
P7    Library Stabilization
```

## 6. 阶段控制原则

后续每个 phase contract 都继续回答四个问题：

```text
1. canonical semantics 属于哪一层？
2. transient frontend state 属于哪一层？
3. mutation 是否全部收敛到 transaction / session？
4. host-specific resource / policy 是否通过 capability / adapter？
```

只要这四条继续守住，新增 Image、Table、extension 不需要推翻 P0/P1 的底层设计。
