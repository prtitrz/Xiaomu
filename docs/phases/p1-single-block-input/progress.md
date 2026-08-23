# P1 Single Block Native Input 进度

状态：进行中

本文档只记录 P1 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P1 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P1.2 DocumentSession 已完成实现并通过本地检查，等待 PR CI 后合并**

当前分支：`feat/p1-document-session`

前置状态：P0 已完成（PR #13）；P1.0 随 PR #14 合入；P1.1 GPUI 依赖已合入（PR #15）。

## P1.0 Phase Contract 与阶段骨架

- [x] 创建 `docs/phases/p1-single-block-input/design.md`
- [x] 创建 `docs/phases/p1-single-block-input/progress.md`
- [x] 记录 P0 移交的 7 项前置依赖归属（见下方决策记录）
- [x] 运行 source-size 与 dependency-boundary guard
- [x] 运行 `cargo fmt --all -- --check`
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 运行 `cargo test --workspace --all-targets`

完成证据：

```text
分支 feat/p1-phase-contract（本地）：
uv run python tools/check_source_size.py 全绿
uv run python tools/check_dependency_boundaries.py 全绿
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿：18 个 test target，
xiaomu-core 95 个测试全部通过（含 inverse_roundtrip 随机不变量）。
本 PR 的远端 CI Success 即 P1.0 Gate 证据。
```

## P1.1 GPUI 依赖引入

- [x] workspace 依赖表 pin crates.io `gpui = "=0.2.2"`
- [x] `tools/check_dependency_boundaries.py` ALLOWED 表核对（xiaomu-gpui → core + runtime 已声明，无需修改）
- [x] cargo-deny 策略核对 / 必要豁免（新增 NCSA）
- [x] 编译级 smoke（`gpui_platform_layer_links`，不依赖窗口环境）
- [x] Cargo.lock 提交（依赖树约 700 包）
- [x] architecture.md 同步

实现说明：

```text
gpui 0.2.2 是 crates.io 当前最新版（2025-10-22 发布，Apache-2.0），
按 planning §17 以 "=0.2.2" 精确 pin 在 workspace.dependencies，升级走单独 PR。
cargo-deny licenses 需要新增 NCSA：libfuzzer-sys（`(MIT OR Apache-2.0) AND NCSA`）
经 gpui → image → ravif → rav1e 链进入依赖树；NCSA 为 permissive、OSI 批准。
macOS 本机构建需要 Xcode Metal Toolchain 组件（新版 Xcode 拆分下载；
`xcodebuild -downloadComponent MetalToolchain`），因为 gpui build script
在构建期用 xcrun metal 预编译 Metal shader——这是 gpui 自身的构建要求，
与是否打开窗口无关；不使用 runtime-shaders / macos-blade 特性绕过，
避免改变生产渲染路径。
xiaomu-gpui 新增编译级 smoke：px/Hsla 解析 + 链接验证，无窗口、无显示服务器依赖；
crate 文档同步为 GPUI adapter 定位描述。
传递依赖存在 future-incompat 提示（block、proc-macro-error2），不阻塞当前构建。
CI 适配：gpui 的 test 二进制在 Linux 链接期需要 libxcb / libxkbcommon / libxkbcommon-x11，
ubuntu job 增加 apt 安装 libxcb1-dev / libxkbcommon-dev / libxkbcommon-x11-dev；
首轮 CI 证实 macOS runner 自带 Metal Toolchain，ubuntu 链接失败是唯一真实失败
（windows 因 matrix fail-fast 被连带取消，非自身错误）。
```

完成证据：

```text
分支 feat/p1-gpui-dependency：
cargo deny check bans licenses sources 全绿（NCSA 豁免后）
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿（含 xiaomu-gpui gpui_platform_layer_links）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
三平台 CI（ubuntu / windows / macos）以本 PR 远端运行为准。
```

## P1.2 DocumentSession（runtime 编排层）

- [x] DocumentSession：原子 apply + SelectionUpdate resolution + notification seam
- [x] EditIntent → EditPlan（Core transaction + after-selection policy）
- [x] HistoryStack：基础 undo / redo + before/after selection
- [x] SessionOutcome：document change / selection-only change / no-op 分流
- [x] session 单元测试（插入/替换后的 caret、Deleted 拒绝、no-op、redo 清空、undo/redo selection 恢复）

实现说明：

```text
session 编辑流：intent → EditPlan(Transaction + SelectionUpdate) →
apply_with_changes → resolve after-selection → 原子替换 snapshot / selection /
history 并通知。任何失败（Core 拒绝 / selection 映射 Deleted / 新 selection 校验失败）
session 状态保持不变；undo/redo 失败时 history entry 原位放回。
after-selection policy：InsertText → CaretAfterReplacement（range.start + 插入字节长）；
Backspace/Delete → CaretAtEditStart；ToggleMark 与 raw apply → MapExisting
（ChangeMap 向外映射，Deleted 即拒绝，无 fallback）。
no-op 判定在 intent 层：边界 Backspace / Delete / caret 移动、collapsed+空文本 InsertText、
collapsed ToggleMark 返回 NoChange（不调 Core、不推 revision、不发通知、不写 history）；
raw apply 无 no-op 检测，空 transaction 也提交（符合设计 §4.1）。
undo/redo 重放 AppliedTransaction::inverse()（ADR 0003）并直接恢复记录的
before/after selection；undo 后新编辑清空 redo 栈。
caret 移动 / 删除按 Unicode scalar boundary（previous/next boundary 辅助函数），
Home / End 到 paragraph 逻辑首尾；combining sequence 只承诺 offset 合法不 panic。
ToggleMark：选区整体已带该 mark kind 才 RemoveMark，否则 AddMark；collapsed 无 pending-mark。
Core 最小扩展：InlineContent::offset_at（boundary 校验的 offset 构造，
与 TextBuffer::offset_at 对齐；runtime 不再需要重组拼接文本来构造合法 offset）。
```

完成证据：

```text
分支 feat/p1-document-session：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿（runtime 加入 missing_docs 门禁）
cargo test --workspace 全绿（新增 tests/session.rs 16 个集成测试 + intent 边界单元测试）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
cargo deny check bans licenses sources 全绿
```

## P0 移交的 P1 前置依赖与归属

P0.7 在 progress.md 记录了 7 项"进入 P1 前需要明确归属"的依赖，处置如下：

```text
1. Transaction 未携带 before/after selection 与 history_group
   → 归属 runtime：DocumentSession 持有 selection 与 history 栈，
     EditIntent 通过 runtime SelectionUpdate 明确 after-selection，普通 apply 才使用 ChangeMap fallback；
     Core contract 不动。
2. history 栈 / typing coalescing / undo selection 恢复
   → P1 在 runtime HistoryStack 实现基础 undo/redo 与 selection 恢复；
     undo = 重放 AppliedTransaction::inverse()（ADR 0003）；typing coalescing 留到 P3。
3. TextSelection 限单个 inline node 内
   → 维持；跨 block selection 属 P2 document selection。
4. SplitNode / JoinNodes / MoveNode / list / InlineAtom steps 未实现
   → 维持 P2/P4 范围；P1 单 block 内容编辑只使用 ReplaceText / AddMark / RemoveMark，
     不允许删除当前 inline node。
5. Grapheme-cluster 光标移动与视觉 affinity resolution
   → 属 frontend；P1 的 Left / Right / Backspace / Delete 按 Unicode scalar boundary，
     Home / End 到 Paragraph 逻辑首尾；视觉行导航与 grapheme 留作后续增强（ADR 0001 边界）。
6. GPUI 依赖尚未引入
   → P1.1 单独 PR 从 crates.io pin 精确版本（planning §17）。
7. commands.rs / history.rs 占位模块
   → command 行为与 history 编排按 planning §9 在 runtime 层落地；
     Core 占位模块保持不动，待 Core 真需要语义支持时再启用。
```

## 决策记录

这里只记录影响 P1 执行的决定。长期且难逆转的架构理由应进入 ADR。

### 2026-08-23（P1.0）

- selection 状态与 undo/redo 栈在 runtime 侧编排：新建 `DocumentSession`（xiaomu-runtime）持有 selection 与 history；每个 EditIntent 产出 runtime `EditPlan { transaction, selection_update }`，由 intent-specific `SelectionUpdate` 决定 after-selection，不扩展 Core Transaction contract（符合 planning §9、ADR 0003、P0.3 评审结论）。
- P1 不定义结构删除后的 selection fallback：任何导致当前 `TextSelection` 映射为 `Deleted` 的 transaction 都原子失败；父级 `NodeGap` / 邻近 block 收敛留到 P2 document-level selection。
- GPUI 从 crates.io 引入并以精确版本 pin（`gpui = "=x.y.z"`），升级走单独 PR；不使用 git revision 依赖。
- undo 实现为会话内重放 `AppliedTransaction::inverse()`，round-trip 要求 store 完全相等；history entry 保存 before/after selection，恢复时直接校验并使用对应 selection；typing coalescing / history grouping 留到 P3。
- caret 移动 P1 按 Unicode scalar boundary；Home / End 到 Paragraph 逻辑首尾；视觉行导航和 grapheme cluster 光标留作后续增强。
- IME `CompositionState` 位于 GPUI adapter，维护 base selection、preedit、preedit selection 与 virtual projection；composition 全程不写 canonical document，commit 单笔入历史，cancel 恢复 composition 前状态。
- 合法空操作返回 `SessionOutcome::NoChange`，不调用 Core apply，不增加 revision / notification / history。
- P1 文档沿用 P0 惯例：阶段文档中文，代码标识 / API 名 / 公开 rustdoc 用英文。

### 2026-08-22（P1.1）

- GPUI 采用 crates.io 发布版而非 git revision：`gpui = "=0.2.2"`（crates.io 当前最新），精确 pin 在 workspace.dependencies；升级只走单独 PR。
- cargo-deny licenses 新增 NCSA 豁免：来源是 libfuzzer-sys（libFuzzer 绑定）经 image → ravif → rav1e 的必然传递链，无法通过 feature 裁剪；NCSA 为 permissive、OSI 批准。
- macOS 构建 gpui 需要 Xcode Metal Toolchain 组件（build script 构建期预编译 Metal shader）。不采用 gpui 的 runtime-shaders / macos-blade 特性绕过，避免改变生产渲染路径；本地环境用 `xcodebuild -downloadComponent MetalToolchain` 一次性解决。
- xiaomu-gpui 的 P1.1 交付是编译级 smoke（px / Hsla 解析 + 链接验证），不引入窗口依赖；gpui 类型暂不进入任何公开 API。

### 2026-08-22（P1.2）

- Core 最小扩展 `InlineContent::offset_at`：runtime 需要 boundary 校验的 offset 构造（caret 移动 / after-selection resolution），此前只能在 runtime 侧重组拼接文本绕行；与 `TextBuffer::offset_at` 语义对齐。
- no-op 判定只在 intent 层；`DocumentSession::apply`（raw transaction）无 no-op 检测，空 transaction 正常提交并推进 revision，与设计 §4.1 一致。
- 删除当前 inline node 的 transaction 映射为 `Deleted` 时原子失败（`SessionError::SelectionDeleted`），P1 不做父级 NodeGap / 邻近 block 收敛（P2 document selection 统一定义）。
- ToggleMark 语义：选区内所有重叠 piece 均带该 mark kind 才移除，否则整体添加；collapsed selection P1 无 pending-mark，返回 NoChange。
- undo/redo 失败路径：history entry 原位放回（restore_undo / restore_redo），session document / selection 不受影响。
- runtime 加入 `#![warn(missing_docs)]`，与 core 的 rustdoc 门禁一致。

## P1 Phase Gate

P1 只有在以下条件全部满足后才能完成：

- [x] GPUI 依赖 pinned 引入，dependency-boundary guard 全绿
- [x] DocumentSession 编排 snapshot / selection / history，无绕过路径
- [x] selection 在任何公开读取点针对当前 snapshot 合法
- [x] undo / redo 恢复精确 store 状态与合理 selection
- [x] intent-specific SelectionUpdate 正确处理插入、替换、删除与 mark
- [x] Deleted update 原子失败；no-op 不增加 revision / notification / history
- [x] undo 后新编辑清空 redo
- [ ] IME composition 不触碰 canonical document，commit / cancel 语义正确
- [ ] 单 block 键盘编辑 + hit-test + clipboard + 基础 marks 实机可用
- [ ] 真实 IME（Microsoft Pinyin）+ selection + undo 手动 Gate 通过
- [x] session 纯逻辑测试在 CI 无显示器环境全绿
- [ ] 架构文档与实现一致
- [ ] P1 最终 `CI Success` 全绿

## Regression Log

（暂无）
