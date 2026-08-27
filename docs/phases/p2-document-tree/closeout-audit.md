# P2 收官审计

状态：**P2.7 收官输入**

本文档记录 P2.0–P2.6 合入后，对照 `design.md`、`progress.md`、当前 `main` 实现进行的收官审计。它只记录需要在 P2 关闭前处理或明确移交的事项；长期路线仍以 `docs/planning.md` 为准。

## 1. 当前阶段判断

P2 已经完成主要实现切片：

```text
P2.0  Phase contract                 已完成
P2.1  SplitNode / JoinNodes          已完成
P2.2  DocumentSelection              已完成
P2.3  Structural commands            已完成
P2.4  List editing                   已完成
P2.5  GPUI multi-block               已完成实现，最终实机 Gate 待收口
P2.6  Minimal host-contract harness  已完成实现，持久化契约需补强
P2.7  Mapping / invariants / Gate     待完成
```

因此 P2 已经过了“做一半”的阶段。剩余工作量主要集中在收官正确性、真实宿主契约和实机验证，不应因为功能面已经可见而提前关闭 Phase Gate。

## 2. P2 关闭前必须修复

### 2.1 跨块 Up / Down 必须始终产出合法 Unicode 坐标

当前 `xiaomu-gpui::document_view::navigation::step_vertical` 把当前块的 UTF-8 byte offset 直接 `min(target.len())` 后带到相邻块。

这在中英混排下不成立。例如：

```text
source = "one"      offset = 2
 target = "二👍三"   raw candidate = 2
```

`2` 落在汉字 `二` 的 UTF-8 编码内部，不是合法 scalar boundary。随后 `validated_offset()` 会拒绝该坐标，使 Up / Down 静默失效。

P2.7 要求：

```text
vertical navigation target
    ↓
clamp / resolve to a valid scalar boundary
    ↓
TextOffset validation succeeds
```

至少增加：

- ASCII → CJK / emoji 的向上、向下回归测试；
- candidate 落在多字节字符内部时的确定性 boundary policy；
- 测试必须断言最终 `TextOffset` 合法，而不是只断言 raw `usize`。

P3 的 soft-wrap / x-preserving visual-line navigation 会替换这套简化算法，但 P2 的单视觉行模型本身也不能产生非法 Core 坐标。

### 2.2 `DocumentPersistence::load` 必须区分“没有文档”和“加载失败”

当前接口：

```rust
fn load(&self) -> Option<XiaomuDocument>;
```

无法区分：

```text
store 不存在       → 合法空状态
I/O 失败            → 错误
格式损坏 / parse 失败 → 错误
```

当前 harness 的 `FixtureStore` 又用 `.ok()?` 吞掉 I/O / parse 错误，损坏文件会被当成“没有文档”，然后回退到 demo fixture。这对真实宿主契约不可接受。

P2.7 应调整为类似：

```rust
fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError>;
```

并锚定：

- NotFound → `Ok(None)`；
- read / parse error → `Err(PersistenceError)`；
- harness 不允许在损坏持久化数据时静默启动一份新文档。

### 2.3 P2.6 fixture round-trip 不能丢失 P1 已经支持的 marks

`DocumentPersistence` 的契约是保存 canonical snapshot；但当前 P2.6 fixture 明确只拼接 inline text，load 时用 `MarkSet::empty()` 重建，Bold / Italic / Code / Underline / Strike 会在 save → reload 后丢失。

这会让 P1 已完成的 canonical 语义在 P2 host-contract Gate 中发生数据损失。

P2.7 要求至少让 fixture 保存当前 P1/P2 可编辑语义：

```text
node kind / tree shape
inline run boundaries needed for mark reconstruction
MarkSet（含 Link 属性的 preservation，即使当前无 Link UI）
当前阶段实际使用的 NodeAttrs
```

fixture 仍然可以是 harness-private 格式，不需要升级成公共 codec，但 round-trip 断言必须从“kind + 拼接文本”提高到“当前阶段 canonical semantics 等价”。

## 3. P2.7 必须完成的原计划事项

### 3.1 Mapping regression matrix

P0 / P2.1 已经有单 step mapping 与随机 inverse 基础；P2 关闭前还需要 session / structural composition 级矩阵：

```text
SplitNode → selection map
JoinNodes → selection map
RemoveNode / RestoreSubtree → Deleted / restored identity
list wrap / lift / indent / outdent staged plans
undo / redo across structural edits
cross-block anchor/focus direction preservation
```

重点不是增加测试数量，而是证明 P2 runtime 不在 ChangeMap 之外维护另一套隐式 offset 修补规则。

### 3.2 会话级随机结构编辑不变量

在合法 fixture 上生成结构命令序列，至少检查：

```text
after every committed command: document.validate() succeeds
after every committed command: selection validates against current snapshot
undo whole chain: initial canonical semantics / identities restored as contracted
redo whole chain: recorded selections remain valid
NoChange: revision / history / notification do not advance
```

### 3.3 Windows 实机最终 Gate

P2.5/P2.6 已经有多次实机反馈和修复，但最终收官清单仍需一次完整执行并记录证据：

```text
multi-block direct input
Microsoft Pinyin inside different blocks
Left / Right cross-block
Up / Down cross-block（含中英 / emoji）
Shift keyboard selection cross-block
mouse drag selection cross-block
Enter split / Backspace join
paragraph → list → indent / outdent → paragraph
undo / redo structural edits
Ctrl+S save
restart + load
listener observes committed changes
```

P2 不要求 cross-block copy / cut / delete；这些仍属于 P3。

## 4. 关闭前文档与代码卫生

### 4.1 进度文档应按真实状态收口

`progress.md` 底部 Gate 目前落后于实现：P2.5/P2.6 已经合入，P1 session 回归也在后续 PR 中持续通过。P2.7 最终 PR 应把“实现已完成”和“实机 Gate 已完成”区分清楚后同步状态。

### 4.2 `architecture.md` 顶部摘要需要同步 multi-block 事实

当前架构正文已经记录 DocumentView / multi-block，但顶部总体摘要仍主要描述单 Paragraph GPUI 闭环。P2 收官 PR 应统一为当前真实状态。

### 4.3 Source-size warning 在进入 P3 前清理

P2 已经出现多个高频修改文件接近 source-size review warning。P2.7 应重新运行 guard；仍处于 501–700 行 warning 且 P3 会继续增长的 hot module，优先按职责拆分。

目标是避免 P3 的 cross-block clipboard / history / visual-line layout 继续堆进同一个 `mod.rs` / `actions.rs`。

## 5. 明确移交 P3，不作为 P2 blocker

以下能力在 P2 中有意识停损，继续留在 P3：

```text
soft-wrap / visual-line layout
x-preserving Up / Down
跨视觉行 Home / End 语义
cross-block copy / cut / delete
structured clipboard
history grouping / typing coalescing
composition / history group interaction
persistence / focus realistic integration fixture
accessibility text / role / selection / focus projection seam
grapheme-cluster caret semantics
BiDi visual affinity resolution
```

其中 soft-wrap / visual-line navigation 应放在 P3 前部完成，因为 cross-block selection、鼠标拖选和长期 hit-test 都依赖真实视觉行几何；不建议拖到 P6 virtualization 阶段再返工。

## 6. P2 最终关闭标准（审计版）

P2.7 关闭时同时满足：

```text
原 design.md P2 Completion Definition 全部满足
+ vertical navigation 永远产出合法 Unicode coordinate
+ persistence load error 不被吞掉
+ P1/P2 canonical marks 经 host fixture save/load 不丢失
+ Windows 最终实机 Gate 有记录
+ source-size / dependency / fmt / clippy / tests / CI Success 全绿
+ architecture / progress 与真实实现一致
```

达到以上条件后进入 P3，不把已知 correctness / persistence contract 问题带入下一阶段。
