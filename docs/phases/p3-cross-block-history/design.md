# P3 Visual Lines / Cross-block Clipboard / History 设计

状态：进行中

本文档是 P3 的可执行设计。顶层方向以 `docs/planning.md` 为准；P2 收官后的查缺补漏见 `docs/roadmap-gap-review.md`；已经真实成立的架构事实记录在 `docs/architecture.md`。

P2 已经把晓木从单块输入升级为完整 document tree 编辑：multi-block、DocumentSelection、结构 transaction、list、跨块键鼠选择、position mapping 与 minimal host-contract harness 均已闭环。P3 的任务是把当前“每个 block 只有一条视觉行”的过渡模型升级为真实文本布局，并补齐跨块编辑、structured clipboard 与可用的本地 history 语义。

P3 Gate：长段落发生 soft-wrap 后，Unicode/CJK/emoji 场景中的视觉导航、选择、跨块 copy/cut/delete 与 undo/redo 均保持正确；连续输入与 IME commit 形成可预测的 history group；宿主无需产品专用类型即可完成 persistence/change/focus 的 realistic integration fixture。

## 1. 范围

P3 横跨 `xiaomu-runtime`、`xiaomu-gpui`，必要时以最小 Core 增量支持明确的 canonical multiline 语义。P3 不改变 P0/P2 已建立的 canonical position：逻辑位置仍是 `TextPoint(NodeId + TextOffset + CursorAffinity)`，视觉行只是 frontend projection。

P3 必须交付：

```text
GPUI：soft-wrap / visual-line geometry
GPUI：x-preserving Up / Down，visual Home / End
GPUI：wrapped-line selection rectangles / hit-test / scroll-to-caret
Runtime：cross-block delete / copy / cut
Runtime：structured clipboard model，与平台 clipboard adapter 分离
Runtime：typing history grouping / coalescing
Runtime：IME composition 与 history boundary 的明确交互
Runtime：collapsed caret stored/pending marks
P3 内明确 HardBreak / CodeBlock multi-line canonical contract
P3：accessibility projection seam（text / semantic role / selection / focus）
P3：persistence/change/focus realistic integration fixture，含 multi-editor focus isolation
回归：Unicode cross-block + history/mapping invariants
```

## 2. 非目标

P3 不实现：

```text
Image / HorizontalRule 的完整 atomic editing loop（P4）
InlineAtom / renderer registry（P4）
AssetService / LinkOpenService 产品能力闭环（P4）
Markdown production baseline codec（P4.5）
Table（P5）
完整 virtualization/windowing（P6）
协作 OT / CRDT
floating image / page layout
复杂 spellcheck / comments 产品系统
```

P3 可以为后续能力建立 seam，但不能为对称性提前加入无真实调用方的抽象。

## 3. P2 移交状态

P3 从以下已验证事实开始：

```text
Core coordinates：UTF-8 scalar-safe TextOffset
Runtime selection：DocumentSelection 可跨 inline block
GPUI：DocumentView 已支持跨块 keyboard/mouse selection
GPUI：每个 inline block 当前只 shape 为一条 ShapedLine
Up/Down：当前是单视觉行块间移动，不保持 desired x
clipboard：已有 frontend-neutral TextClipboard seam，但 document-level structured selection 语义未完成
history：一笔 session transaction 一个 entry，typing 尚未 coalesce
IME：composition transient，commit 是单笔 InsertText
persistence：DocumentPersistence seam 已完成，harness fixture fail-closed
```

P3 不重新发明这些能力，只在其上增加真实视觉几何与跨块语义。

## 4. 关键设计决策

### 4.1 Visual line 是 frontend projection，不是 canonical state

soft-wrap 由字体、宽度、DPI、theme 等 frontend 条件决定，不能写入 `XiaomuDocument`。

```text
canonical inline text
      + width / typography
      ↓
VisualLineLayout
      ↓
line ranges / caret geometry / hit-test / selection rects
```

逻辑 `TextOffset` 不因 wrap 改变。相同逻辑位置在换行边界可能有两个视觉 caret 解释，使用已经存在的 `CursorAffinity` 表达，不引入 source-offset 或裸 pixel coordinate 到 Core。

### 4.2 Layout cache 从“单 ShapedLine”升级为 block layout

P2 的 `last_layout: Option<ShapedLine>` 只适用于单视觉行。P3.1 把它替换为窄定义的 block text layout cache，至少持有：

```text
visual lines
每行 logical byte range
每行 baseline / bounds
line-local x hit-test
logical offset → visual caret point
visual point → logical offset
```

cache key 延续 P2 原则，并补足真实布局输入：

```text
node / editing epoch
content width
font / font-size / line-height / typography revision
render-extension revision（进入 extension 阶段后使用）
```

不得把所有 blocks 永远 mounted 写入公开 API。

### 4.3 Vertical navigation 持有 transient desired_x

连续 Up / Down 需要保持视觉列：

```text
首次 Up/Down：从当前 caret 几何得到 desired_x
连续 Up/Down：沿用 desired_x
Left/Right/Home/End、pointer placement、文本编辑：重置 desired_x
```

`desired_x` 属 GPUI view/session projection state，不进入 canonical document、transaction 或 codec。

跨 block Up/Down 使用目标 block 的真实视觉首/末行几何，不能再直接复制 UTF-8 byte offset。

### 4.4 Selection paint 必须从一个 quad 升级为多 visual rect

wrapped selection 可能覆盖同一 block 多行，也可能跨多个 block。P3 的 selection projection 仍从 `DocumentSelection` 派生，但 paint 结果是 visual rect list：

```text
DocumentSelection
  ↓ ordered logical ranges
per-block TextLayout
  ↓
Vec<SelectionRect>
```

任何 rect 都是 transient frontend data，不回写 Core。

### 4.5 Cross-block mutation 只能由 Runtime 编译成 typed transaction

P2 已能跨块选择，但内容编辑仍主要要求 single inline node。P3 扩展 Runtime 语义：

```text
cross-block Delete
cross-block Backspace/Delete over selection
Cut = structured copy + one atomic delete
Paste = clipboard slice → typed transaction
```

frontend 不允许逐 block 私自删除后再拼结果。一次用户命令必须得到一笔可 undo 的 session history change；结构边界、保留首尾 block、移除中间节点等规则由 Runtime 统一决定。

### 4.6 Structured clipboard 与系统 clipboard 分层

系统剪贴板只是 transport。Runtime 引入 frontend-neutral structured payload，例如概念上的：

```text
ClipboardSlice
  plain_text fallback
  structured fragment / nodes / marks
```

GPUI adapter 负责与平台 clipboard 交换 MIME/format；Runtime/Core 不依赖 GPUI platform type。

最低要求：

```text
晓木 → 晓木：结构、marks、block boundary 可保留
晓木 → 外部：始终提供 plain-text fallback
外部 → 晓木：只有 plain text 时按明确规则导入
```

P3 不要求兼容第三方 editor 私有 JSON。

### 4.7 History grouping 属 Runtime，不改变 Core inverse contract

Core 继续只保证 transaction application / inverse。Runtime History 增加 group/coalescing 规则：

```text
连续相邻 typing          → 可合并同一 group
caret/selection move     → 结束 typing group
paste / cut / structural → 独立 group
mark command             → 明确 boundary
IME composition updates  → 不写 history
IME commit               → 恰好一次 history commit
undo / redo              → 不与新 typing 合并
```

是否合并必须由显式 group metadata / Runtime policy 决定，禁止根据时间戳偷偷猜测 canonical semantics。时间阈值若存在，只能作为“允许合并”的附加条件。

### 4.8 StoredMarks 是 Runtime transient editing state

collapsed caret 下的 Bold/Italic 等 toggle 在 P3 不再永远 NoChange。引入 frontend-neutral pending/stored marks：

```text
collapsed caret + ToggleMark
→ 更新 StoredMarks
→ document revision 不变
→ 后续 InsertText / IME commit 使用 StoredMarks
```

StoredMarks 不通过空 `TextRun` 伪造 canonical state，也不进入 codec。

P3 实现前必须用测试锚定这些清除/继承规则：

```text
显式 caret move 到其他位置 → 清除或按目标上下文重新解析
non-collapsed selection     → mark command 直接修改 canonical marks
split block                  → 继承策略明确
IME commit                   → 与普通 typing 使用同一 pending mark 规则
undo/redo                    → restored selection 后 pending marks 不凭空泄漏
```

### 4.9 HardBreak / CodeBlock 先定 canonical contract，再实现

P2 把 paragraph paste 中的换行折叠为空格，`CodeBlock` 也还沿用 paragraph 式单行输入。P3 必须区分：

```text
soft wrap   → 纯视觉，不改 canonical
HardBreak   → 用户显式 Shift+Enter 的文档语义
CodeBlock newline → code block 内真实多行内容
Enter       → 普通 block 的结构 split
```

P3.5 在动 Core 前先写 ADR，比较至少两种方案：

```text
A. built-in inline break piece
B. 受 node-kind 约束的 newline scalar in text content
```

选择必须满足 position/mapping/inverse/codec 可组合，并避免为了 HardBreak 提前把整个 P4 InlineAtom extension system 倒灌进 P3。若无法在这些约束下得到干净方案，允许把 HardBreak implementation 移交 P4，但 P3 必须完成视觉行架构与 CodeBlock 方案决策，不允许用 UI-only 换行冒充 canonical 语义。

### 4.10 Accessibility 从原则变为 projection seam

P3 至少提供可以从 frontend 读取的语义投影：

```text
visible/editable text
semantic node role/kind
current selection
focus owner
```

若 GPUI 当前缺少完整平台 accessibility API，记录 limitation；平台对象不得进入 Core/Runtime contract。

## 5. P3 实施切片

### P3.0 Phase Contract

交付：

```text
P3 design / progress
P2 → P3 handoff 固化
roadmap-gap-review 中 P3 结论并入阶段契约
source-size / dependency-boundary baseline
```

Gate：文档边界明确，CI 全绿。

### P3.1 Visual-line Geometry / Soft-wrap

交付：

```text
block TextLayout abstraction
soft-wrap text shaping
visual line logical ranges
logical offset ↔ visual caret geometry
point ↔ logical offset hit-test
layout cache 从 single ShapedLine 升级
Unicode/CJK/emoji/combining/BiDi fixture 基础
```

Gate：窄宽度 paragraph 可以稳定形成多视觉行；所有 line boundary 最终映射到合法 `TextOffset`。

### P3.2 Visual Navigation / Selection

交付：

```text
desired_x state
x-preserving Up / Down，含跨 block
visual Home / End
wrapped selection multi-rect paint
wrapped mouse drag / click hit-test
scroll-to-caret
```

Gate：Windows 实机长段落 + 中英/emoji 的键盘/鼠标导航与选择正确。

### P3.3 Cross-block Editing / Structured Clipboard

交付：

```text
cross-block delete
cross-block copy / cut
ClipboardSlice / structured payload
plain-text fallback
晓木内部 structured paste
single history entry / atomic failure
mapping + selection fallback regression
```

Gate：跨 paragraph/heading/list 的 selection 可 copy/cut/delete/undo，文档始终 validate。

### P3.4 History Grouping / Stored Marks / IME

交付：

```text
typing coalescing
explicit history boundaries
IME composition-history interaction
collapsed StoredMarks
undo/redo selection + pending mark regression
```

Gate：连续 typing 一次 undo；IME 一次 commit 一次 undo；移动/结构命令正确断组；collapsed mark toggle 对后续输入生效。

### P3.5 HardBreak / CodeBlock Multi-line

交付：

```text
HardBreak canonical ADR
Shift+Enter 语义（若本阶段实施方案通过 Gate）
CodeBlock Enter / paste newline / Tab 策略
multi-line layout 与 selection 回归
```

Gate：不能用 soft-wrap 或 UI-only state 冒充文档换行；任何 canonical 变更都有 mapping/inverse 测试。

### P3.6 Accessibility / Realistic Host Integration

交付：

```text
accessibility projection seam
persistence + change + focus fixture
restore selection/focus
multiple editors coexist / focus isolation
host-neutral theme/typography injection 的最小验证（若现有 seam 足够）
```

Gate：两个 editor 同时存在时输入、selection、focus、save/listener 不串状态；无产品专用类型进入 Runtime/Core。

### P3.7 Closeout

交付：

```text
Unicode cross-block regression matrix
history/mapping randomized invariants
Windows 最终实机 Gate
architecture / planning / progress 同步
source-size / dependency / fmt / clippy / tests / CI
```

## 6. 测试策略

自动测试优先放在最靠近语义所有者的位置：

```text
Core：仅 P3 新增 canonical step/representation 的 mapping + inverse + invariant
Runtime：cross-block edit / clipboard slice / history grouping / StoredMarks
GPUI pure logic：visual line range、导航、hit-test、desired_x reset
GPUI harness：soft-wrap paint、selection rect、IME、focus
Host fixture：load/change/save/focus/multi-editor
```

固定 Unicode 矩阵：

```text
ASCII
中文
中英混排
emoji / surrogate pair
combining mark
CJK + emoji cross-block
BiDi samples
```

任何视觉算法最终产出的 logical byte position 都必须经 Core boundary API 合法化，不能把 raw `usize` 直接跨层传递。

## 7. P3 完成定义

以下全部成立才关闭 P3：

- soft-wrap 成为 GPUI 正式布局路径，单 block 不再等价于单视觉行；
- logical position 与 visual line 完全分层，Core contract 不含 pixel/GPUI 类型；
- Up/Down 保持 desired x，Home/End 具有明确 visual-line 语义；
- wrapped selection / hit-test / scroll-to-caret 实机可用；
- cross-block copy/cut/delete 与 structured clipboard 可 undo 且保持文档合法；
- typing history grouping、IME history boundary 可预测；
- collapsed StoredMarks 不污染 canonical state；
- HardBreak / CodeBlock multiline 至少完成长期 canonical 决策，已实施部分具备 mapping/inverse 回归；
- accessibility projection seam 与 realistic host focus fixture 成立；
- Unicode cross-block + undo/redo invariant 全绿；
- P0/P1/P2 regression 全绿；
- source-size / dependency / fmt / clippy / tests / CI Success 全绿。

如果 visual-line geometry 无法稳定支撑 P3 交互，停止扩 structured clipboard/history，不把视觉债继续推到 P6。