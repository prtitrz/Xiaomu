# P2 收官审计

状态：**收官验证中，功能 Gate 已通过，仅待本收官 PR 的最终 CI Success。**

本文档记录 P2 的最终收官判断。长期架构事实放在 `docs/architecture.md`，执行证据放在 `progress.md`，未来路线仍以 `docs/planning.md` 为准。

## 1. 最终阶段判断

P2 的主体实现和 Windows 实机 Gate 已完成：

```text
P2.0  Phase contract                 已完成
P2.1  SplitNode / JoinNodes          已完成
P2.2  DocumentSelection              已完成
P2.3  Structural commands            已完成
P2.4  List editing                   已完成
P2.5  GPUI multi-block               已完成并通过 Windows 实机验证
P2.6  Minimal host-contract harness  已完成
P2.7  Mapping / invariants / Gate     已完成
```

P2 已形成完整的 document-tree 编辑闭环：Core 提供结构 transaction / mapping / inverse，Runtime 提供 document-level selection 与结构命令编排，GPUI 提供 multi-block 视图与跨块交互，harness 证明宿主可 load、listen、save。

## 2. P2.7 收官问题处置

### 2.1 Unicode Up / Down 坐标

已修复。跨块 Up / Down 在单视觉行模型下先把候选 UTF-8 byte offset 钳制到目标文本范围，再向下解析到合法 Unicode scalar boundary，随后才构造 `TextOffset`。

因此中英混排与 emoji 场景不会再把 mid-scalar raw index 交给 Core validation 形成静默 NoChange。

P3 仍可升级为基于 shaped-line 几何的 x-preserving visual-line navigation；这不影响 P2 当前模型的坐标合法性。

### 2.2 List Enter / empty-item exit

已修复。`SplitBlock` 在 list item 内具有 list-aware 语义：

```text
非空 item Enter
→ split 当前 inline block
→ tail 进入新 sibling ListItem
→ caret 到新 item 的 tail block 起点
→ staged plan 作为一笔 history 提交

空折叠 item Enter
→ 嵌套 item 执行 outdent
→ 顶层 item 执行 lift out
→ selection 按结构命令 policy 收敛并重新校验
```

没有为此新增 Core list 专用 step，继续复用既有结构原语和 staged plan。

### 2.3 Bullet / Ordered marker

已修复。BulletList / OrderedList marker 由 GPUI frontend projection 生成，ordinal 确定性计算；marker 不写入 canonical text，不占 selection offset，也不改变 Core document semantics。

### 2.4 Persistence load 错误语义

已修复。Runtime seam 为：

```rust
fn load(&self) -> Result<Option<XiaomuDocument>, PersistenceError>;
```

契约明确：

```text
store 不存在          → Ok(None)
I/O 读取失败           → Err(PersistenceError)
格式损坏 / parse 失败  → Err(PersistenceError)
```

harness 遇到损坏 store 会拒绝以 demo fixture 覆盖启动，不再吞掉错误。

### 2.5 Fixture canonical fidelity

fixture v2 已保存当前 P1/P2 所需的 canonical 语义：

```text
node kind / tree shape
inline run boundaries
MarkSet
Link href / title
NodeAttrs
```

round-trip 断言按当前阶段 canonical semantics 等价校验，不比较分配顺序相关的 NodeId。

### 2.6 Unsupported node 必须 fail closed

P2 收官复核又发现一个 persistence 边角：fixture v2 尚未编码 `HorizontalRule`、`Image` 或 extension `Custom` node；原 `write_node()` 的 fallback 会让这些节点在 save 时被静默跳过。

本收官 PR 改为 fail closed：fixture 遇到当前格式不支持的 node kind 时直接返回 `PersistenceError`，不会产生一个“保存成功但丢节点”的快照。

新增回归测试覆盖：

```text
HorizontalRule → save Err，不允许静默丢失
Custom node    → save Err，不允许静默丢失
```

这保持 `DocumentPersistence::save` 的“保存整个 canonical snapshot”契约，同时避免为了关闭 P2 提前扩展 P4 的 atomic / extension codec 能力。

## 3. Mapping 与结构不变量

P2 已补齐 session / structural composition 级覆盖，重点验证：

```text
SplitNode / JoinNodes selection mapping
RemoveNode / RestoreSubtree identity 与 Deleted 语义
list wrap / lift / indent / outdent staged plans
list Enter / empty-item exit
undo / redo across structural edits
cross-block anchor / focus 方向保持
```

会话级结构测试持续检查：

```text
after committed command: document.validate() 成功
after committed command: selection 对当前 snapshot 合法
undo: contracted identity / canonical state 可还原
redo: recorded selection 仍合法
NoChange: revision / history / notification 不推进
```

P2 Runtime 没有在 `ChangeMap` 之外引入另一套隐式 offset 修补规则。

## 4. Windows 最终实机 Gate

PR #38 已记录 Windows 真机完整 Gate 完成。收官核对覆盖：

```text
multi-block direct input
Microsoft Pinyin in different blocks
Left / Right cross-block
Up / Down cross-block，含中英 / emoji
Shift keyboard selection cross-block
mouse drag selection cross-block
Enter split / Backspace join
paragraph → list
list Enter creates sibling item
empty list item Enter exits current list level
bullet / ordered marker 可见且不同
indent / outdent → paragraph
undo / redo structural edits
Ctrl+S save
restart + load
listener observes committed changes
```

P2 不要求 cross-block copy / cut / delete；这些按原计划进入 P3。

## 5. 明确移交 P3

以下能力有意留给 P3 或后续阶段，不构成 P2 blocker：

```text
soft-wrap / visual-line layout
x-preserving Up / Down
跨视觉行 Home / End
cross-block copy / cut / delete
structured clipboard
history grouping / typing coalescing
composition / history group interaction
更真实的 persistence / focus integration fixture
accessibility projection seam
grapheme-cluster caret semantics
BiDi visual affinity resolution
atomic / extension node 的产品级 codec 表达
```

其中 visual-line layout 应在 P3 前部处理，避免跨块选择与 hit-test 在单视觉行模型上继续扩张。

## 6. P2 最终关闭标准

收官 PR 合入时必须同时满足：

- [x] 原 P2 Completion Definition 的功能项全部完成
- [x] vertical navigation 不产生非法 Unicode coordinate
- [x] list Enter / empty-item exit 形成真实 list editing loop
- [x] bullet / ordered marker 可见且不污染 canonical text
- [x] persistence load 错误不被吞掉
- [x] fixture round-trip 保留当前 P1/P2 marks / attrs
- [x] unsupported atomic / custom node save 时 fail closed，不静默丢数据
- [x] Windows 最终实机 Gate 有记录
- [x] mapping / structural invariant 回归覆盖已补齐
- [x] source-size / dependency boundary guard 在 P2.7 检查通过；后续 hot module 拆分按 P3 真实增长驱动
- [x] architecture / progress / closeout 文档与实现同步
- [~] 本收官 PR 最终 `CI Success` 全绿

最后一项通过后，P2 状态改为 **CLOSED**，后续功能变更进入 P3，不继续扩张 P2 范围。
