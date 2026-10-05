# 有界表格剪贴板导出契约

2026-10-05，Clipped Copy引擎重建：Runtime541、全库1278、strict Clippy/fmt/source/dependency门禁通过；产品接入和真实GUI另行验收。

## 宿主选择

`SessionPolicy::clipboard_export_spec(context, purpose)` 是唯一只读导出入口。
`purpose` 明确区分 Copy 与 Cut；默认 `None` 保持历史 unit-cell、TSV 和 v4–v13 行为。
宿主可独立选择闭合逻辑矩形复制、精确源矩形裁切，以及 `TextBetweenLfV1` 文本投影。
后者也适用于普通全篇复制，不局限于 CellRange。
投影描述是可重算规则，不接收宿主自报的任意文本或摘要。

投影-only opt-in CellRange Cut仍在克隆、投影、平台写入之前明确拒绝。
后继显式`prepare_cut`策略可提供完整删除计划，Runtime返回独占原session的
`PreparedCut`；完整候选/Core/选择/policy/inverse/redo及lossless item预检全部通过后，
才一次平台写入并发布同一个candidate。不能先覆盖剪贴板再调用可能拒绝的Delete。
默认legacy路径保持兼容，不把专用路径保证扩大到所有Cut；OS writer无确认、crash
与外部ownership边界明确保留，详见[prepared Cut契约](prepared-table-cut.md)。

## 结构和来源

闭合逻辑矩形保留每个实际行、row attrs、Header/body、span/colwidth、
covered empty row，以及 cell 内文本、marks、typed HardBreak、图片和嵌套表格。
每个 cell origin 仅捕获一次，不展开 unit skeleton，不复制 covered slot。
`with_closed_cell_ranges()`仍拒绝跨边界span，不会自动扩选或裁切；原18个
Copy oracle的11闭合成功/7非闭合拒绝继续作为该模式的兼容契约。

`with_clipped_cell_ranges(empty_paragraph_attrs)`显式启用源矩形裁切。宿主只提供
其空Paragraph默认attrs，不提供任意文本、子树或callback；例如产品默认值
textAlign:null与indent:false由宿主传入，不硬编码进Runtime。
`ClipboardExportSpec`改为Clone而非Copy，内部Arc共享不可变attrs，避免在预算前
深拷贝。geometry builder替换整个模式并释放不用的attrs；closed builder不再const，
`text_projection()`改借用self。其余0.x API变化均在rustdoc记录。

裁切不扩张endpoint完整cell形成的bbox。每个相交物理origin仅捕获一次，包括从
上方/左侧进入的origin，再按裁后(row,column)排序。上/左进入者清除全部富子树，
替换为一个带宿主默认attrs的空Paragraph；只裁右/底边者保留全部原rich forest。
Header/body和未知cell attrs保留，只重写确实变化的span；横裁colwidth取对应片段，
若其中无正值则变null，纯纵裁保持原全0数组。所选原物理行的attrs与covered空行
继续保留，不随移入cell携带其原origin行metadata。PM tableRow的空schema不能单独
证明这些非空Runtime metadata，需原生专测。

新模式先对完整借用源、roots frame做资源预检，再用有界仅含ID/geometry的临时表
确定裁切。克隆attrs/forest之前按clear数量checked_mul/add保留完整生成空Paragraph
的节点、结构和depth4 attrs预算；不扣除被替换forest，边界因此有意保守。count0
不遍历无用默认attrs。Closed模式预算不改，后续roots/wire验证仍独立。
Copy不消耗源或目标文档allocator、不把ID写进DTO/wire；已有完整结构验证仍使用
临时NodeStoreBuilder本地ID，不能宣称整个Copy调用链完全不创建NodeId。
裁后carrier可编码不替代宿主对原始完整文档的准入，未知内容不能先清除再放行。

`source_boundary` 与 `text_projection` 分别保存和验证：

- WholeRoots：显式完整 block 来源，closed=true，open depth 为 0/0
- CellRange：closed=false，固定 open depth 1/1，区分实际 Rows fragment 与 Table root
- 普通 open 来源：不宣称未知的 ProseMirror open depth，不推断成 WholeRoots

CellRange 内部仍以一个有效 Table DTO 承载实际 origin rows；Rows 标记说明
该外层 Table 仅为传输容器。全表 CellRange 也不等于显式 All。
解码要求一个完整有效的 Table carrier；错误 closed/depth/root 组合、额外 roots
以及历史版本携带新字段均拒绝。

`ClipboardSlice::allows_default_fitting` 集中处理消费端来源准入：只有历史
open 切片和普通 Open 来源可以进入原有默认拟合。WholeRoots 沿原 closed
门禁；CellRange 的 Rows/Table 两种形式都要求显式宿主处理，默认返回
`UnsupportedTableOperation`。该检查先于默认 marks/history/cell-range 收敛，
GPUI 默认 Code 文本降级也使用同一检查；显式 host Apply 或 Code router
仍可验证完整来源后接管。Paste 拒绝不会清空已有剪贴板。

## 可重算平台文本

`TextBetweenLfV1` 对应已测宿主 `Fragment.textBetween(0, size, '\n', '\n')`：
textblock 之间用 LF；空 Paragraph 参与分隔；空 TableRow 不产生文本；
Image/HorizontalRule 作为 block leaf 产生 LF；HardBreak 产生 LF。
文字本身的 CRLF、tab 和 Unicode 原样保留，不拼图片 URL/alt。
未知 Custom/InlineAtom 的 schema 语义不能推测，整次投影拒绝。

所有新投影先借用预扫预算，再克隆子树或扩展字符串；有界数量、嵌套深度、
attrs/marks 和输出大小同时受控。不能为每个节点反复全篇验证。

## v14 传输和失败边界

新 envelope 使用固定前缀 `xiaomu.clipboard.v14\n` 加严格 JSON body。
该前缀先于解析识别；匹配后无论未知/重复/转义重复 version、坏 JSON、
超预算或字段不合法，都属于 `RejectedNative`，不得回退 Text 或 Image。
body 必须通过有界 duplicate-key 检查；不得先用 JSON last-wins 决定版本。
历史无前缀 v4–v13 与 foreign 内容保持原有读取/fallback 边界。

解码从已验证结构与投影规范重新计算文本，与平台原始文本按字节相等时才成功。
编码也先验证，并要求完整 encode/decode roundtrip（包括来源与投影规范）。
新 projected Copy 和所有 Cut 在失败时不替换旧剪贴板。
旧未 opt-in 的 open Copy 仍保留既有普通文本降级行为。

## 不属于本阶段

复制不会开启 CellRange Paste。后续同尺寸闭合矩形替换需要 Core 语义步骤、
真实 fresh IDs、精确选择映射和单 history Undo/Redo；重复铺排、grow、
非闭合目标拟合、caret paste、外部 HTML 与未知 attrs 准入分别验收。
宿主工厂 Node oracle、Rust 测试和真实产品 GUI 是独立证据。
# Exact rectangle replacement foundation

`TransactionStep::ReplaceTableRect { table, rect, tree }` replaces a closed
destination rectangle with the same logical dimensions from a validated
`TableTreeTemplate`. It keeps destination Table/row IDs and attributes plus all
outside cell forests. The template's outer Table and direct row wrappers are
not allocated; every incoming cell descendant, nested Table/row and inline atom
receives a fresh identity. `TableTreeTemplate::node_count()` still reports all
captured nodes, not this operation's smaller fresh-ID count.

The final aggregate table budget subtracts removed nested grids before charging
incoming nested grids. The entire identity range is checked before allocation.
An exact opaque `TableCellRestore` inverse includes rich descendants, position
maps and expected parent edges through the destination table. Stale payloads,
reparented rows/descendants, occupied identities and later transaction failure
are rejected without publishing a changed snapshot or consuming allocator IDs.

This is a semantic Core transaction, not automatic clipboard fitting. Runtime's
default native CellRange-carrier gate remains closed. A product policy must
explicitly validate provenance, styles, width-repair requirements and its final
selection before publishing an `Apply` plan. Core does not guess ProseMirror
selection, normalize widths, repeat/clip source cells, grow a destination table
or copy source wrapper attributes onto destination rows.

The foundation passed 13 focused rectangle tests, the seven existing TableTree
tests, all310 Core and511 Runtime tests, strict Core Clippy and source/dependency
guards. Consumer virtual views additionally check copy/paste, exact Undo/Redo
and isolated SQLite readback; that is not native desktop rectangle acceptance.
