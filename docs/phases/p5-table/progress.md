# P5 Table Progress

## Current status

P0–P3 已关闭。P4 功能交付完成，历史 Windows CI 被误记为实机 Gate 的条目已更正；人工证据随 P5.6 补齐。P5 于 2026-09-05 启动，当前记录更新于 2026-10-23。

```text
P5.1 Table Canonical Model        CLOSED
P5.2 Runtime Cell Editing         CLOSED
P5.3 Row / Column Operations      CLOSED
P5.4 GPUI Table Rendering         CLOSED
P5.5 Cell Selection / Clipboard   CLOSED
P5.6 Integration Gate / Closeout  IN PROGRESS — local fixes, external gates open
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
- [x] last-cell Tab 追加新行（Core 语义步骤 `TransactionStep::InsertTableRow`，step map 报告新行首 cell paragraph 作 caret 目标；redo 恢复同一批 node id）；first-cell Shift+Tab no-op
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
- [x] caret / selection / IME / hit-test 复用验证（`text_blocks` 本就收集 cell 段落，P5.6 已补 cell bounds + 二维命中，修复原共享 registry 仅按纵坐标选块的问题）
- [x] Tab / Shift+Tab keybinding 上下文（cell > list > paragraph：修复 cell 段落 offset 0 上 Tab 误转列表）
- [x] e2e：`table_gpui.rs` 3 个 `gpui::test`——真实击键 Tab 行走 + last-cell 追加行 + 输入/undo、Up/Down text↔table↔text（文档序）、Enter cell 内分段、点击进入 cell 首段

## P5.5 Cell Selection / Clipboard — CLOSED（PR #84）

- [x] cell-range selection variant + validate + mapping（`DocumentSelection` 新增 `cell_range: Option<CellRange>` 字段保持 Copy；端点为 cell 身份，插入不动矩形、删除才收缩/收敛；公开 seam `set_cell_range_selection`；P5.6 起矩形 Delete/Backspace/Cut 清空所有选中 cell，其他内容 intent 收敛到 anchor cell 首块；表结构 op 映射矩形）
- [x] clipboard table 载荷 + 旧版本 fail-soft（P5.5 为 v5；P5.6 升级 v6，保留 row attrs、兼容读取 v5；非表片段仍为 v4，见 ADR 0006）
- [x] TSV plain-text fallback（cell `\t` 分列、行 `\n` 分行、cell 内 block 边界及 CRLF/CR/LF/tab 扁平化为空格（P5.6 补齐））
- [x] paste 矩阵：range 替换（尺寸匹配，单 history entry）/ 1×1 进 focused cell / 兄弟表插入（`InsertTable` + 逐 cell 填充 + seed 段删除）/ mixed 与尺寸不符 fail closed（`ClipboardTableUnsupported`）
- [x] markdown Table 导出 fail closed 断言（codec 测试 `table_nodes_export_fail_closed`）
- [x] GPUI：range 矩形 cell 高亮（`cell_range_rect`）

## P5.6 Integration Gate / P5 Closeout — IN PROGRESS

代码与自动化收尾已完成；最终验收以 [P5.6 Gate](p5.6-integration-gate.md) 为唯一记录入口。**不能用本机通过或旧 HEAD 的 CI 关闭当前阶段。**

- [x] A1：二维命中 + cell padding/不同行高/三列双行原生点击回归
- [x] A2：插列逐行 NodeGap 映射，覆盖全部列插入位置、双 bias、inverse
- [x] A3：矩形剪切/清空整个范围，保留结构与属性，一次 undo/redo
- [x] A4：新表完整保留 table/row/cell attrs；覆盖保留目标 attrs；wire v6 + v5 兼容
- [x] A5：Core 校验 Table 身份与中间形状，错误输入返回错误而非 panic
- [x] A6：quote/list/code/atom/media/nested table 递归 copy/paste
- [x] A7：TSV cell 内分隔符归一化；v5 旧 fallback 仍能验证并读取
- [x] A8：fixture v5 save/load/reopen，保留 v2/v3/v4 样例与旧版拒绝表格测试
- [x] 原生矩形入口：Alt+单击、跨格拖选、Shift+单击、Alt+Shift+方向键
- [x] Table/TableRow/TableCell accessibility roles
- [x] 集成测试发现并修复：结构 undo/redo 后恢复 native focus，连续撤销不丢击键
- [x] realistic table fixture（CJK/emoji/combining/BiDi、marked atom、quote、结构 attrs；其余 media/list 同文档）
- [x] Unicode + cell + atom + rich content matrix，fresh IDs、精确 store/selection undo/redo
- [x] multi-editor isolation（原生矩形选择、输入、清空、focus 与独立 history）
- [x] architecture / planning / progress / README / ADR final sync
- [ ] Windows real-machine Gate：待记录最终 commit、OS/IME 版本及逐项结果
- [ ] final three-platform `CI Success`：待最终修订提交后的 CI

本地最终工作区：**483 tests passed，0 failed**；Clippy `-D warnings`、fmt、source-size、dependency-boundary 和 diff 检查通过。本批新增 23 项回归测试，并更新既有映射/版本断言。命令、结果与测试文件见 [Gate 验证记录](p5.6-integration-gate.md)。审计原始证据与修复对应见 [2026-10-23 audit](../../audits/2026-10-23-project-audit.md)。

## P5 Phase Gate

- [x] 表格中英文连续编辑 + undo/redo（自动化；人工 IME 仍待下面平台 Gate）
- [x] Tab / Shift+Tab 全表稳定行走
- [x] 行列操作 caret/selection 可预测
- [x] cell 矩形选区 clipboard 在声明的支持矩阵内保真
- [x] canonical 不变量由 Core validation 持有
- [ ] 三平台 CI + Windows 实机 Gate 全绿

只有上述 Gate 完成，才允许 **P5 = CLOSED** 并进入 P6 Performance。P6/P7 不属于本批实现范围。
