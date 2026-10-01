# P5 Table Progress

## Current status

**P5 CLOSED（2026-10-01）**。P0-P3 已关闭；本轮一并补齐 P4 原生 Windows Gate。P5 于 2026-09-05 启动，P5.1–P5.5 原切片合并后，在 PR #85 完成 correctness 修复、P5.6 集成验收与两个原生 IME 阻塞修复。最终代码 `6d09167` 的 490 项本地测试、三平台 CI 和下方分版本记录的 Windows 原生验收均通过；P6 尚未启动。

```text
P5.1 Table Canonical Model        CLOSED
P5.2 Runtime Cell Editing         CLOSED
P5.3 Row / Column Operations      CLOSED
P5.4 GPUI Table Rendering         CLOSED
P5.5 Cell Selection / Clipboard   CLOSED
P5.6 Integration Gate / Closeout  CLOSED
```

## P5.1 Table Canonical Model — CLOSED（PR #80）

- [x] `NodeKind::Table / TableRow / TableCell` + content-shape validation
- [x] `allows_child`：Table/TableRow/TableCell 容器规则，TableCell 可入 Document/Quote/ListItem/TableCell
- [x] `validate_tree` 不变量：行数 ≥1、cell 数 ≥1、同表列数一致、cell 非空；新 `Error::InvalidTableStructure`
- [x] builder 构造矩阵 + 非法形状 fail closed 测试（`crates/xiaomu-core/tests/table_model.rs`）
- [x] 表格构造 = Core 语义步骤 `TransactionStep::InsertTable`（实施修订：stage 事务独立验证使纯 `InsertNode` staging 无法表达嵌套构造；见 design.md §1）+ degenerate 尺寸 fail closed + inverse 全子树删除
- [x] runtime seam：`EditIntent::InsertTable { rows, columns }` 在聚焦块后插入整表（单 isolated history entry，caret 原地保留）
- [x] P0-P4 regression 保持全绿（419 tests / clippy -D warnings / fmt / size / dependency guards）

## P5.2 Runtime Cell Editing — CLOSED（PR #81）

- [x] `EditIntent::MoveToNextCell / MoveToPreviousCell`（caret-only 导航，无事务无 history；Atomic 焦点导航，Gap 焦点不导航）
- [x] last-cell Tab 追加新行（Core `InsertTableRow` 的 map 报告实际新行，Runtime 解析首个 inline 后代作 caret 目标；redo 恢复同一批 node id）；first-cell Shift+Tab no-op
- [x] cell 内 Enter / Backspace / Delete 边界行为（Backspace cell 起点 no-op 记录；Enter split 留在原 cell）
- [x] cell 内 typing coalescing / IME（isolated entry + stored marks 复用）验证
- [x] undo / redo 精确矩阵（`crates/xiaomu-runtime/tests/p5_cell_editing.rs` 9 tests + `table_model.rs` row-append 矩阵）
- [x] gates：fmt / clippy -D warnings / workspace all-targets / source-size（apply.rs 拆出 apply/table.rs）/ dependency-boundary

## P5.3 Row / Column Operations — CLOSED（PR #82）

- [x] `InsertTableRow / DeleteTableRow / InsertTableColumn / DeleteTableColumn`（intent 携带 `{ table, index }`）
- [x] 实施修订：插入走 Core 语义步骤（`InsertTableRow` 推广为带索引；新增 `InsertTableColumn`），删除走单事务 `RemoveNode` 组合（staging 无法表达表格中间态，见 design.md §3）；单 isolated history entry + 精确 inverse
- [x] SelectionUpdate 映射矩阵：被删子树内（inline/atomic/gap 焦点）→ `CaretAtGap`；其余 → `MapExisting`；插入不抢 caret
- [x] 最后一行/列删除 fail closed（planner 前置校验 + Core 最终快照验证双保险）
- [x] 结构 op 与 inline atom 载荷共存（undo 恢复同一批 node id）+ typing history 共存（`p5_row_column_ops.rs` 12 tests）

## P5.4 GPUI Table Rendering — CLOSED（PR #83）

- [x] table grid layout / borders / focus affordance（`document_view/table_block.rs`：每行 flex row、cell 边框、表级聚焦蓝框、选中 cell 底色、空 cell 最小高度）
- [x] cell 内块渲染递归复用（走既有 `render_block_tree`，cell 内 heading/list/quote 展示与 atom 渲染不变）
- [x] cell 内容复用普通块投影；本轮修正鼠标命中：先约束到完整 cell bounds，再按二维 block bounds 投影，覆盖左右同高与短 cell 空白区
- [x] Tab / Shift+Tab keybinding 上下文（cell > list > paragraph：修复 cell 段落 offset 0 上 Tab 误转列表）
- [x] e2e：`table_gpui.rs` 3 个 `gpui::test`——真实击键 Tab 行走 + last-cell 追加行 + 输入/undo、Up/Down text↔table↔text（文档序）、Enter cell 内分段、点击进入 cell 首段

## P5.5 Cell Selection / Clipboard — CLOSED（PR #84）

- [x] cell-range selection variant + validate + mapping（公开 Runtime seam `set_cell_range_selection`；端点为 cell 身份；表结构 op 映射矩形）。本轮修正 Delete/Cut 清空全矩形，typing 清空矩形后写入 anchor，不再隐式只改首 cell
- [x] clipboard wire v5 table 载荷 + 旧版本 fail-soft（仅含 table 的载荷升级 v5 信封，v4 reader 对未知 tag/版本静默回退 plain text；非 table 载荷保持 v4）
- [x] TSV plain-text fallback（cell `\t` 分列、行 `\n` 分行、cell 内 block 边界扁平化为空格）
- [x] paste 矩阵：range 替换（尺寸匹配，单 history entry）/ 1×1 进 focused cell / 兄弟表插入（`InsertTable` + 逐 cell 填充 + seed 段删除）/ mixed 与尺寸不符 fail closed（`ClipboardTableUnsupported`）
- [x] markdown Table 导出 fail closed 断言（codec 测试 `table_nodes_export_fail_closed`）
- [x] GPUI：range 矩形 cell 高亮（`cell_range_rect`）

## P5.6 Integration Gate / P5 Closeout — CLOSED（PR #85）

- [x] GPUI 矩形入口：Ctrl/Cmd+Shift+Space、Shift+方向键、cell 选择柄拖动；原生输入代理与焦点恢复；自动化 cut/paste/typing/IME cancel/commit/undo
- [x] Up/Down 按视觉列移动，覆盖不同行高、wrapped cell、空格、嵌套表及 text↔table 边界；修复长内容撑开列宽，旧 row-major 断言已替换
- [x] realistic table fixture（fixture v5；rich/nested table、各层 scalar attrs、旧版本读兼容、非法表 fail closed、adapter save/load）
- [x] Unicode + cell + atom matrix（rich clipboard、同锚点 atoms、CJK/emoji/combining/ZWJ、换行及精确 undo/redo）
- [x] multi-editor isolation（独立 window/session/range proxy、输入/clipboard/undo 不改变另一 editor）
- [x] architecture / planning / progress final sync（含 P4 原生 Gate 补验及 P7 宿主接口归属）
- [x] Windows 原生实机 Gate（下方按代码版本、操作者、步骤记录；涵盖 P4 遗留 atom/atomic + P5 表格输入法矩阵，不以 TestAppContext 或 windows-latest 代替）
- [x] 最终代码 `6d09167` 三平台 `CI Success`（[run 36838449485](https://github.com/prtitrz/Xiaomu/actions/runs/36838449485)：Windows / macOS / Ubuntu + policy + aggregate 全绿；不是沿用此前 `809afb5` / `6a861cb` 的绿灯）
- [x] P4 遗留 `BlockRendererRegistry` / `LinkOpenService` 列入 P7 Host Extension Contracts，具体归属与验收见 planning P7；不冒充 P4/P5 已交付

## 2026-10-01 Review 修正（commit `16f9d36`）

- [x] 右侧同高 cell 点击不再误入左侧；短 cell 空白命中限制在该 cell 内
- [x] Cut/Delete/Backspace 清空全部选中 cell、保留表形状/身份/attrs，exact undo/redo；输入替换为一次独立历史记录
- [x] `InsertTableColumn` 对每一行发出真实 `NodeInserted` map；`InsertTableRow` map 指向实际 row，由 Runtime 单独解析 caret
- [x] 复制/粘贴合法 rich cell 子树（quote/list/atomic/nested table/marks/inline atoms），不限定直接 inline child
- [x] table/row/cell/block attrs 无损；行属性使 wire 升 v6，普通表保持 v5，非表保持 v4；旧信封带新特性 fail soft
- [x] 完整矩形枚举集中于 `CellRange::cells`，供 Runtime 命令、clipboard、GPUI 高亮共用

回归证据：`p5_review_regressions.rs`、`p5_rich_table_clipboard.rs`、`p5_cell_range_clipboard.rs`、`table_model.rs`、`table_gpui.rs`。这些是本地自动化证据，不是原生实机验收，也不自动关闭 P5.6。

本地 Windows 检查（2026-10-01，`agent/p5-review-fixes`，code head `6d09167`）：`cargo test --workspace --all-targets --locked` **490 tests PASS**；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、source-size、dependency-boundary、`git diff --check` PASS。实现已提交并推送到 [PR #85](https://github.com/prtitrz/Xiaomu/pull/85)。三平台 CI 独立记录于上方；不以 CI 替代下方的原生验收。

## 原生验收发现的 IME 阻塞与修复

- `809afb5` 标签后有候选框但无拼音预编辑：text-only selection 丢失 atom ordinal，组合布局也未保留 chip。`6a861cb` 修正完整 inline gap、预编辑/候选框几何和 commit 落点；新增 5 个 `ime_atom_tests.rs` 回归。
- `6a861cb` 按 Esc 后仍显示带下划线拼音：用户报告后，临时回调日志证明 empty marked text 和 cancel 均已到达，composition=false、history=(0,0)，没有把拼音写入正文。真实绘制日志显示应绘制 87 bytes 却仍绘制旧的 92 bytes，定位为 intrinsic width 无 key 与 preedit 无 key 的 `None == None` 误命中，不是 GPUI 丢失取消消息。
- `6d09167` 只允许明确有效的 key 命中缓存；没有引入 Windows 特判、unsafe、定时刷新或 GPUI 升级。`element_tests.rs` 在 MinContent/MaxContent 测量路径重现旧代码失败（实际 shaped text 仍含 `zhongwen`），修正后 PASS；另补多帧预编辑更新/取消后的真实 shaped text 断言。诊断日志代码已移除。

## 2026-10-01 Windows 原生验收记录

- 测试代码：首轮 `809afb5`，修复复测 `6d09167`，本地 debug `xiaomu-editor-harness.exe`，默认 fixture v5；独立临时存档，不覆盖用户既有文档。
- 环境：Windows registry 报告 `Windows 10 IoT Enterprise LTSC 2024`，DisplayVersion `24H2`，build `26100.9168`；Microsoft Pinyin `ChsIME.exe` file version `10.0.26100.8972`。
- 操作者与方法：首轮由用户在真实 Windows 窗口手动操作，Codex 提供步骤、检查截图/fixture/日志。computer-use 曾返回 `failed to activate captured window`，失败操作不算证据。后续恢复成功，Codex 通过 computer-use 在同机真实窗口按物理键、逐步检查截图，完成下列注明 `6d09167` 的复测（非 `simulate_input` 或 TestAppContext）。
- [x] `809afb5` 普通及嵌套表格 cell 内微软拼音输入：预编辑文字、候选框、中文上屏正常。用户针对该步骤确认“正常”；截图显示嵌套 cell 内新增中文。
- [x] `809afb5` 第一行左格 `Ctrl+Shift+Space` → `Shift+Right` 选中两格；拼音后 `Esc` 取消保持原内容；再次确认“你好”后仅 anchor 留下文本、其余 cell 清空；`Ctrl+Z` 恢复原矩形内容、`Ctrl+Y` 重做、继续输入。用户针对这三步确认“正常”；后续通用 Esc 画面残留另按上节定位修复，不能以这次简短确认否定后报缺陷。
- [x] `6d09167` 原失败位置输入 `zhongwen`，候选框与下划线预编辑可见；Esc 后整段拼音和下划线消失，原文/光标保留。随后 `ni` + Space 正常确认“你”，Ctrl+Z / Ctrl+Y 可撤销/重做。
- [x] `6d09167` `@xiaomu` 前输入 `ni` + Space，“你”落在 chip 前；undo 后 Right 一次跨到 chip 后，预编辑与候选框在正确位置、chip 不消失；Esc 清除、再次 `ni` + Space 落在 chip 后；undo 后 Left 一次回到 chip 前。
- [x] `6d09167` 点击 horizontal rule 呈蓝色整节点选择，Left 回到前一引用段落末尾；`ni` + Space 可继续输入“你”，undo 恢复原文。
- [x] `6d09167` 第一格 Tab 到右格，Shift+Tab 回左格；外表末格 Tab 新增第三行并聚焦其首格。新格输入 `n` 后 Esc 完全清除，再 Ctrl+Z 直接撤销追加行，证明取消未制造 history entry。
- [x] `Ctrl+S` 保存 fixture v5（两次 `snapshot saved` 日志 + 文件存在，保留 rich/nested table、attrs、atom/image 与用户输入）
- [x] 原生程序重开读取同一存档：`loaded from` 日志、rich/nested outline 和截图确认用户已保存的中文/数字、嵌套表、mention 与 image 均保留；先后启动 atom 修复版和 Esc 修复版都成功读取。

人工/原生确认只覆盖明确列出的步骤，不扩写成所有输入法、平台或所有 Unicode 组合都经过实机验收。Unicode/attrs/clipboard/多 editor 完整矩阵另由永久自动化测试覆盖。Codex 复测插入的文字和追加行均已 undo，保留用户原有测试内容。

## P5 Phase Gate

- [x] 表格中英文连续编辑 + undo/redo（原生记录 + `p5_cell_editing.rs` / GPUI 矩阵）
- [x] Tab / Shift+Tab 全表稳定行走（原生普通/末格 + 自动化完整位置矩阵）
- [x] 行列操作 caret/selection 可预测（Core maps + `p5_row_column_ops.rs` / review regressions）
- [x] cell 矩形选区 clipboard 无损（rich/attrs/Unicode 矩阵 + GPUI 输入/undo）
- [x] canonical 不变量由 Core validation 持有（非法形状、事务原子性、精确 inverse）
- [x] 三平台 CI + Windows 实机 Gate 全绿（自动化与原生操作分别取证）

上述 Gate 已闭合，**P5 = CLOSED**。下一步为 P6 Performance：先建立长文档/复杂表格/多 editor 的可重复 benchmark 与 profile 基线，再依据实测瓶颈安排缓存优化和 virtualization；不将此处 closeout 误记为 P6 已开始。
