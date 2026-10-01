# P5 Table Design

> 状态：**CLOSED**。本文保留阶段设计契约；完成情况及验收范围见 [overview](overview.md) / [progress](progress.md)。

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
cell 内放至少一个 block（新建空 cell 使用 Paragraph），不放裸 inline
  → cell 内复用既有 caret / gap / SplitBlock / marks / atom 全套语义
表格不变量属于 canonical validation：
  → 每个 TableRow 的 cell 数一致（列数由行推导，不另设 attrs 冗余）
  → 每个 TableCell 至少一个 child block（允许 atomic-only；文本 caret 仅落在 inline 后代）
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

表格构造是 Core 语义步骤 `TransactionStep::InsertTable { parent, index, rows, columns }`（P5.1 实施修订）：每个 stage 事务都会独立通过 `validate_tree`，而表格中间态（有行无 cell、有 cell 无段落、空表）必然非法，因此 `InsertNode` 分层 staging 无法表达嵌套构造。`InsertTable` 与 `InsertInlineAtom` 同一哲学——Core 分配 table/row/cell/cell 内空 Paragraph 的全部 fresh identity，产出的 snapshot 直接满足表格不变量，inverse 为 `RemoveNode`（子树随删）。行列插入同理使用 `InsertTableRow / InsertTableColumn`；删除用单事务内 `RemoveNode` 组合，只有事务最终快照验证，而不是把不合法中间态分成独立 stage。

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
  Delete/Backspace/Cut 清空整个矩形（每 cell 留空 Paragraph、保留 cell 身份/attrs）；
  typing/plain-text paste/IME commit 清空矩形并仅在 anchor cell 写入文本，caret 随输入；
  Runtime Tab 从 anchor 定位相邻格；GPUI 普通方向键/Escape 退出到 focus cell 首个可导航后代；表结构 op 映射矩形；
  未定义的格式/结构内容命令 fail closed，不静默改第一个 cell
```

## 3. 编辑语义

### Tab / Shift+Tab（P5.2）

```text
caret 在 cell 内 → MoveToNextCell：caret 移到下一 cell 首段的 (0, ordinal 0)
最后一个 cell → Core 语义步骤 TransactionStep::InsertTableRow { table, index }（与
  InsertTable 同理：整行构造一次成型，列数取自表首行；step map 报告实际新行，
  Runtime 解析该子树首个 inline 后代作为 caret 目标），单 isolated history entry，redo 恢复
  同一批 node id
Atomic 焦点（cell 内 HR/Image）同样导航；atomic-only 目标格落在首个 atomic 节点；普通 Gap 焦点不导航
Shift+Tab 反向；第一个 cell 上 no-op
GPUI keybinding 上下文优先级：table cell > list item（Tab 缩进）> paragraph（Tab 变列表）
```

### Enter / Backspace / Delete（P5.2）

```text
Enter 在 cell 内段落 → 既有 SplitBlock（新段落留在同 cell）
Backspace 在 cell 首段 (0,0) → baseline no-op（不 join 前 cell；cell join 的
  atom/嵌套迁移语义明确后另立切片，本切片记录为已知边界）
Delete 在 cell 末尾 → 同理 no-op
cell 内文本/选区/IME/undo 全部走既有 intent，无表格特例
```

### 行列操作（P5.3，实施修订）

```text
InsertTableRow { table, index }      Core 语义步骤（修订：单一新行使表在命令中途
                                     非法，validated staging 无法表达，与 P5.1 同理）
InsertTableColumn { table, index }   Core 语义步骤：每行 index 处插一个空 cell（空
                                     Paragraph），每行各发出一个 NodeInserted map，
                                     inserted 指向该行新 cell，不伪装成 paragraph
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
wire v5 / v6：
  ClipboardNodeContent::Table { rows, row_attrs }；table attrs 位于节点本身
  无行属性继续写 v5（省略 row_attrs）；有非空行属性写 v6，逐行保留 attrs
  reader 接受 v4/v5/v6；旧信封夹带新特性 fail soft，避免旧 reader 静默丢 attrs
  cell 载荷沿用 ClipboardNodeContent::Children / Inline / Atomic
旧版本 fail-soft：v4 及以下 reader 遇 table 载荷退化为 plain text（不静默重组结构）
plain-text fallback：TSV——cell 内文本以 \t 分列、\n 分行；cell 内已有换行按 block 边界扁平化
paste（P5.5 实施事实，`session/paste_table.rs`）：
  表载荷 + 匹配尺寸的 cell-range → 逐 cell staged 替换（先插后删，单 history entry）
    替换内容和 cell attrs，目标 table/row attrs 保持；非表 fragment 填充各 cell 内容
  1×1 表载荷 + caret 在 cell 内 → payload 块插入 focused block 之后
  表载荷 + caret 在普通文本 → InsertTable 语义步骤建空表 + 逐 cell 填充 + seed 段删除
    完整保留 table/row/cell/block attrs、marks、inline atoms、容器、atomic 与嵌套表
  mixed / 尺寸不符 / 其余落点 fail closed（新错误变体 ClipboardTableUnsupported）
markdown：GFM table 不入 P4.9 baseline codec；Table 节点导出走既有 UnsupportedNodeKind
```

## 5. GPUI（P5.4）

```text
TableBlockPresentation：
  grid 布局（gpui div + 固定列计数），cell 边框 / hover / focus affordance
  cell 内块渲染递归复用既有 block element（不做表格专用第二套段落渲染）
  caret / selection / IME：复用普通块；hit-test 必须区分同高度的不同列
  表格级 hit-test：点击 cell 空白 → caret 到该 cell 首段；点击既有块 → 既有路径
keyboard：Tab / Shift+Tab action 在 cell 上下文注册；上下文判定依据 focus 所在块的祖先链
Ctrl/Cmd+Shift+Space 选中当前格；Shift+方向键扩展矩形；普通方向键/Escape 退出矩形
格左上角选择柄可拖动矩形，Shift+点击选择柄扩展；外层矩形命中嵌套格时归一到外层 cell
矩形使用前端空 ParagraphView 输入代理（不创建 canonical 假节点），复用 UTF-16/IME 投影
  预编辑/取消不改文档，commit 经 Runtime 一次事务替换矩形，撤销恢复矩形并重新聚焦代理
Up/Down 保持窗口坐标 desired-x，优先同格视觉行，再同列相邻行，最后离开表格
  嵌套表先回到所属外层 cell；atomic-only 格没有文字视觉行，Up/Down 跳过、Tab 可访问
等宽列必须 min-width:0，避免内容固有宽度把列撑开；wrapped layout 仍属于普通块
accessibility：Table/TableRow/TableCell 投影为对应 role，cell 内容递归投影
```

## 6. Fixture / codec

```text
harness fixture v5：table 块行编码
  table / row / cell 标签 + end（复用 quote/ul 的容器栈；不冗余存储 rows/cols）
  各层可带 @ attrs，cell 内沿用 p/code/atom/img/quote/list/table 行
  reader 接受 v2/v3/v4/v5；低版本信封携带 table 标签 fail closed；writer 统一写 v5
  列数一致、非空表/行/cell 等仍由 Core 最终验证；scalar attrs 无损，list/object attrs 明确拒绝保存
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
