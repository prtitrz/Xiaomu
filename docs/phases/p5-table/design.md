# P5 Table Design

## 1. Canonical 模型

```text
NodeKind::Table        NodeContent::Children([TableRow…])
NodeKind::TableRow     NodeContent::Children([TableCell…])
NodeKind::TableCell    NodeContent::Children([block…])
```

决策与理由：

```text
Table/TableRow/TableCell 都是普通树节点（NodeId、attrs、Children content）
  → parent / validate_tree / ChangeMap / clipboard projection 全部免费复用
cell 内放一个或多个合法 block，不放裸 inline（默认新 cell 为一个空 Paragraph）
  → cell 内复用既有 caret / gap / SplitBlock / marks / atom 全套语义
表格不变量属于 canonical validation：
  → 每个 TableRow 的 cell 数一致（列数由行推导，不另设 attrs 冗余）
  → 每个 TableCell 至少一个 child block（纯 atomic 内容也合法）
  → allows_child：Table↔TableRow、TableRow↔TableCell；
    Document/Quote/ListItem/TableCell 可包含 Table（嵌套表允许，结构 op 只作用于最内层所属表）
baseline 不设 header row / column alignment / 列宽 attrs
  → 列宽是前端 layout 关切；header/alignment 若后续纳入，走 attrs 扩展，不改 content 形状
```

`validate_tree`（`crates/xiaomu-core/src/document/snapshot.rs`）新增：

```text
Table 的 children 全是 TableRow 且 ≥1 行
TableRow 的 children 全是 TableCell 且 ≥1 cell
同一 Table 下所有行 cell 数一致
TableCell 的 children ≥1 且不含 Table 之外的容器约束沿用 allows_child
```

表格构造是 Core 语义步骤 `TransactionStep::InsertTable { parent, index, rows, columns }`（P5.1 实施修订）：每个 stage 事务都会独立通过 `validate_tree`，而表格中间态（有行无 cell、有 cell 无段落、空表）必然非法，因此 `InsertNode` 分层 staging 无法表达嵌套构造。`InsertTable` 与 `InsertInlineAtom` 同一哲学——Core 分配 table/row/cell/cell 内空 Paragraph 的全部 fresh identity，产出的 snapshot 直接满足表格不变量，inverse 为 `RemoveNode`（子树随删）。P5.3 插入行/列同样使用 Core 语义步骤；删除由单事务 `RemoveNode` 组合承载，只校验最终快照，见 §3。

## 2. 位置与选区

cell 是普通容器，因此 P0-P4 的全部位置语义原样成立：

```text
cell 内 caret      DocumentPosition::Inline(InlinePoint)  —— node_id 是 cell 内的段落
cell 内选区        既有 text selection（可跨 cell 内多段落）
cell 内 gap        NodeGap(parent=cell 段落或 cell 容器)
cell 内 atomic     DocumentPosition::Atomic(image/HR/inline-atom 语义不变)
```

P5.5 新增唯一的新选区形态：

```text
DocumentSelection 新增 cell_range: Option<CellRange> 字段（实施修订：保持 Copy，
  不改枚举形态）：
  cell-range：同一 Table 内 anchor cell 与 focus cell 构成的矩形（端点存 cell 身份）
validate：两端为同表 TableCell
map_through：矩形随 cell 身份走——插入不动矩形；端点子树被删才收缩为存活端点；
  两端皆亡（或 parked caret 被删）→ 收敛到删除缝（NodeRemoved 的 parent+index gap）
矩形选区不与 text selection 混存：range 激活时 text 端点停靠在 anchor cell 行缝；
  Delete/Backspace/Cut 清空整个矩形并保留 cell 身份/attrs 和选区；
  typing 等其他内容 intent 先收敛到 anchor cell 首个可编辑块起点；
  表结构 op 与 structured paste 保留矩形（前者映射、后者替换）
```

## 3. 编辑语义

### Tab / Shift+Tab（P5.2）

```text
caret 在 cell 内 → MoveToNextCell：caret 移到下一 cell 首段的 (0, ordinal 0)
最后一个 cell → Core 语义步骤 TransactionStep::InsertTableRow { table, index }（与
  InsertTable 同理：整行构造一次成型，列数取自表首行；step map 报告新行首
  cell 的 paragraph 作为 caret 目标），单 isolated history entry，redo 恢复
  同一批 node id
Atomic 焦点（cell 内 HR/Image）同样导航；Gap 焦点不导航
Shift+Tab 反向；第一个 cell 上 no-op
GPUI keybinding 上下文优先级：table cell > list item（Tab 缩进）> paragraph（Tab 变列表）
```

### Enter / Backspace / Delete（P5.2）

```text
Enter 在 cell 内段落 → 既有 SplitBlock（新段落留在同 cell）
Backspace 在 cell 首段 (0,0) → baseline no-op（不 join 前 cell；cell join 的
  atom/嵌套迁移语义明确后另立切片，本切片记录为已知边界）
Delete 在 cell 末尾 → 同理 no-op
普通 cell 内文本/IME 复用既有 intent；矩形清空为单独 Runtime planner，保留结构与精确 undo
```

### 行列操作（P5.3，实施修订）

```text
InsertTableRow { table, index }      Core 语义步骤（修订：单一新行使表在命令中途
                                     非法，validated staging 无法表达，与 P5.1 同理）
InsertTableColumn { table, index }   Core 语义步骤：每行 index 处插一个空 cell（空
                                     Paragraph），每行分别产生 NodeInserted 映射，覆盖所有 row-local gaps
DeleteTableRow { table, index }      单步 RemoveNode(row)；表只剩一行时 planner fail
                                     closed（Core 侧最终快照验证同样拦截）
DeleteTableColumn { table, index }   单事务内每行一个 RemoveNode(cell)；中间态 ragged
                                     无妨（事务只有最终快照验证），最后一列 fail closed
SelectionUpdate：
  被删区域内的 caret/selection（含 Gap/Atomic 焦点）→ CaretAtGap（行缝/cell 缝）
  其余 selection → MapExisting（结构映射调整缝隙索引）
  插入不自动聚焦（caret 原地保留，MapExisting），由 Tab/点击进入
undo / redo：整命令一个 isolated history entry；删除的 inverse 恢复同一批 node id
```

结构 op 与 P4 内容共存：

```text
删行/删列时 cell 内可有 atom / image / inline atom → RemoveNode 子树删除即完整载荷删除，undo 精确恢复
插入列不复制既有 cell 内容（空 cell），避免隐式数据复制
```

## 4. Clipboard（P5.5）

```text
wire v6：
  ClipboardNodeContent::Table { rows: Vec<Vec<ClipboardNode>>, row_attrs: Vec<NodeAttrs> }
  table/cell attrs 原位保存，row_attrs 与 rows 一一对应；块内容递归捕获
兼容：v5 无 row_attrs 读作每行空属性；非表片段继续写 v4；v4 不携带 table
未知版本、非法结构、fallback 不匹配 → metadata fail soft，回退系统 plain text
plain-text fallback：TSV——cell 以 \t 分列、table row 以 \n 分行；cell 内所有 CRLF/CR/LF/tab
  与 block 边界扁平化为空格；结构化 payload 不改文本、marks、atoms 或属性
paste（P5.6 修订，见 ADR 0006）：
  表载荷 + 同尺寸 cell-range → 逐 cell 替换内容，保留目标 table/row/cell attrs
  1×1 表载荷 + caret 在 cell 内 → payload 块插在 caret 所在直属 cell block 后
  cell 外 → 新建兄弟表，完整保留源 table/row/cell attrs 与递归子内容
  nested table / quote / list / code / image / HR / atoms 均经 validated stages 重建 fresh IDs
  mixed roots / 尺寸不符 / 其余落点 fail closed（ClipboardTableUnsupported）
  整命令一个 history entry；undo 恢复原内容、原身份与选区，redo 恢复重建身份
markdown：GFM table 不入 P4.9 baseline codec；Table 节点导出走既有 UnsupportedNodeKind
```

## 5. GPUI（P5.4）

```text
TableBlockPresentation：
  grid 布局（gpui div + 等宽列），cell 边框 / focus affordance
  cell 内块渲染递归复用既有 block element（不做表格专用第二套段落渲染）
  caret / selection / IME：复用 cell 内普通块的投影
  hit-test：先定位最内层包含点击的 cell bounds，再按二维距离选其子树内文本块
keyboard：Tab / Shift+Tab 在 cell 上下文优先；矩形扩选用 Alt+Shift+方向键
mouse：Alt+单击单格；跨格拖选/Shift+单击矩形扩选；composition 期间不移动选区
history：undo/redo 同步新 child view 并恢复 native focus
accessibility：Table/TableRow/TableCell 投影为对应 role，cell 内容递归投影
```

## 6. Fixture / codec

```text
harness fixture v5：table 块行编码
  table\t<rows>\t<cols> 行 + end 包裹 row/cell 容器（复用 quote/ul 的 end 栈模型）
  cell 作为容器帧（Frame::TableCell），cell 内沿用 p/code/atom/img 行
  v2/v3/v4 读兼容；旧 header 下 table 帧拒绝，Table 在 v4 写路径仍 fail closed
  save 自动选择 v4/v5；维度、非空 cell、嵌套/end 栈均校验
markdown codec：不变（Table 导出 fail closed，见 §4）
```

## 7. 测试矩阵

```text
P5.1  builder/validate/invariant 矩阵（行列一致、空 cell、嵌套表、非法嵌套）
P5.2  中英文 cell 连续编辑 + undo/redo；Tab 链全表行走；last-cell 追加行
P5.3  行列插入删除 × caret 位置 × undo/redo；最后一行/列 fail closed；atom 载荷共存
P5.4  GPUI e2e：text ↔ table ↔ text 键盘鼠标、cell 内编辑、Tab 行走
P5.5  矩形选区 copy/paste/undo、TSV fallback、wire 往返、mixed fail closed
P5.6  realistic fixture + Unicode 矩阵 + 多 editor 隔离 + Windows/CI
```

语义决策见 [ADR 0006](../../adr/0006-table-clipboard-contract.md)，本阶段最终证据见 [P5.6 Gate](p5.6-integration-gate.md)。
