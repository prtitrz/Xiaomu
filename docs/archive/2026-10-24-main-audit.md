# 2026-10-24 main 规划与实现审计

审计日期：2026-10-24（Asia/Shanghai）。基线：[`7015166683536a58fe52ffdea7d4bf9e7214c305`](https://github.com/prtitrz/Xiaomu/commit/7015166683536a58fe52ffdea7d4bf9e7214c305)，已合并 PR #121。开始前更新远端，确认日常目录位于 main，HEAD 与 origin/main 一致、工作区干净。

本文是一次已完成审查的记录，**不是缺陷均已修复的声明，也不建立第二套执行规划**。本轮未修改生产代码或正式阶段状态；建议被接受后应进入 planning / 对应阶段契约。旧 P5.5 快照审计不作为当前缺陷依据。

> 2026-10-25 跟进：这些建议已获接受，修复与剩余项进入 [planning](../planning.md#delivery-boundaries) 和[维护进度](../maintenance/2026-10-main-audit.md)。下文保留 `7015166` 上的原始发现，不代表修复分支的当前状态。

## 结论与阶段对照

**P0–P5 的历史阶段验收成立；整个项目尚未完成。当前 main 已超过最初 P5 的能力范围，但仍有确定的 Markdown 保真缺陷、依赖通告待处置，以及 P6/P7 和新增宿主能力的验收缺口。**

| 范围 | 当前事实 | 本次判断 |
| --- | --- | --- |
| P0 Core | 不可变 snapshot、typed transaction、mapping/inverse、Unicode 边界、树验证；Core 无外部依赖 | 历史 Gate 成立；事务工作副本已采用一次分离、后续原地复用的 COW，旧审计的“每改一个节点都复制整张 map”不再适用 |
| P1 原生输入 | GPUI caret/selection、IME、clipboard、基础 marks | 历史 Gate 成立；不把旧实机结果扩写为所有后来增加能力的实机验收 |
| P2/P3 结构与历史 | 多块、列表、跨块选区、软换行、history、multi-editor、持久化接口与 Windows 记录 | 已完成对应范围；当前全量回归通过 |
| P4 扩展与媒体 | inline atom renderer/capability、atomic/image、资产接口、baseline Markdown | 历史阶段关闭，但本次发现 Markdown 的独立维护缺陷（A1–A3），须修复；不应重新声称“所有合法支持内容均已保真” |
| P5 表格 / P5.6 | 编辑、导航、行列操作、矩形选区、rich clipboard、fixture、Windows 原生 Gate；PR #121 补充回归 | 已完成；未发现旧 A1–A8 再次出现的证据，不重复实施已合入修复 |
| P5 之后的增量 | typed HardBreak、TaskList/checkbox、header/span 与逻辑表格操作、可选列宽拖动、framed clipboard、mixed font/首行缩进、只读输入、快照导入、reading、prepared Cut、policy finalization | 已有真实实现与自动化。默认开关、支持范围、宿主接入及原生验收需逐项区分，不能仅以“P5 CLOSED”概括 |
| P6 Performance | 已有局部缓存、COW、Taffy lazy min-content 等优化 | **尚未正式启动/验收**；没有完整 benchmark/profile、长文档预算及 virtualization 交付，局部优化不等于 P6 完成 |
| P7 Stabilization | 仍为 0.1.0；存在明确的未交付宿主契约 | **尚未启动**；公开 API/兼容策略/发布流程、通用 BlockRendererRegistry、LinkOpenService、平台无障碍 smoke 等仍需安排 |

阶段依据：[planning](../planning.md#16-roadmap)、[P5 progress](../phases/p5-table/progress.md)、[P5.6 追加验证](../phases/p5-table/p5.6-integration-gate.md)。COW 实现及回归位于 `crates/xiaomu-core/src/document/store.rs` 与 `store/cow_tests.rs`。

## 已确认的问题

优先级：P1 = 建议下一修复批次优先处理；P2 = 在扩大使用范围/进入相关验收前处理；P3 = 后续整理。这里的优先级不是 CVSS，也不表示已证明远程利用。

### A1 · P1 · Markdown 标题导出静默删除格式

位置：[标题写出](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/serialize.rs#L69)、[inline_text](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/serialize.rs#L450)。

已复现：导入 `## **bold**` 得到带 Bold 的 Heading，再导出返回成功但得到 `## bold`；重新导入后 Bold 消失。分别构造 Italic、Strike、Code、Underline 标题也都会丢 mark。该路径只拒绝 Link / TextStyle，却将其余 runs 直接拼成纯文本，甚至绕过段落路径对 Underline 的明确拒绝。

建议：标题按支持的 inline 语义保真输出；不能表达的 mark 明确拒绝。永久回归必须比较 node kind、attrs、文本和 marks，不能只断言输出字符串存在。

### A2 · P1 · Markdown 行内代码的空格与反引号处理不保真

位置：[render_run](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/serialize.rs#L370)、[code_span_fence](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/escape.rs#L59)、[读入空格规范化](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/inline.rs#L172)。

两个确定复现：

- 带 Code mark 的原文 `" a "` 导出为 `"`` a ``\n"`，再次读入变成 `"a"`，两端原始空格消失。
- 带 Code mark 的原文由反引号、字母 a、反引号这三个字符组成，导出为 `"```a```\n"`；重新读入报 code fence info 错误。写出成功却产出自身不能读取的文档。

建议：统一考虑 code span 的分隔符、padding 和行首 fenced-block 歧义；增加空格、纯反引号、首尾反引号、不同连续长度、段首/段中及相邻 mark 组合。任何暂不支持的组合应先明确拒绝，不能默默改写正文。

### A3 · P1 · Markdown 段落末尾 LF 丢失

位置：[paragraph_lines](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/serialize.rs#L332)、[parse_paragraph](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/parse.rs#L363)。

已复现：合法 Paragraph 原文 `"a\n"` 成功导出为 `"a\\\n\n"`，重新导入后变成 `"a"`。writer 特意允许最后一行为空，而 reader 到段尾/空行时没有保存最后的 pending hard break。段内 `"a\nb"` 的现有成功回归不能覆盖此例。

建议：若 baseline Markdown 无法精确表达段尾 LF，应返回 `HardBreakNotRepresentable` / `LossyWhitespace`；覆盖段尾、连续尾 LF、quote/list 内部和有 mark 的尾 LF。不要通过修改 canonical 文本来让导出成功。

### A4 · P1 · 依赖安全通告尚未处置，CI 总绿不等于 advisories 通过

位置：[CI 可失败步骤](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/.github/workflows/ci.yml#L27)、[Cargo.lock](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/Cargo.lock)。

main 的 [CI policy 日志](https://github.com/prtitrz/Xiaomu/actions/runs/39954510652/job/120416872441) 实际包含 7 项通告并输出 `advisories FAILED`。该步骤配置 `continue-on-error: true`，所以 policy / CI Success 仍成功，这是既有策略，不是 CI 漏跑。2026-10-24 本机刷新通告后 `cargo deny check advisories` 返回 1，共 **8 项**，新增 Tokio 通告。

| 当前锁定依赖 | 通告与已公布修复范围 |
| --- | --- |
| git2 0.20.4 | [0183](https://rustsec.org/advisories/RUSTSEC-2026-0183.html)、[0184](https://rustsec.org/advisories/RUSTSEC-2026-0184.html)：特定 Remote / Blame API 的不健全行为；修复于 >=0.21.0 |
| rustls 0.23.43 | [0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html)：修复于 >=0.23.45；[0346](https://rustsec.org/advisories/RUSTSEC-2026-0346.html)、[0348](https://rustsec.org/advisories/RUSTSEC-2026-0348.html)、[0349](https://rustsec.org/advisories/RUSTSEC-2026-0349.html)：修复于 >=0.23.46 |
| rustls-webpki 0.103.15 | [0382](https://rustsec.org/advisories/RUSTSEC-2026-0382.html)：特定已失信中间证书路径；当前稳定版本线修复于 >=0.103.16 |
| tokio 1.53.1 | [0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html)：Unix AsyncFd 构造器 I/O safety；当前版本线修复于 >=1.53.3 |

依赖路径已核对：git2 经 `gpui_util → gpui`；rustls / webpki / tokio 经 `zed-reqwest → gpui_http_client → gpui`。Xiaomu 自有 Core/Runtime 未直接引入这些库。

风险边界：**8 项是版本扫描命中，不是 8 个已复现的应用漏洞**。当前 workspace 全 feature / 全 target 解析中没有 rustls `brotli` feature，因此不能宣称当前构建触发 0348；0346 需要特定自定义 session-store Debug 日志，未在自有代码发现这种用法。其他项仍需结合宿主、传递依赖实际调用和平台评估，没有进行网络攻击验证。

建议：独立依赖维护批次优先检查兼容范围内的 rustls/webpki/tokio 更新并跑三平台；git2 修复跨越 0.20→0.21，而 gpui_util 0.2.2 声明 `git2 = "0.20.1"`，不能承诺仅更新锁文件即可消除。按项目 GPUI 固定版本策略评估上游升级或小范围审查补丁。对无法立即修复且证实不适用的项记录理由、责任与复查条件；不要把关闭整个 advisory 步骤当作处置结果。

### A5 · P2 · Markdown 深层嵌套缺少资源限制，可终止进程

位置：[递归 parse_quote](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/parse.rs#L218)、[from_markdown](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-codec-markdown/src/parse.rs#L23)。

在仓库外独立子进程运行 Rust 1.97.1 debug 构建：`"> ".repeat(8192) + "text\n"`，输入 16,389 bytes，调用 `from_markdown` 后发生 `stack overflow` / abort（进程状态 -6），不是返回 typed error。64、512、2048 层对照均成功。崩溃阈值受平台、栈大小和优化级别影响，不把 8192 当作通用阈值。

建议：进入递归前统一检查源字节、节点数、层深和工作预算；quote/list 共用预算，超限明确报错；对 exporter 的递归也检查。加入子进程防崩溃测试及语义往返/property/fuzz。普通字符串长度上限不能单独替代深度限制。本次未发现 Core 树验证同类问题：其遍历使用显式栈。

### A6 · P2 · 原生子视图同步存在可定位的二次复杂度

位置：[child_sync.rs](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-gpui/src/document_view/child_sync.rs#L18)、[每次 render 调用](https://github.com/prtitrz/Xiaomu/blob/7015166683536a58fe52ffdea7d4bf9e7214c305/crates/xiaomu-gpui/src/document_view/mod.rs#L655)。

`sync_children` 对所有文本节点逐项查找旧 Vec，并 `pool.remove(position)`。即使节点顺序完全没变，每次从头删除也会搬移余下元素，合计 O(n²)；默认视图还建立/更新全部段落 child，尚不是 virtualization。另有全树 `parent_of` 与每次事务首次写 map 分离等应进入 profile 的成本。

这是静态复杂度证据，**不是已测得的卡顿时间或性能达标结论**。建议先建 1k/10k blocks、长 code、复杂表格、多 editor、连续输入/撤销的重复基线，包含 child sync 与 RSS/history 增长；再评估按 NodeId 复用/线性合并、局部失效及 virtualization 的收益。不能照搬旧审计的逐节点 map 复制结论。

### A7 · P2 · 部分“当前事实”文档落后于 main

已核对的具体冲突：

- [planning 交付边界](../planning.md#delivery-boundaries) 仍称 harness 未注入真实资产服务、图片导入只在实验分支；`examples/editor_harness/src/main.rs:89` 已注入 `FixtureAssets`，`assets.rs` 已实现 PNG/JPEG 导入、sidecar 保存与解析。
- `architecture.md:133` 仍称 Task 基础未含命令/checkbox UI；当前已有 `EditIntent::SetTaskChecked`、`session/task_checked.rs` 和 `document_view/task_checkbox.rs`，并有自动化与竞态测试。完整 task 编辑器/宿主存储仍不是自动完成。
- `image-import-experiment.md:26` 的历史限制仍称 A→B→A 未排序；当前 `ImageLoadCache` / `NodeImageSink` 已有 request identity，且有 `returning_to_same_source_does_not_revive_an_old_request` 与 session 替换回归。该句需注明旧快照适用范围；同引用内容自动刷新等更大契约仍需另行明确。
- `architecture.md:568` 保留旧 metadata 错误统一回退 plaintext 的描述；[当前 framed v14/v15 契约](../table-clipboard-export.md#v14-传输和失败边界) 对已识别 native envelope 的损坏输入拒绝，不能把旧 fallback 规则套到所有版本。

建议：更新唯一交付边界表，统一“默认可用 / opt-in / 宿主负责 / 自动化通过 / 原生已验收”的标记；历史数字和限制保留版本说明。此次并未将上述过时文字当作功能缺失的证据。

## 尚未完成的验收与交付

| 优先级 | 缺口 | 下一步验收标准 |
| --- | --- | --- |
| P2 | P5 之后的可选能力缺少当前版本原生覆盖 | 针对真实字体/混合字号、首行缩进、measured table resize、原生 clipboard/IME 与多 editor，在目标平台按固定矩阵记录 commit、系统、输入法、步骤及结果 |
| P2 | 图片路径跨平台验收不完整 | 已有 Linux/X11 PNG 粘贴→undo/redo→保存/重开的历史原生记录；不等于 Windows/macOS、原生 JPEG 或所有失败路径已验收。当前声明应保持此边界 |
| P2 | P6 还没有可重复性能与内存基线 | 建立场景、硬件、p50/p95 编辑/粘贴/撤销延迟、布局/绘制耗时、RSS/history 指标与回归门禁，再决定优化顺序 |
| P3 | P7 API/发布/平台稳定化 | 明确 0.x API 和 wire 兼容/迁移，补真实宿主接入与 patch 依赖检查；按计划交付通用 BlockRendererRegistry / LinkOpenService、平台无障碍 smoke 与发布流程 |
| P3 | 文件规模与维护成本 | source-size 有 37 项 >500 行提示（包含内联测试），没有 >700 行失败；优先拆职责混杂的生产模块，避免为了行数大规模机械重构 |

依据：[mixed-font-size 验收边界](../mixed-font-size.md#有界能力与验证边界)、[first-line-inset](../first-line-inset.md#conservative-support-and-evidence-limits)、[列宽验收边界](../measured-column-resize.md)、[图片原生记录](../image-import-experiment.md#initial-native-linux-gate-and-close-regression)。P5 的 2026-10-01 Windows Gate 继续有效；不能拿它为之后所有增量背书。本次没有执行新的人工输入法/系统剪贴板验收。

## 实际验证记录

| 检查 | 结果 |
| --- | --- |
| 当前 main 的 push CI | [run 39954510652](https://github.com/prtitrz/Xiaomu/actions/runs/39954510652)，Ubuntu/Windows/macOS/policy/CI Success 成功；advisories 可失败步骤的实际错误另见 A4 |
| 本机 macOS，Rust 1.97.1，workspace all-targets | **1,856 passed，0 failed，0 ignored**，118 个结果分组 |
| workspace rustdoc | **6 passed，0 failed** |
| fmt / Clippy `-D warnings` | **通过**；既有依赖 future-incompat 提示不当作当前编译失败 |
| source-size / dependency boundaries / XIM / Taffy provenance | **通过**，37 个规模提示，无硬门禁失败 |
| cargo-deny bans / licenses / sources | **通过** |
| cargo-deny advisories，2026-10-24 | **失败，8 项通告**；5 项 vulnerability、3 项 unsound，未证明均有应用可达性 |
| 仓库外新增语义断言 | **4 failed**：标题 Bold、code 边缘空格、code 字面反引号、段尾 LF；断言要求“导出拒绝或无损往返” |
| 仓库外深层引用探针 | 8192 层 / 16,389 bytes 导致 debug 进程栈溢出；较浅对照成功 |

外部复现目录为 `/tmp/xiaomu-main-audit-20261024`，通过 path dependency 直接调用上述 main 的真实源码；没有复制/改写 codec 实现，也没有向工作区加入失败测试。复现命令：

```sh
cargo test --offline --manifest-path /tmp/xiaomu-main-audit-20261024/Cargo.toml
cargo run --offline --manifest-path /tmp/xiaomu-main-audit-20261024/Cargo.toml
python3 /tmp/xiaomu-main-audit-20261024/check_depth.py
```

临时目录不保证跨机器存在；A1–A5 已给出可重新构造的输入、实际结果和代码位置。修复时应把这些场景迁为仓库永久回归，增加生成式语义往返和资源边界覆盖。

## 建议执行顺序

1. **先做一个 Markdown 正确性批次**：A1–A3 的四个失败断言进入永久测试，同时补 A5 的有界导入；保持“不能保真就拒绝”，不要扩成 Markdown 全语法重写。
2. **独立做依赖维护批次**：处理 A4 的兼容更新和实际可达性审查，记录剩余项，三平台验证；与 codec 修复分开便于回退和审查。
3. **同步当前交付表并补所需原生验收**：处理 A7，按实际要启用的宿主能力排矩阵。没有证据的能力保持明确边界，不重新打开已经关闭的 P5。
4. **正式建立 P6 基线再优化**：先量化 A6 等热路径；将“场景、阈值、证据、回归”写入 P6 阶段契约，之后再排局部缓存、数据结构与 virtualization。
5. **P7 再收 API 与发布承诺**：以一个真实消费者的完整加载、编辑、保存、资产、链接、焦点、多实例链路验收，区分 baseline Markdown、harness fixture 和宿主长期存储格式。

本次审查以规划兑现、Core/Runtime 事务与历史、表格/clipboard、GPUI 宿主边界、Markdown、依赖和验收记录为重点。现有全量测试通过只说明已有覆盖，没有穷尽全部输入与平台；本报告中的确定缺陷不依赖对旧分支结论的沿用。
