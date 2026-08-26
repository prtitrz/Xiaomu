# P2 Document Tree / Structural Edit 进度

状态：进行中

本文档只记录 P2 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P2 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P2.4 List 编辑已完成（待实机验证，GPUI 接入属 P2.5）**

前置状态：P0 已完成（PR #13）；P1 已全部完成并关闭（PR #14–#20）；P2.0 / P2.1 / P2.2 / P2.3 已合入（PR #21–#24）。

## P2.0 Phase Contract 与阶段骨架

- [x] 创建 `docs/phases/p2-document-tree/design.md`
- [x] 创建 `docs/phases/p2-document-tree/progress.md`
- [x] 记录 P1 移交的 P2 前置依赖归属（见下方决策记录）
- [x] 运行 source-size 与 dependency-boundary guard
- [x] 运行 `cargo fmt --all -- --check`
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 运行 `cargo test --workspace --all-targets`

完成证据：

```text
分支 docs/p2-phase-contract：
uv run python tools/check_source_size.py 全绿
uv run python tools/check_dependency_boundaries.py 全绿
cargo fmt / clippy -D warnings / cargo test 全绿
本 PR 的远端 CI Success 即 P2.0 Gate 证据。
```

## P2.1 Core 结构 steps

- [x] SplitNode step：构造校验（inline 节点、scalar boundary offset）+ application + 新兄弟分配
- [x] JoinNodes step：相邻 inline 兄弟合并，内容归一化拼接，被吸收子树移除
- [x] StepMap::NodeSplit / NodeJoined 映射数据（text point / node gap / node selection）
- [x] inverse 精确还原（SplitNode ↔ JoinNodes；JoinNodes 逆 = 删除追加文本 + RestoreSubtree）
- [x] 随机不变量测试扩展到新 step（valid 序列 + 整链 undo 还原初始 store）
- [x] mapping 单测迁出 production source（src/mapping.rs tests → tests/step_mapping.rs，source-size guard）

实现说明：

```text
SplitNode 只作用于 inline-bearing 节点；offset 经 InlineContent::validate_offset 校验。
run 内拆分时两半继承该 run 的 marks；恰好落在 run 边界则各 run 完整归属一侧；
任一半允许为空。tail 兄弟复用原节点的 kind 与 attrs，由 snapshot 内部 allocator 分配 id。
JoinNodes 要求 second 是 first 的紧邻后继兄弟；合并结果保留 first 的 identity/kind/attrs，
内容按 piece 顺序归一化拼接（跨 first/second 边界的同 marks run 会重新合并，
undo 时 JoinNodes 逆可精确还原该切分）。
映射语义见 architecture.md Transaction/Mapping 小节；split 点 offset 由 MapBias 解析。
```

完成证据：

```text
分支 feat/p2-core-structural-steps：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿（新增 split/join 应用与 round-trip 测试，
随机不变量生成器扩展 SplitNode/JoinNodes，mapping 单测迁至 tests/step_mapping.rs）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
```

## P2.2 Runtime DocumentSelection 与 session 升级

- [x] `DocumentSelection` / `DocumentPosition` 数据结构：端点为 TextPoint 或 NodeGap，validate / ordered / map_through / as_single_node 语义完整并有单测
- [x] 跨块排序：snapshot pre-order slot 分配（节点与 gap 各占单调槽位），ordered() 解析 head/tail
- [x] session selection 读写点切换到 document-level 校验；`text_selection()` 投影回单块 Core 选区供 P1 前端使用
- [x] MapExisting 升级为 `map_through`：非折叠选区 head/tail 分别取 Start / End bias，任一端点被删整体失败，anchor/focus 方向保留
- [x] gap 端点行为定义：内容编辑 intent 返回 `SelectionInvalid`；caret 移动返回 `NoChange`（跨块导航属 P2.5）
- [x] P1 全部行为回归通过（session.rs 19 个集成测试未改断言语义即通过）

实现说明：

```text
Core selection 类型不动；DocumentSelection 是 runtime 层超集。
listener seam 与 HistoryEntry 的 before/after selection 同步升级为 DocumentSelection。
gpui 单块视图全部经 text_selection() 投影读取；editor 入口用
DocumentSelection::text 包装既有 TextSelection 参数，公开 API 无破坏性变更。
```

完成证据：

```text
分支 feat/p2-document-selection：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿（新增 selection.rs 7 个单元测试，
P1 回归 session.rs 19 个测试不改断言通过）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
```

## P2.3 结构命令编排

- [x] 结构 EditIntent：`SplitBlock` / `JoinWithPrevious` / `TurnInto { kind }`
- [x] AfterSelectionPolicy：`CaretAtSplitTail`（新块起点）/ `CaretAtJoinSeam`（接缝 offset）；TurnInto 走 `MapExisting`
- [x] Enter 命令流 = `SplitBlock`（非折叠选区先删除再拆分，一笔 history）；split 后新块继承被拆 run 的 marks（Core SplitNode 语义）
- [x] Backspace-at-start 解释为 `JoinWithPrevious`；第一块块首仍为 NoChange（P1 回归）
- [x] Core `SetNodeKind`：保留 NodeId / attrs / content，校验 shape 与 parent/child kind；root 不可改 kind
- [x] undo / redo 对 split / join / turn-into 还原 store、identity 与 recorded selection

实现说明：

```text
TurnInto 只做同 shape 转换（Paragraph ↔ Heading ↔ CodeBlock）。把 inline 节点
变成 Quote 等 container 由 Core 以 InvalidNodeContent 拒绝；wrapping / list
留给 P2.4。JoinNodes 保留 first 的 kind，因此 heading 后的 paragraph 在块首
Backspace 会并入 heading。
```

完成证据：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
tools/check_source_size.py 与 tools/check_dependency_boundaries.py
```

## P2.4 List 编辑

- [x] TurnInto(BulletList/OrderedList)：段落 wrap 为单 item list（staged 命令，焦点块 identity 保留）
- [x] TurnInto(Paragraph)：item 内块 lift out；单 item list 整体溶解，paragraph → list → paragraph 闭环
- [x] bullet ↔ ordered 转换走 SetNodeKind（保留 list / item / 块全部 identity）
- [x] IndentListItem：移入前一兄弟 item；前一 item 以嵌套 list 结尾则复用，否则 staged 创建同 kind 内层 list
- [x] OutdentListItem：移入外层 list 紧跟外层 item；被清空的内层 list 同笔删除
- [x] PreserveFocus after-selection policy（结构移动后 caret 折叠回原 focus 点并重新校验）
- [x] undo / redo 对 wrap / lift / indent / outdent 全部还原 exact store 与 recorded selection（含 redo 复用 identity）
- [x] NoChange 语义：首 item indent、顶层 item outdent、同 kind 转换、非 list 段落 TurnInto(Paragraph)

实现说明：

```text
未新增任何 Core step（遵守 design §3.4）；list 命令全部由 InsertNode /
RemoveNode / RestoreSubtree / SetNodeKind 组合。
wrap 与 indent 需要引用 application 期间才分配的容器 NodeId：runtime 引入
staged plan——每个阶段从其看到的 snapshot 按确定性位置重新推导新容器 id，
全部阶段要么全成功并合并为一笔 history entry（undo = 各阶段 inverse 逆序
拼接，redo = inverse(undo)），要么原子失败且 session 状态不变。
lift 的插入下标用 list 在父级中的槽位（list_index），被抬升块出现在残留
list 内容之上；outdent 目标父节点是外层 list 而非外层 item（Core 禁止
ListItem 直接嵌套 ListItem）。
已知边界：item 含多个子块时整体抬升/缩进；空 item 不产生（wrap/lift 均
保证至少一个子块）；Enter 在 item 内仍走 SplitNode（在 item 内拆块），
Backspace 在 item 内块首为 JoinWithPrevious 无前兄弟 → NoChange（不自动
退出 list，留待后续按需评估）。
```

完成证据：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p xiaomu-core -p xiaomu-runtime -p xiaomu-codec-markdown --all-targets
  （新增 tests/list.rs 11 个测试；P1 session 19 个、P2.3 structural 14 个
   回归不改断言通过）
tools/check_source_size.py ok（session/mod.rs 566 行超 review 阈值 500，
  仅 warning，后续切片拆分）
tools/check_dependency_boundaries.py ok
注：本机 WSL 缺 libxkbcommon-x11，gpui/harness 测试无法链接，与 P1–P2.3
各切片环境一致；GPUI 实机验证归 P2.5。
```

## P1 移交的 P2 前置依赖与归属

P1 在 progress.md 与 design.md 中记录了若干"留到 P2"的事项，处置如下：

```text
1. 跨 block selection 属于 session 层
   → P2 引入 runtime DocumentSelection（anchor/focus 端点为 TextPoint 或 NodeGap 级），
     Core TextSelection / NodeSelection 语义不变；渲染投影属 GPUI 层。
2. "Deleted 即原子失败"的 selection fallback 停损
   → P2 结构命令需要显式 AfterSelectionPolicy（join 点 / 邻接位收敛等），
     取代无差别停损；fallback 无法合法化才允许 typed error 原子失败。
3. SplitNode / JoinNodes / MoveNode / list steps 未实现
   → P2.1 落地最小集 SplitNode / JoinNodes；MoveNode 仅在 indent/outdent 组合成本
     过高时评估引入；list 通过 InsertNode/RemoveNode/RestoreSubtree 组合实现。
4. 视觉行导航与 grapheme 光标
   → P2 提供跨 block Left/Right/Home/End/Up/Down 导航（仍按 scalar boundary、
     单行视觉模型）；grapheme cluster 与 BiDi affinity 维持 ADR 0001 后续增强边界。
5. IME composition 跨 block preedit
   → P2 明确停损：composition 只在单 block 内启动，跨块 preedit 属后续增强，
     在实现切片中记录并测试该约束的行为。
6. 宿主集成（planning §13.1 从 P2 开始）
   → P2.6 minimal host-contract harness：load document / listen to changes /
     persist through adapter，不绑定 Markdown codec 生产质量，不引入产品专用类型。
```

## 决策记录

这里只记录影响 P2 执行的决定。长期且难逆转的架构理由应进入 ADR。

### 2026-08-25（P2.0）

- Document selection 放在 runtime session 层（DocumentSelection），不扩展 Core selection 类型——沿用 P0 "跨 block selection 属 session 层"决策，避免 Core contract 为 UI 形态让步。
- 结构编辑 after-selection 用逐 intent 显式 policy 定义，取代 P1 的全局"Deleted 即失败"；这是行为语义变化，P1 回归测试需按新契约审视后迁移。
- Core 最小 step 集 = SplitNode + JoinNodes；WrapList/UnwrapList 不在本阶段引入，list 编辑用既有原语组合，indent/outdent 是否需要 MoveNode 由 P2.4 以实际成本决定。
- GPUI multi-block 采用"保留单块 view 能力 + 外层容器/焦点路由"的渐进替换，不重写渲染路径；所有 block 永远 mounted 只是过渡形态，公开 API 不得固化该假设（planning §10.1）。
- IME composition 在 P2 停损为单块内启动；跨块 preedit 不做。

### 2026-08-26（P2.3）

- TurnInto 通过新的 Core `SetNodeKind` 保持 NodeId，而不是 RemoveNode + InsertNode。kind 变更不移动 position，after-selection 走 MapExisting。
- P2.3 的 TurnInto 只覆盖同 shape 的 inline kind（Paragraph / Heading / CodeBlock）；Quote wrapping 与 list 闭环留给 P2.4。
- Backspace 在块首且存在前一兄弟时由 session 解释为 JoinWithPrevious，前端不必另发结构 intent；显式 `JoinWithPrevious` 在没有前一兄弟时同样是 NoChange。
- Enter 对应 `SplitBlock` intent。GPUI 按键绑定仍属 P2.5；本切片只保证 session 语义与纯逻辑测试。
- Session history 的 redo 存 `inverse(inverse(T))`，不重放原始 SplitNode。否则 redo 会重新分配 tail NodeId，录下的 after-selection 与 JoinNodes inverse 都会指向已消失的 identity。

### 2026-08-27（P2.4）

- List 编辑不新增 Core step（维持 §3.4 决策）。跨 step 引用新分配 NodeId 的问题用 runtime 层 staged plan 解决：每阶段从当前 snapshot 按确定性位置重新推导容器 id，而不是给 Core 加占位符机制或复合 step——保持 Core step 语言最小、inverse/mapping 语义不变。
- Staged 命令的 undo = 各阶段 inverse 按逆序拼成的单笔 transaction，redo = inverse(undo)；与单笔命令共用 HistoryEntry 形态，identity 还原语义一致（已由 store 相等断言锚定）。
- 结构性 list 移动的 after-selection 新增 `PreserveFocus`（焦点块 identity 保留时 caret 折叠回原点），不用 MapExisting——Remove→Restore 组合会把端点判为 Deleted。
- OutdentListItem 的目标父节点是外层 list（紧跟外层 item 之后），不是外层 item：Core 的 `allows_child` 禁止 ListItem 直接嵌套 ListItem。被清空的内层 list 同笔删除。
- Lift out 把被抬升块插在 list 的原槽位（出现在残留 list 之上）；多 item list 只溶解焦点 item，其余 item 不动。
- indent/outdent 未引入 MoveNode step：单笔 RemoveNode + RestoreSubtree 即可表达（组合成本可控，§3.4 的 MoveNode 评估结论为不需要）。

## P2 Phase Gate

P2 只有在以下条件全部满足后才能完成：

- [x] SplitNode / JoinNodes 以 Core step 落地，mapping + inverse 满足随机不变量
- [x] DocumentSelection 成为 session 的 selection 形态，公开读取点全部校验
- [x] 结构命令 after-selection fallback 显式且可测试
- [x] list 日常编辑闭环 undo 可还原（P2.4 纯逻辑层；实机验证归 P2.5）
- [ ] multi-block 渲染 + 跨块导航 + 跨块 selection 实机可用
- [ ] minimal host-contract harness 完成 load / listen / persist 闭环
- [ ] position mapping regression matrix 建立并通过
- [ ] P1 全部 session 行为回归通过
- [ ] 架构文档与实现一致
- [ ] P2 最终 `CI Success` 全绿

## Regression Log

（空）
