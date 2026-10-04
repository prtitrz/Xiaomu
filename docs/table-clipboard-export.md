# 有界表格剪贴板导出契约

2026-10-04，实施中；编译、测试与产品接入证据由合并验收补充。

## 宿主选择

`SessionPolicy::clipboard_export_spec(context, purpose)` 是唯一只读导出入口。
`purpose` 明确区分 Copy 与 Cut；默认 `None` 保持历史 unit-cell、TSV 和 v4–v13 行为。
宿主可独立选择闭合逻辑矩形复制，以及 `TextBetweenLfV1` 文本投影。
后者也适用于普通全篇复制，不局限于 CellRange。
投影描述是可重算规则，不接收宿主自报的任意文本或摘要。

本阶段 opt-in CellRange Cut 在克隆、投影、平台写入之前明确拒绝。
不能先覆盖剪贴板，再依靠不支持的 Delete 保留源文档。
实际 GPUI Cut 回归须同时证明原剪贴板（含 metadata）、文档、选择和历史未变。
完整 Cut 原子化与 revision/selection 绑定的 prepared delete 留待后续。

## 结构和来源

闭合逻辑矩形保留每个实际行、row attrs、Header/body、span/colwidth、
covered empty row，以及 cell 内文本、marks、typed HardBreak、图片和嵌套表格。
每个 cell origin 仅捕获一次，不展开 unit skeleton，不复制 covered slot。
跨边界 span 的非闭合矩形目前拒绝；不能静默扩选、裁剪或 flatten。
真实工厂的 18 个 Copy oracle 中先实现 11 个几何闭合案例；7 个非闭合案例
用于保持明确拒绝，后续 clipping 必须以原工厂结果实现。

`source_boundary` 与 `text_projection` 分别保存和验证：

- WholeRoots：显式完整 block 来源，closed=true，open depth 为 0/0
- CellRange：closed=false，固定 open depth 1/1，区分实际 Rows fragment 与 Table root
- 普通 open 来源：不宣称未知的 ProseMirror open depth，不推断成 WholeRoots

CellRange 内部仍以一个有效 Table DTO 承载实际 origin rows；Rows 标记说明
该外层 Table 仅为传输容器。全表 CellRange 也不等于显式 All。
解码要求一个完整有效的 Table carrier；错误 closed/depth/root 组合、额外 roots
以及历史版本携带新字段均拒绝。

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
非闭合 clipping、caret paste、外部 HTML 与未知 attrs 准入分别验收。
宿主工厂 Node oracle、Rust 测试和真实产品 GUI 是独立证据。
