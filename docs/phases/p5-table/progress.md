# P5 Table Progress

## Current status

P0-P4 已关闭。P5 于 2026-09-05 启动。

```text
P5.1 Table Canonical Model        CLOSED
P5.2 Runtime Cell Editing         CURRENT
P5.3 Row / Column Operations      PENDING
P5.4 GPUI Table Rendering         PENDING
P5.5 Cell Selection / Clipboard   PENDING
P5.6 Integration Gate / Closeout  PENDING
```

## P5.1 Table Canonical Model — CLOSED（PR #80）

- [x] `NodeKind::Table / TableRow / TableCell` + content-shape validation
- [x] `allows_child`：Table/TableRow/TableCell 容器规则，TableCell 可入 Document/Quote/ListItem/TableCell
- [x] `validate_tree` 不变量：行数 ≥1、cell 数 ≥1、同表列数一致、cell 非空；新 `Error::InvalidTableStructure`
- [x] builder 构造矩阵 + 非法形状 fail closed 测试（`crates/xiaomu-core/tests/table_model.rs`）
- [x] 表格构造 = Core 语义步骤 `TransactionStep::InsertTable`（实施修订：stage 事务独立验证使纯 `InsertNode` staging 无法表达嵌套构造；见 design.md §1）+ degenerate 尺寸 fail closed + inverse 全子树删除
- [x] runtime seam：`EditIntent::InsertTable { rows, columns }` 在聚焦块后插入整表（单 isolated history entry，caret 原地保留）
- [x] P0-P4 regression 保持全绿（419 tests / clippy -D warnings / fmt / size / dependency guards）

## P5.2 Runtime Cell Editing — PENDING

- [ ] `EditIntent::MoveToNextCell / MoveToPreviousCell`
- [ ] last-cell Tab 追加新行；first-cell Shift+Tab no-op
- [ ] cell 内 Enter / Backspace / Delete 边界行为（Backspace cell 起点 no-op 记录）
- [ ] cell 内 typing coalescing / IME / stored marks 复用验证
- [ ] undo / redo 精确矩阵

## P5.3 Row / Column Operations — PENDING

- [ ] `InsertTableRow / DeleteTableRow / InsertTableColumn / DeleteTableColumn`
- [ ] staged transaction + 单 history entry + inverse
- [ ] SelectionUpdate（CaretAtGap / MapExisting）映射矩阵
- [ ] 最后一行/列删除 fail closed
- [ ] 结构 op 与 atom/image 载荷共存 + undo/redo

## P5.4 GPUI Table Rendering — PENDING

- [ ] table grid layout / borders / focus affordance
- [ ] cell 内块渲染递归复用
- [ ] caret / selection / IME / hit-test 复用验证
- [ ] Tab / Shift+Tab keybinding 上下文（cell > list > paragraph）
- [ ] e2e：text ↔ table ↔ text 键盘鼠标

## P5.5 Cell Selection / Clipboard — PENDING

- [ ] cell-range selection variant + validate + mapping
- [ ] clipboard wire v5 table 载荷 + 旧版本 fail-soft
- [ ] TSV plain-text fallback
- [ ] paste 单/多 cell + mixed fail closed
- [ ] markdown Table 导出 fail closed 断言

## P5.6 Integration Gate / P5 Closeout — PENDING

- [ ] realistic table fixture（fixture v5）
- [ ] Unicode + cell + atom matrix
- [ ] multi-editor isolation
- [ ] architecture / planning / progress final sync
- [ ] Windows real-machine Gate
- [ ] final three-platform `CI Success`

## P5 Phase Gate

- [ ] 表格中英文连续编辑 + undo/redo（planning 总 Gate）
- [ ] Tab / Shift+Tab 全表稳定行走
- [ ] 行列操作 caret/selection 可预测
- [ ] cell 矩形选区 clipboard 无损
- [ ] canonical 不变量由 Core validation 持有
- [ ] 三平台 CI + Windows 实机 Gate 全绿

只有上述 Gate 完成，才允许 **P5 = CLOSED** 并进入 P6 Performance。
