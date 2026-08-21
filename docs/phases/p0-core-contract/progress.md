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

当前切片：**P0.2B Node Storage 与 Immutable Snapshot 已完成，等待 PR 最终 current-head CI 与合并**

当前分支：`feat/p0-document-snapshot`

P0.0、P0.1、P0.2A 已合并。P0.2B 的 canonical node tree、不可变 snapshot、full-tree validation 和 structural sharing 已完成实现并通过代码级完整 CI；本 PR 最后的中文架构/进度同步提交仍需通过 current-head CI 后合并。

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

- [ ] 实现 `CursorAffinity`
- [ ] 实现 `TextPoint`
- [ ] 实现 `TextSelection`
- [ ] 实现 `NodeSelection`
- [ ] 实现 structural boundary position（`NodeGap` 或最终等价类型）
- [ ] selection 针对 document snapshot 校验
- [ ] invalid / deleted node position 测试
- [ ] 中文 / emoji text position 测试

完成证据：

```text
待开始
```

## P0.4 Transaction Application

- [ ] 实现 typed `Transaction`
- [ ] 实现 transaction origin
- [ ] 增加不携带宿主专用类型的 metadata seam
- [ ] 实现 `ReplaceText`
- [ ] 实现 `InsertNode`
- [ ] 实现 `RemoveNode`
- [ ] 实现 `SetNodeAttrs`
- [ ] 实现 `AddMark`
- [ ] 实现 `RemoveMark`
- [ ] apply 返回新 snapshot
- [ ] apply 后重新验证 resulting document
- [ ] 确认没有公开 direct canonical mutation escape hatch

完成证据：

```text
待开始
```

## P0.5 Position Mapping

- [ ] 定义 P0 mapping result 语义
- [ ] text replacement mapping
- [ ] insertion mapping
- [ ] deletion mapping
- [ ] removed-node result
- [ ] transaction 内 mapping composition
- [ ] insertion / replacement / deletion mapping table
- [ ] 中文 / emoji mapping fixture
- [ ] removed-node mapping fixture

完成证据：

```text
待开始
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
- [ ] position / selection model 校验正确
- [ ] typed transaction 保持 document invariant
- [ ] StepMap / ChangeMap 可显式映射旧 position
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

## Regression Log

当前没有已知 regression。
