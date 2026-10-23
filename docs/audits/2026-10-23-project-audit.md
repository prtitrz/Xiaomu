# Xiaomu 规划完成度与代码审计

日期：2026-10-23（Asia/Shanghai）
审计基线：`f0746e7f54cd70b3e16cf1c2684eb1808496df4f`（P5.5 / PR #84），加当前工作区改动。
工作区原有改动：`examples/editor_harness/src/store/format.rs`、`examples/editor_harness/src/store/mod.rs`。本次审计未修改这两处实现，也未修复生产代码。


> 下文保存首次审计的基线发现与当时验证结果。用户授权后的修复及最新证据见文末“修复跟进”，原始失败记录不代表当前工作区仍失败。

## 结论

**尚未按完整规划完成，也不建议现在宣告表格阶段验收通过或进入稳定发布。**

核心架构和基础编辑能力已有实质实现，Core / Runtime / Markdown / Testkit 的现有 378 项测试在本机通过；依赖方向检查通过。当前最需要补的是 P5 表格的跨层正确性和验收，而不是重做底层架构。

P5.1–P5.5 虽被标为 CLOSED，但本次发现鼠标命中、结构位置映射、剪切、剪贴板保真等问题。P5.6 本来就未关闭。P6 的长文档性能与虚拟化、P7 的库稳定化尚未完成。不能用“阶段数完成比例”代替可发布程度。

## 规划对照

| 阶段 | 文档状态 | 审计判断 |
| --- | --- | --- |
| P0 Core Contract | CLOSED | 文档模型、事务、映射、逆操作等已有实现和测试；此次发现新表格步骤的错误参数处理及映射缺陷，需补回归。 |
| P1 Single Block Input | CLOSED | 输入、selection、composition 等基础路径已存在；本次未重新进行真实输入法实机验证。 |
| P2 Document Tree | CLOSED | 树编辑、列表、多块、宿主存储接口等已有实现。 |
| P3 Cross-block / History | CLOSED | 跨块编辑、历史分组、soft wrap、多编辑器基础集成等已有实现；历史文档有 Windows 手工验收记录。 |
| P4 Atom / Media / Extension | 分阶段文档 CLOSED；顶层规划仍 IN PROGRESS | Atom、Image、AssetService、Markdown baseline 已实现；规划中 BlockRendererRegistry、LinkOpenService/链接激活闭环仍未见对应实现。需补交付或明确延期。Windows 实机验收证据口径有问题。 |
| P5 Table | P5.1–P5.5 CLOSED；P5.6 CURRENT/PENDING | 模型、导航、行列操作、矩形选区接口和表格剪贴板已实现，但存在以下确定缺陷，不能视为完成。工作区的 fixture v5 仍在收尾。 |
| P6 Performance | 路线目标 | 已有早期布局缓存；未见完整 windowing/virtualization、长文档基准与性能验收。 |
| P7 Stabilization | 路线目标 | 当前仍为 0.1.0 开发状态；公开 API、兼容策略、发布流程及完整接入文档尚未形成阶段收口。 |

依据：[顶层规划](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/planning.md#L975)、[P5 进度与未完成 Gate](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/phases/p5-table/progress.md#L60)。顶层规划中 P5–P7 的“完成：”是目标列表，不能作为实际完成记录。

## 审计发现

优先级：P1 = 应在下一次集成/表格验收前修复；P2 = 应在阶段收口或对外稳定使用前解决。以下前七项涉及已提交实现，第八项来自当前未提交改动。

### A1 · P1 · 表格鼠标定位没有横坐标参与，右侧单元格可能命中左侧

位置：[mouse.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-gpui/src/document_view/mouse.rs#L126)。

`hit_test` 先遍历所有文本块，只计算点击位置与块的纵向距离。同一行两个单元格的文本块距离都为零，且比较条件为严格小于，先遍历到的左侧块会一直保留。后续才在该块内处理横坐标，已经无法纠正“选错块”。这会让光标、后续输入与拖选落到错误的单元格。

证据类型：静态代码确认。本次没有完成原生点击交互复现。现有 `table_gpui.rs` 的点击用例只点第一格，不能覆盖此问题。

建议：优先选择二维 bounds 包含点击点的文本块；空白区先定位所属 cell，再在该 cell 内选择文本落点；同距时使用横向距离。覆盖每行的首列、中间列、末列、不同换行高度和 cell padding 点击。

### A2 · P1 · 插入列只产生第一行的映射，后续行结构位置不移动

位置：[table.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-core/src/transaction/apply/table.rs#L191)。

`InsertTableColumn` 修改了每一行的 children，却只返回 `parent: first_row` 的一个 `NodeInserted`。`map_node_gap` 因 parent 不同而原样返回其余行的 gap。

已运行复现：2×2 表格，第二行原来的 `NodeGap(row2, 1)`；在第 0 列插入后，预期为 `NodeGap(row2, 2)`，实际仍为 `NodeGap(row2, 1)`。稳定 NodeId 能保护文本位置，但不能代替 row-local gap 的结构映射。

建议：让一个语义步骤能够产生覆盖所有受影响行的映射，或引入完整的列插入映射语义。不要在 Runtime/前端另写 offset 修补。测试所有行的 gap、矩形选区停靠位置、外部 anchor 及 inverse/undo/redo。

### A3 · P1 · 矩形选区剪切只删除左上角一个字符

位置：[actions.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-gpui/src/document_view/actions.rs#L407)、[session/mod.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/session/mod.rs#L138)。

前提：宿主通过已公开的 `set_cell_range_selection` 安装矩形选区。`cut` 先复制整个矩形，再调用普通 `Delete`；`apply_intent` 却先把矩形折叠到 anchor cell 首段起点，然后执行单字符删除。

已运行与 `DocumentView::cut` 相同的 Runtime 操作序列：原 2×2 内容为 `[a1, b1; a2, b2]`，复制得到完整 TSV，但 Delete 后内容为 `[1, b1; a2, b2]`。被复制的范围与被删除的范围完全不一致。

建议：定义显式的 cell-range clear/cut 语义，在一个事务中清空被选 cell 内容并保留合法表结构，undo 恢复原选区和内容。若暂不支持，必须在写剪贴板/修改文档之前明确拒绝，不能降级成删一个字符。

### A4 · P1 · 表格复制粘贴会静默丢失结构属性

位置：[projection.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/clipboard/projection.rs#L183)、[paste_table.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/session/paste_table.rs#L201)。

已运行复现：源 cell 带 `extension-data = "retain-me"`。复制后的 clipboard cell 仍携带属性；粘贴为新表格成功后，新 cell 的 attrs 变成空集。新表通过 `InsertTable` 创建空 attrs 的结构，后续仅填充内部 block，没有恢复 cell attrs。

同一链路还存在静态可见问题：投影创建 table fragment 时使用 `NodeAttrs::empty()`，row wrapper 在 wire 中被省略，因此 table/row 的未知属性也无法完整保留。此处不是“显式拒绝不支持内容”，而是成功返回后丢信息，违反 preservation-first 原则。

建议：分别明确“新表复制”和“替换已有 cell 内容”的属性策略；前者完整保留 table/row/cell attrs，后者按明确契约保留目标样式或应用源样式。对 wire 结构作必要扩展；无法表达时先拒绝，避免静默丢弃。

### A5 · P2 · 错误表格事务能导致 Core panic

位置：[table.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-core/src/transaction/apply/table.rs#L90)、[越界点](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-core/src/transaction/apply/table.rs#L138)。

已运行复现：合法文档 `Document > Quote([])`，对 root 调用 `TransactionStep::InsertTableRow { table: root, index: 0 }`。实现未先验证目标 kind，推导出 columns=0 后访问 `paragraphs[0]`，出现 `index out of bounds`。宿主直接使用 Core transaction 或 Runtime 的公开 apply 接口可触发；正常 Runtime 表格 intent 有前置检查，不能替代 Core 的防线。

建议：在 Core 步骤入口验证 Table/TableRow 身份、非零尺寸及相关边界；可拒绝输入返回 `Error`，不依赖最终整树验证或直接数组索引。补错误节点种类和多步骤中间态用例。

### A6 · P2 · 合法的富内容 cell 无法复制

位置：[projection.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/clipboard/projection.rs#L203)、[paste_table.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/session/paste_table.rs#L311)。

已运行复现：合法的 `TableCell > Quote > Paragraph("inside")`，安装单格矩形选区后 `clipboard_slice()` 返回错误。`cell_fragment` 要求每个直接 child 都是 Inline。列表、Quote、Image/HR、嵌套表格同样无法走这条捕获路径。表格设计和渲染允许 richer blocks，但剪贴板只处理直接文本块。

建议：复用递归 ClipboardNode 捕获/粘贴逻辑，明确嵌套表边界，补 quote/list/image/atom 混合矩阵。若要收窄 P5 baseline，必须同步限定模型或公开能力并说明，而不能继续声称所有合法 cell 都可无损复制。

### A7 · P2 · TSV fallback 没有处理 cell 内换行，外部粘贴会改变行列数

位置：[fragment.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-runtime/src/clipboard/fragment.rs#L380)。

已运行复现：1×1 表格，cell 文本为 `line1\nline2`。plain-text fallback 原样得到 `line1\nline2`，没有按 cell 内换行扁平化；普通 TSV 消费者会解释为两行。CodeBlock 的换行/制表符也会与 TSV 分隔符冲突。现有实现只把多个 block 之间的边界换为空格。

建议：选定可互操作的字段转义规则，或按照当前 baseline 明确把 cell 内 LF/CR/tab 扁平化。测试硬换行、代码、tab、atom fallback，并确保 metadata 校验使用同一规则。

### A8 · P2 · 当前未提交 fixture v5 改动未通过已有门禁

位置：[store/mod.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/examples/editor_harness/src/store/mod.rs#L391)、[store/format.rs](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/examples/editor_harness/src/store/format.rs#L46)。

`demo_fixture()` 新增了表格，但原有 `fixture_v4_round_trips_inline_atom_chips_and_media` 仍用 v4 writer 保存这个 fixture。运行原测试得到：`fixture format does not encode node kind Table; refusing to save a lossy snapshot`。存储模块五个现有测试中四过一败。`cargo fmt --all -- --check` 也在新增代码处失败。另对真实存储模块进行非测试编译并将 unused-imports 设为错误，确认 `store/mod.rs:24` 的 `use format::write_node` 已成为 unused import，会被项目的 `-D warnings` 门禁拒绝。

建议：保留独立、明确的 v2/v3/v4 兼容样例；增加 v5 table round-trip、尺寸校验、坏输入拒绝、嵌套 cell 内容与未知 attrs 的测试；让生产 save/load 路径也参与测试，再格式化。不能仅把旧测试的版本号改成 v5 而丢掉旧版本覆盖。

## 规划与验收还缺什么

1. **原生矩形选区入口尚未接上。** 仓库内 `set_cell_range_selection` 的调用来自 Runtime 测试，GPUI 只消费矩形高亮；鼠标拖选仍安装 inline selection。应明确提供鼠标/键盘建立 cell range 的路径，或标明它目前只是宿主 API 能力，并增加真正从输入事件走到 copy/paste 的端到端用例。
2. **表格 accessibility role 未实现。** [设计](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/phases/p5-table/design.md#L136)要求 Table/TableRow/TableCell 对应角色，但 [role_for_kind](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-gpui/src/accessibility.rs#L195)仍把它们投影为 Generic。平台 screen reader adapter 可以按 P7 推进，但已有 frontend-neutral projection 的表格语义应在 P5 补上。
3. **P4 的验收证据被错误等同。** [P4 进度第 272 行](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/phases/p4-structured-content-extension/progress.md#L272)把 windows-latest CI 测试记为“Windows final real-machine Gate”。这只证明自动化环境的编译/测试通过，不能证明 Microsoft Pinyin 候选窗、连续 composition、焦点恢复和取消/提交行为。需分开保存 CI 与人工实机记录，记录 commit、OS/IME 版本、测试矩阵与结果。此审计并不否定既有 P1/P3 的历史手工记录。
4. **文档状态与范围漂移。** [planning.md:7](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/planning.md#L7)仍停在 P4.2；分阶段文档已进入 P5.6；README 仍写 bootstrap。P4 的 BlockRendererRegistry、旧路线审计要求的 [LinkOpenService/链接闭环](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/docs/roadmap-gap-review.md#L116)未见落地。应建立明确的“已交付/延期/取消”清单和唯一进度入口；不要只把状态改成 CLOSED。

已明示的 baseline 限制不当作新 bug：Markdown 拒绝导出表格和未知扩展、最后一个 cell 上 Tab 加行、首格 Shift+Tab no-op、cell 边界不跨格 join 等。需要的是在宿主接入文档中让能力边界可见。

## 验证记录与局限

| 验证 | 本次结果 |
| --- | --- |
| Core / Runtime / Markdown / Testkit，`cargo test ... --all-targets` | **378 passed，0 failed**，39 个测试结果分组。 |
| 同四个 crate，Clippy `-D warnings` | **通过**。 |
| 依赖边界检查 | **通过**。 |
| 源文件规模检查 | **通过**；15 个文件超过 500 行的警告，未超过 700 行硬门禁。 |
| 全工作区 fmt check | **失败**，位于原有未提交的 fixture 改动。 |
| 在临时独立测试项目中运行原存储模块测试 | **4 passed，1 failed**，见 A8。 |
| 原存储模块非测试编译，`-D unused-imports` | **失败**，`store/mod.rs:24` 的 `format::write_node` 导入未使用。 |
| 定向审计测试 | 原 P5.5 的 13 个测试通过；额外 6 个预期正确行为断言全部失败，分别确认 A2–A7。 |
| 已提交 HEAD 的 GitHub checks | **CI Success、Ubuntu/macOS/Windows、policy 全绿**：[对应运行](https://github.com/prtitrz/Xiaomu/actions/runs/34683169227)。不覆盖工作区未提交改动。 |
| 本机整个 workspace / GPUI 测试 | 尝试运行；依赖下载出现 static.crates.io SSL 连接错误，审计结束时停止仍处于依赖下载阶段的进程，未取得完整本机通过结果。A1 仅作静态结论，未宣称完成原生交互验证。 |
| 真实输入法与跨平台人工 Gate | 本次未执行；不能用上述单元测试或 CI 替代。 |

定向复现使用仓库实际 Core/Runtime 源码及公开 API，在 `/tmp/xiaomu-audit-20261023` 编译；未改动现有测试或业务实现。审计覆盖重点是规划兑现、结构模型/事务/映射、P5 跨层行为、剪贴板、存储及验收证据，不能据此声称其他路径没有缺陷。

## 后续建议与顺序

### 1. 先完成一个 P5 正确性修复批次

按“写错位置/错误剪切/丢属性 → 映射 → panic/富内容/TSV → fixture”处理 A1–A8，每个修复都保留能先失败后通过的回归测试。映射问题必须在 Core 修复，剪切与粘贴语义由 Runtime 持有，二维命中留在 GPUI。

这一批不建议同时加入合并单元格、复杂列宽、协作或更多编辑器插件。

### 2. 用真正的集成矩阵关闭 P5.6

- 多行多列，逐格点击、键盘导航、选区建立、整矩形 copy/cut/paste/undo/redo。
- CJK、emoji、combining marks、BiDi、HardBreak、CodeBlock、相邻 atoms、image/list/quote 混合内容。
- 所有行的 gap 与矩形端点经过行列插删、undo/redo 后保持正确。
- v5 存储 round-trip，v2/v3/v4 向后读取，未知属性保留，非法文档明确拒绝。
- 两个编辑器并存、不同焦点、独立历史与 clipboard 回读。
- 三平台 CI 加独立的 Windows IME 实机记录；修复当前工作区 fmt/test 失败后，针对最终 commit 重跑。
- 同步 README、planning、architecture 和 phase progress；对 P4 遗留能力明确归属。

### 3. P6 先测量，再做局部优化

建立 1k/10k 级 block 文档、较大表格、长 CodeBlock、多 editor 的基准，记录输入/粘贴/撤销耗时的分布、滚动帧耗时与内存/历史增长，选定目标硬件和预算。

从代码看，值得优先测量的热点是：

- [NodeStore](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-core/src/document/store.rs#L73)每次节点替换/插入都会克隆整个 BTreeMap；大表创建包含大量逐节点操作。
- [parent_of](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-core/src/document/snapshot.rs#L98)通过整树搜索找父节点，表格与选区路径会反复调用。
- [sync_children](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-gpui/src/document_view/mod.rs#L411)每次 render 收集全部文本块并同步所有 view，尚无 mounted window。
- [LayoutCacheKey](https://github.com/prtitrz/Xiaomu/blob/f0746e7f54cd70b3e16cf1c2684eb1808496df4f/crates/xiaomu-gpui/src/document_view/cache_key.rs#L14)依赖整个文档的 epoch；一次编辑可能让无关 block 缓存也失效。

这些是待测的复杂度/失效范围问题，本次未测性能，不能据此给出卡顿阈值。先建立数据，再决定父索引、事务内批量更新、块级 revision、windowing 的收益；不建议直接整体替换数据结构。

### 4. P7 面向一个真实宿主完成稳定化

收紧公开 API，明确支持的文档/clipboard/codec 版本与迁移规则；补接入示例、错误处理与能力矩阵。宿主侧实际验证加载、编辑、dirty/save、资产、链接、焦点、多实例、扩展和主题。生产存储应有完整写入、失败恢复和版本迁移策略，不能把 harness-private fixture 当作长期文件格式承诺。

最后再确定版本兼容承诺、发布自动化及平台 accessibility smoke tests。当前更适合作为受控试接的编辑器引擎，稳定发布还需要上述正确性与验收工作。


## 修复跟进（2026-10-23）

用户授权后按优先级修复 A1–A8，并补 P5.6 集成能力。原有 fixture 工作区改动继续保留并完成 v5 收尾。

| 项目 | 修复与回归证据 |
| --- | --- |
| A1 | cell bounds 限定子树 + 二维距离；原生右列、三列双行、不同行高和 padding 点击测试 |
| A2 | Core 每个受影响行发出 NodeInserted；所有插列位置/双 bias/全部 row gaps 与精确 inverse |
| A3 | 矩形 Delete/Backspace 清空全部所选 cell，保留结构与 attrs；原生 cut/paste、反向部分选区、undo/redo |
| A4 | table/row/cell attrs 全层捕获与新表重建；wire v6，v5 历史 body 验证读取；范围覆盖保留目标结构 attrs |
| A5 | 表格步骤入口验证 kind、非空行列和当前工作形状；错误 target 与多步骤 ragged 中间态测试 |
| A6 | quote/list/code/atoms/image/HR/nested table 递归捕获与 fresh-ID 重建；新表、范围与单格粘贴矩阵 |
| A7 | cell 内 CRLF/CR/LF/tab 扁平化为空格；内部结构化原文保真，v5 旧 fallback 向后读取 |
| A8 | demo table 含 Unicode、marked atom、quote 与 attrs；v5 实际 save/load/reopen；v2/v3/v4 兼容与坏输入拒绝；拆分 reader/tests 消除导入及规模问题 |
| 集成新发现 | 结构历史操作删除旧 ParagraphView 后键盘焦点丢失；同步 children 并恢复 focus，原生连续 undo/redo + 输入测试已覆盖 |
| 规划缺口 | 原生矩形键鼠入口与 Table/TableRow/TableCell roles 已补；P4 Windows CI 误记已更正；BlockRendererRegistry / LinkOpenService 明确延期至 P7 |

当前验证与最终验收入口：[P5.6 integration gate](../phases/p5-table/p5.6-integration-gate.md)。修复批次本地 **483 tests passed，0 failed**，Clippy/fmt/size/dependency/diff 门禁通过，详细结果以该记录为准；**最终修订的三平台 CI 和 Windows 人工 IME Gate 仍待完成，因此 P5 保持 IN PROGRESS**。不得用本文首次审计时旧 HEAD 的 CI 为修复批次背书。
