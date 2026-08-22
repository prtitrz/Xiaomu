# P0 Core Contract 进度

状态：进行中

本文档只记录 P0 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P0 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P0.5 Position Mapping 已完成实现并通过 fmt / clippy / test，等待 PR CI 后合并**

当前分支：`feat/p0-position-mapping`

P0.0、P0.1、P0.2、P0.3、P0.4 已合并。

## P0.0 Phase Contract 与模块骨架

- [x] 创建 `docs/phases/p0-core-contract/design.md`
- [x] 创建 `docs/phases/p0-core-contract/progress.md`
- [x] 建立 `document`、`text`、`selection`、`transaction`、`mapping`、`history`、`commands` Core 模块边界
- [x] 增加初始 Core `Error` / `Result`
- [x] 保持 `#![forbid(unsafe_code)]`
- [x] 审查 bootstrap API 的 public/private 可见性
- [x] 同步 `docs/architecture.md`
- [x] 运行 source-size 与 dependency-boundary guard
- [x] 运行 `cargo fmt --all -- --check`
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 运行 `cargo test --workspace --all-targets`

完成证据：

```text
PR #2 在 CI Success 全绿后合并。
P0 Core 模块骨架与阶段契约建立完成。
```

## P0.1 Text Boundary

- [x] 实现 `TextBuffer`
- [x] 实现 `TextOffset`
- [x] 实现 `TextRange`
- [x] 构造 / 使用时验证 UTF-8 char boundary
- [x] stale offset/range 使用时针对目标 buffer 重新校验
- [x] safe slicing API
- [x] immutable replacement API
- [x] ASCII fixture
- [x] 中文 fixture
- [x] 中英混排 fixture
- [x] emoji fixture
- [x] combining-mark fixture
- [x] BiDi fixture
- [x] invalid boundary typed error
- [x] out-of-bounds typed error
- [x] stale-coordinate typed error
- [x] 预期非法输入路径不 panic
- [x] 用 ADR 0001 固化 Core text coordinate 决策
- [x] 完整 `CI Success`

完成证据：

```text
PR #3 已合并。
Ubuntu / Windows / macOS / policy / CI Success 全绿。
```

## P0.2 Document Model

### P0.2A Document Value Layer

- [x] 实现 `DocumentVersion`
- [x] 实现 `DocumentRevision`
- [x] 实现 opaque `NodeId`
- [x] 实现受校验的 `HeadingLevel`
- [x] 实现 built-in/custom `NodeKind`
- [x] 实现 `MarkKind` / `Mark`
- [x] 实现 `LinkMark`
- [x] 实现 `MarkSet` canonical ordering
- [x] 相同重复 mark 自动规范化
- [x] 同 kind 冲突 mark 被拒绝
- [x] 实现 `TextRun`
- [x] 禁止持久化空 `TextRun`
- [x] 将 document model 按职责拆分到独立源码文件
- [x] 完整 `CI Success`

完成证据：

```text
PR #4 已合并。
Ubuntu / Windows / macOS / policy / CI Success 全绿。
```

### P0.2B Node Storage 与 Immutable Snapshot

- [x] 实现确定性 NodeId allocator
- [x] 保证失败构建不消耗 NodeId
- [x] 实现 `AttrValue` / `NodeAttrs`
- [x] 未知 attrs preservation-first 保存，key 顺序确定
- [x] 实现 `InlineContent`
- [x] 实现 `NodeContent`
- [x] 实现 immutable `Node`
- [x] 实现 `NodeStore`
- [x] `NodeStore` 使用 `Arc<Node>` 做 node-level structural sharing 原型
- [x] 实现 safe bottom-up `NodeStoreBuilder`
- [x] 实现 externally immutable `XiaomuDocument`
- [x] 实现 full-tree validation
- [x] 拒绝 unknown child ID
- [x] 拒绝重复 child reference
- [x] 拒绝非法 parent/child kind 关系
- [x] 拒绝非法 node/content shape
- [x] 拒绝非 Document root
- [x] 拒绝 multiple parents
- [x] 拒绝 cycle，并优先返回 `CyclicDocument`
- [x] 拒绝 unreachable node
- [x] 相邻且 `MarkSet` 相同的 `TextRun` 自动合并
- [x] 用 revision 测试证明未变化 node payload 可共享
- [x] 保持 `XiaomuDocument` 无公开 mutation escape hatch
- [x] 代码级完整 `CI Success`
- [x] `docs/architecture.md` 同步为真实实现并切换为中文

完成证据：

```text
PR #5 的实现 head 7d9b4f7 通过 CI #57：
Ubuntu / Windows / macOS 的 fmt / clippy / test 全绿；
policy 的 source-size / dependency-boundary / cargo-deny / advisory 全绿；
CI Success 汇总全绿。

最后的 architecture/progress 文档同步提交仍需通过 current-head CI 后才可合并。
```

## P0.3 Position 与 Selection

- [x] 实现 `CursorAffinity`
- [x] 实现 `TextPoint`
- [x] 实现 `TextSelection`
- [x] 实现 `NodeSelection`
- [x] 实现 structural boundary position（`NodeGap`）
- [x] selection 针对 document snapshot 校验
- [x] invalid / deleted node position 测试
- [x] 中文 / emoji text position 测试

实现说明：

```text
TextPoint 构造不校验；validate 针对具体 snapshot 校验节点存在、inline content shape、UTF-8 boundary。
InlineContent::validate_offset 承担拼接文本的 offset 校验。
TextSelection P0 限制在单个 inline node 内；anchor/focus 保留用户意图，ordered_range 返回逻辑排序半开 range。
NodeStoreBuilder::peek_next_id 提供确定性未分配 NodeId 供测试使用。
```

完成证据：

```text
分支 feat/p0-position-selection：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace 15 个 test target 全绿（含 tests/position_selection.rs 6 个集成测试）
```

## P0.4 Transaction Application

- [x] 实现 typed `Transaction`
- [x] 实现 transaction origin
- [x] 增加不携带宿主专用类型的 metadata seam
- [x] 实现 `ReplaceText`
- [x] 实现 `InsertNode`
- [x] 实现 `RemoveNode`
- [x] 实现 `SetNodeAttrs`
- [x] 实现 `AddMark`
- [x] 实现 `RemoveMark`
- [x] apply 返回新 snapshot
- [x] apply 后重新验证 resulting document
- [x] 确认没有公开 direct canonical mutation escape hatch

实现说明：

```text
Transaction = origin + metadata(BTreeMap<String, String>) + steps。
apply 原子性：任一 step 失败即整体失败，原 snapshot 不变，无部分状态逃逸。
step 应用在内部中间 store 上顺序进行；最终 snapshot 走 full-tree validation。
ReplaceText / AddMark / RemoveMark 由 piece-based inline 编辑实现，replacement 继承 range.start 所在 piece 的 marks；
AddMark 在 range 内替换同 kind 冲突 mark；结果经 InlineContent 规范化（相邻同 mark 合并、空 run 丢弃）。
InsertNode 从 document allocator 分配新 NodeId；RemoveNode 连同子树一起从 store 移除；root 不可移除。
NodeStore 的 replace/insert/remove 原语均为 pub(crate)，无公开 mutation escape hatch。
```

完成证据：

```text
分支 feat/p0-transaction-application：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace 16 个 test target 全绿（含 tests/transaction_application.rs 12 个集成测试）
```

## P0.5 Position Mapping

- [x] 定义 P0 mapping result 语义
- [x] text replacement mapping
- [x] insertion mapping
- [x] deletion mapping
- [x] removed-node result
- [x] transaction 内 mapping composition
- [x] insertion / replacement / deletion mapping table
- [x] 中文 / emoji mapping fixture
- [x] removed-node mapping fixture

实现说明：

```text
StepMap 记录单个 step 的映射数据（TextReplaced / NodeInserted / NodeRemoved），
坐标是 step 应用时中间状态的坐标，由 apply 引擎在应用 step 时直接产出。
ChangeMap 按 application order 组合 step maps，无公开构造入口；
Transaction::apply_with_changes 返回 AppliedTransaction（新 snapshot + ChangeMap），
Transaction::apply 保持只要新 snapshot 的旧入口。
映射结果 MappedPosition 显式区分 Mapped 与 Deleted：
目标位于被删子树内的 position / selection 一律 Deleted，不静默 clamp。
落在被替换 range 内部/起点的 offset 与恰好位于插入点的 NodeGap 由 MapBias（Start/End）显式解析。
删除 child 时指向被删 child 的 gap 在前一个兄弟处存活，仅其后 gap 平移 -1，无歧义。
映射是纯坐标算术，不校验 snapshot；映射结果需要针对目标 snapshot 重新校验。
TextSelection 映射采用向外 bias，覆盖被替换内容的 selection 仍覆盖 replacement；collapsed 保持 collapsed。
属性与 mark steps 不移动 position，不产生 step map 条目；NodeInserted 的 step map 携带新分配的 NodeId。
```

完成证据：

```text
分支 feat/p0-position-mapping：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace 17 个 test target 全绿（含 tests/position_mapping.rs 9 个集成测试、mapping 模块 5 个单元测试）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
```

## P0.6 Inverse 与随机不变量

- [ ] 定义 inverse / change-set prototype 边界
- [ ] invert text replacement
- [ ] invert node insertion / removal
- [ ] invert attrs changes
- [ ] invert mark changes
- [ ] semantic round-trip helper
- [ ] deterministic multi-step inverse tests
- [ ] 可行范围内增加 randomized valid transaction sequence tests
- [ ] 随机 sequence 保持 document validity
- [ ] 随机 sequence 不 panic

完成证据：

```text
待开始
```

## P0.7 Contract Stabilization

- [ ] 审查 public rustdoc 的语义契约
- [ ] 明确记录 offset unit
- [ ] 明确记录 mapping deletion behavior
- [ ] 更新 `docs/architecture.md` 与最终 P0 实现一致
- [ ] 固化需要长期保留的 ADR
- [ ] 复核 `docs/planning.md` 的 P0/P1 一致性
- [ ] 记录 P1 尚未解决的依赖
- [ ] 完整 `CI Success`
- [ ] 标记 P0 完成

完成证据：

```text
待开始
```

## P0 Phase Gate

P0 只有在以下条件全部满足后才能完成：

- [x] 版本化 structured document model 可用
- [x] snapshot 对外不可变
- [x] NodeId / NodeStore structural-sharing prototype 可工作
- [x] TextRun-local marks 在 inline content 内确定性规范化
- [x] TextOffset / text boundary 测试全绿
- [x] position / selection model 校验正确
- [x] typed transaction 保持 document invariant
- [x] StepMap / ChangeMap 可显式映射旧 position
- [ ] inverse prototype 可恢复语义原状态
- [x] Unicode / CJK / emoji text-boundary 测试全绿
- [ ] property / randomized invariant tests 全绿
- [ ] P0 最终 `CI Success` 全绿

## 决策记录

这里只记录影响 P0 执行的决定。长期且难逆转的架构理由应进入 ADR。

### 2026-08-22

- P0 使用 `docs/phases/p0-core-contract/design.md` 与 `progress.md` 作为专门阶段文档。
- 面向项目维护和决策的阶段文档统一使用中文；代码标识、API 名称和公开 Rust rustdoc 可以继续使用英文。
- Core text offset 采用 opaque `TextOffset` 包装的 validated UTF-8 byte offset；UTF-16 属于 frontend/platform adapter。长期理由见 `docs/adr/0001-core-text-coordinate.md`。
- `TextBuffer` P0 先使用 `String`；是否切 rope 由 benchmark 决定。
- Node storage 先使用标准库 ownership / `Arc` 做 structural sharing，不提前把公开契约绑定到 persistent collection crate。
- Core semantic module 是公开边界，error 实现模块保持私有，只 re-export `Error` / `Result`。
- `TextOffset` 没有 public raw integer constructor；普通调用者通过 `TextBuffer::offset_at` 获取坐标。
- offset 即使曾经合法，也不能永久视为合法；每次针对目标 buffer 使用时重新校验。
- P0 text safety 保证到 UTF-8 Unicode-scalar boundary；grapheme cursor 行为留给更高编辑层。
- Text replacement 返回新 `TextBuffer`，保持 immutable snapshot 方向。
- P0.2 拆成 value semantics（P0.2A）与 storage/snapshot（P0.2B），避免在值语义未稳定前冻结 storage contract。
- `NodeId` 可比较、hash，但普通外部调用方不能从 raw integer 构造；分配由 document/store 负责。
- `DocumentRevision` 是本地 snapshot metadata，不是 collaboration clock。
- `MarkSet` 使用确定性语义顺序；完全相同的重复项规范化，同 kind 冲突值拒绝。
- `XiaomuDocument` 的公开 API 只允许查询；P0.4 的 `Transaction::apply` 才会成为 canonical mutation 公开入口。
- `NodeStoreBuilder` 使用确定性 allocator，并保证失败 insert 不推进 allocator，避免测试和后续 transaction fixture 出现无意义 ID 漂移。
- Full-tree validator 对 cycle 给出明确 `CyclicDocument`，不让 cycle 被较次级的 multiple-parent 错误遮蔽。
- P0.2 不为 P0.4 提前保留 production dead-code mutation helper；当前 structural-sharing replacement helper 仅在测试配置下存在，P0.4 再按正式 Transaction contract 引入内部 mutation API。

### 2026-08-23（P0.3）

- TextPoint 构造不触碰文档；合法性始终通过针对具体 snapshot 的 `validate` 建立，与“offset 不能永久视为合法”的既有决策一致。
- `InlineContent` 增加跨 run 的 offset 校验；run 边界因 run 非空恒为合法坐标，run 内部按 UTF-8 scalar boundary 校验。
- P0 的 `TextSelection` 限制在单个 inline node 内；跨 block selection 留给后续 session 层，不提前进入 Core contract。
- `NodeStoreBuilder` 新增只读 `peek_next_id`，让测试获得确定性且保证不存在的 NodeId，同时继续不开放 raw NodeId 构造。

### 2026-08-23（P0.4）

- `XiaomuDocument` 内部持有 next-node-id allocator 计数器，由 store 最大 raw id 推导初始值；`InsertNode` 由此分配稳定 NodeId。
- apply 是原子的：step 顺序应用在内部中间 store 上，最终状态 full-tree validation 后才产出新 snapshot；任一 step 失败则整体失败。
- ReplaceText 的 replacement 继承 `range.start` 所在 run 的 marks，保证连续输入的确定性；AddMark 在 range 内替换同 kind 冲突 mark 而不是拒绝。
- RemoveNode 语义为移除节点及其整个子树，避免 unreachable node 违反 store 不变量；root 不可移除。
- metadata seam 采用 `BTreeMap<String, String>`，不引入宿主专用类型。
- 每次 apply（包括空 transaction）推进 `DocumentRevision`。

### 2026-08-22（P0.5）

- Mapping 数据只由 transaction application 产出（`apply_with_changes` → `AppliedTransaction`），`ChangeMap` 无公开构造入口，其他子系统不允许自行修补 offset。
- 映射结果 `MappedPosition` 显式区分 `Mapped` / `Deleted`；目标节点位于被删子树内时返回 `Deleted`，默认行为不静默 clamp。
- 落在被替换 range 内部/起点的 offset、恰好位于插入点的 `NodeGap` 属于歧义位置，由调用方显式传入 `MapBias`（Start/End）解析，而不是隐式选择。
- 删除 child 对 gap 无歧义：指向被删 child 的 gap 在前一个兄弟处存活，仅其后的 gap 平移 -1。
- `ChangeMap` 组合按 application order 折叠，`Deleted` 短路；step map 记录的是 step 应用时中间状态的坐标，因此组合天然正确。
- 映射是纯坐标算术，不校验 snapshot；映射结果与任何 stale 坐标一样需要针对目标 snapshot 重新校验。
- `TextSelection` 映射采用向外 bias（两端向 replacement 外侧解析），collapsed selection 用 Start bias 保持 collapsed。
- 属性与 mark steps 不移动 position，不产生 step map 条目；`NodeInserted` 的 step map 携带新分配的 `NodeId`，供上层定位插入结果。

### 2026-08-23（P0.5 评审）

- 评审发现并修复：`TextReplaced` 空 range（纯文本插入）时，插入点上的 offset 恒被平移到插入文本之后，`MapBias` 失效；修复后空 range 插入点按 bias 解析（Start→原地，End→越过插入文本），补回归测试（PR #9）。
- mapping 的 bias / deletion policy 属于难逆转的长期语义决策，固化为 `docs/adr/0002-position-mapping-policy.md`。
- 历史评审结论存档：跨节点 TextSelection 建议被拒绝（跨 block selection 归 session/editor 层，见 planning.md §5.2）；apply 返回类型已按评审建议落地为 `AppliedTransaction`。

## Regression Log

- 2026-08-23：P0.5 空 range 插入点 bias 失效，PR #9 修复并附回归测试；无遗留影响。
