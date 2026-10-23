# ADR 0006: Table Clipboard and Rectangular Editing Contract

Status: Accepted
Date: 2026-10-23

## Context

P5 的 Table / TableRow / TableCell 都有 canonical attrs，cell 可包含普通块、atoms、media 和嵌套表格。审计发现原 wire v5 省略 row attrs，新表重建又遗漏 cell attrs；直接文本块专用的捕获路径与合法 cell 模型不一致。矩形剪切还会先复制全范围、再折叠选区只删一个字符。

这些行为影响 Core、Runtime 与 GPUI 的共同契约，需要明确保真、覆盖、兼容与历史边界。

## Decision

1. **矩形清空**：Delete / Backspace / Cut 将每个选中 cell 的全部内容替换为一个空 Paragraph，保留 table/row/cell 身份、属性与矩形选区。一个 isolated history entry；undo 精确恢复原子树和选区。
2. **复制**：递归捕获完整 cell 子树与所有结构属性，包括 marks、同缝 atoms、quote/list/code、Image/HR、嵌套表格。剪贴板不携带可复用的源 NodeId。
3. **新表粘贴**：完整保留源 table/row/cell attrs，重建 fresh IDs。已有矩形覆盖仅替换内容，保留目标 table/row/cell attrs；尺寸必须一致。1×1 进入 focused cell 时插在 caret 所在直属 block 后，保留目标包装属性。
4. **格式**：含表片段写 wire v6，以 `Table { rows, row_attrs }` 保存行包装属性。v6 的 row_attrs 数量必须与 rows 一致；非表片段继续写 v4。v5 缺省空 row attrs，并按 v5 的旧 fallback 规则校验原 body 后转换为当前片段；v4 中含表、未知版本、非法形状和 stale metadata 均 fail soft。
5. **外部文本**：TSV 的 tab/newline 仅划分 cell/table row。cell 内 CRLF/CR/LF/tab 及 block 边界扁平为空格，atom 用 fallback，ExternalUrl image 用 URL。结构化载荷保留原始文本及属性，因此 TSV 只是有损互操作表示。
6. **层次职责**：Core 负责形状验证和完整位置映射。一次插列对每个受影响行产生映射，`ChangeMap::steps()` 可多于 transaction steps。Runtime 负责内容、选择和历史；GPUI 负责二维命中、原生矩形手势和历史变更后的焦点恢复。

## Alternatives considered

- 保持 wire v5 并添加可忽略字段：旧 reader 会成功读取却丢 row attrs，因此升级表格信封版本，旧 reader 回退 plain text。
- 把 cell 限制为直接 Paragraph：与已交付 Core/GPUI 支持的合法块冲突，改为递归捕获/重建。
- 覆盖目标格时替换整个 cell：会改变表结构身份与目标样式，破坏外部 anchor；覆盖只更新内容。
- 用 TSV 引号处理 cell 内换行：接收端方言不统一，当前 baseline 采用明确扁平化；无损传输使用结构化 metadata。

## Consequences

旧应用接收新表格退化为 TSV，非表剪贴板保持兼容。嵌套结构由 validated stages 重建，正确性优先；大表粘贴的阶段数量与成本留给 P6 测量。harness fixture v5 与 clipboard wire v6 是不同私有协议，版本不能混用。

## Revisit when

需要跨应用无损表格、结构样式覆盖策略、合并单元格或大表性能优化时，以新增版本/明确能力扩展此契约，并保留旧版本回归。
