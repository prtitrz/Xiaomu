# P5 Table 总览

> 状态：**IN PROGRESS（2026-09-05 启动）**
>
> P4：**CLOSED（2026-09-05）**

P5 让晓木跨过"表格"边界：canonical 表格模型、单元格内编辑、Tab 导航、行/列结构操作、单元格选区与 clipboard。P0-P4 已把 transaction / mapping / history / structured clipboard / capability seam 全部铺平，P5 的要求是**全部复用这些机制**，不引入第二套坐标或历史系统。

## 执行顺序

```text
P4 structured content
        ↓
P5.1 Table Canonical Model
        ↓
P5.2 Runtime Cell Editing
        ↓
P5.3 Row / Column Operations
        ↓
P5.4 GPUI Table Rendering
        ↓
P5.5 Cell Selection / Clipboard
        ↓
P5.6 Integration Gate / P5 Closeout
        ↓
P6 Performance / Long Document
```

## 分片目标

### P5.1 Table Canonical Model

```text
NodeKind::Table / TableRow / TableCell
content-shape / child-kind validation
row cell-count uniformity invariant（validate_tree）
cell 最少一个 child block 不变量
builder / 事务可构造性（InsertNode 组合即可，不加新 Core step）
P0-P4 regression
```

Gate：canonical builder 可构造合法表格；非法形状（行列不齐、空 cell、错误嵌套）fail closed；所有既有测试保持绿。

### P5.2 Runtime Cell Editing

```text
EditIntent::MoveToNextCell / MoveToPreviousCell（Tab / Shift+Tab）
last-cell Tab 追加新行、first-cell Shift+Tab no-op
cell 内 Enter = 既有 SplitBlock（cell 是普通容器）
cell 起点 Backspace baseline no-op（cell join 留待后续，明确记录）
cell 内 typing history coalescing / IME / stored marks 全部复用
undo / redo 精确
```

Gate：表格中英文连续编辑 + undo/redo 精确恢复；Tab/Shift+Tab 在 cell 序列上稳定行走。

### P5.3 Row / Column Operations

```text
EditIntent::{InsertTableRow, DeleteTableRow, InsertTableColumn, DeleteTableColumn}
staged multi-step transaction（InsertNode / RemoveNode 组合，整命令一个 history entry）
selection mapping：被删 cell 内的 caret/selection 收敛
边界 fail closed：删最后一行不动 table（显式报错）；结构 op 与 atom / image 载荷组合安全
undo / redo
```

Gate：行列插入删除全程 caret/selection 可预测，undo/redo 精确，row/col op 与 cell 内 atom 共存无损。

### P5.4 GPUI Table Rendering

```text
table grid layout / cell borders / focus affordance
cell 内 block 渲染复用（Paragraph 等 element 递归使用）
caret / selection / IME / hit-test 在 cell 内复用既有 projection
Tab / Shift+Tab keybinding（cell 上下文优先于段落 Tab 变列表）
click → cell → caret；e2e 测试
```

Gate：`text ↔ table ↔ text` 鼠标键盘行为稳定；cell 内编辑视觉与普通块一致。

### P5.5 Cell Selection / Clipboard

```text
cell-range selection（同表内 anchor/focus cell 矩形）
selection variant + mapping + validate
structured clipboard table 载荷（wire v5）+ TSV plain-text fallback
paste 单 cell / 多 cell；mixed fail closed 沿用 P4 结论
markdown：GFM table 不入 baseline codec（导出 fail closed，明确记录）
```

Gate：cell 矩形选区 copy/paste/undo 无损；TSV fallback 语义正确。

### P5.6 Integration Gate / P5 Closeout

```text
realistic table fixture（harness fixture v5 编码 Table）
Unicode + cell + atom matrix
multi-editor isolation
architecture / planning / progress final sync
Windows real-machine Gate + 三平台 CI Success
```

Gate 全部通过才允许 **P5 = CLOSED** 并进入 P6。

## 核心约束

- `TextOffset` 仍只表示 cell 内 inline text 的 UTF-8 byte offset；表格结构永远不进入文本坐标；
- cell 是普通 block 容器：cell 内的 caret / selection / gap / atomic 语义全部沿用既有模型，不新增坐标种类；
- 行列结构操作收敛为 staged transaction（一个 history entry），中间 snapshot 不可见；
- 表格结构不变量（行列一致、cell 非空）属于 canonical validation，不属于前端约定；
- GPUI 只做 layout / paint / hit-test / keybinding，不含表格语义；
- clipboard wire 升级必须保持旧版本 fail-soft 行为（v4 及以下遇到 table 载荷退化为 plain text 或 fail closed，不静默重组）；
- markdown baseline codec 不为 table 破坏 P4.9 的 refuse-instead-of-drop 契约。

统一进度见 [`progress.md`](./progress.md)，模型与语义细节见 [`design.md`](./design.md)。
