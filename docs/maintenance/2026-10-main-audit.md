# 2026-10 main 审计修复进度

更新：2026-10-25（Asia/Shanghai）。原始审计基线为 main `7015166`，修复分支为
`codex/main-audit-fixes`。本记录与 [planning](../planning.md#delivery-boundaries)
同步，保留[原始审计](../archive/2026-10-24-main-audit.md)的历史失败证据。
P0–P5 / P5.6 历史验收不变，维护分支通过不等于已经合入 main。

## 问题处置

| 审计项 | 本批处置 | 状态 / 边界 |
| --- | --- | --- |
| A1 P1 标题格式丢失 | 使用支持的 inline mark 输出并检查精确文本/marks；Underline/TextStyle/heading Link 继续拒绝 | 已修复，覆盖 1–6 级标题和组合格式 |
| A2 P1 Code 空格/反引号 | 分隔符必要 padding；可能被读成 block fence 的行首明确拒绝；邻接 delimiter 逐 run 保真校验 | 已修复，不能表达时拒绝，不插入隐形字符 |
| A3 P1 段尾 LF 丢失 | 段尾 LF 明确 HardBreakNotRepresentable；quote/list/marks 共用规则，补列表内部 LF 读写 | 已修复，canonical 文档不被规范化修改 |
| A4 P1 依赖通告 | rustls 0.23.46 / webpki 0.103.16 / tokio 1.53.3；保留 GPUI 0.2.2 | 6 项消除；2 项 git2 测试专用通告已评估，版本修复仍待上游约束解决，见[处置记录](../dependency-advisories.md) |
| A5 P2 解析栈溢出 | import 字节/节点/深度/shared work budget；export 迭代深度/节点/输出预算预检 | 已修复，8192 层引用子进程返回 typed error；另修孤立列表标记、Tab fence 字节越界与残缺 destination 越界 |
| A6 P2 子视图 O(n²) | NodeId map 复用，保留 Entity/focus；稳定/逆序 1k/10k release benchmark | 算法问题已修复；P6 原生布局、内存与虚拟化尚未验收 |
| A7 P2 事实文档过时 | 更新图片服务/导入主线、Task 命令/checkbox、图片 request identity、framed clipboard 拒绝规则 | 已同步 planning / architecture / 图片记录及统一原生矩阵 |

Markdown 新边界与兼容影响见[codec 契约](../markdown-codec.md)：超过固定资源限制的
输入现在会拒绝；原先会丢内容的导出现在拒绝或保真。它仍是 baseline codec，
不是任意 canonical 文档的无损存储格式。错误必须由宿主显示/处理，不能静默覆盖文件。

## 本地验证

环境：Apple M3 / 24 GiB / macOS 26.1（25B78），Rust 1.97.1。

- workspace all-targets：**1,868 passed，0 failed，0 ignored**，比基线新增 **12** 个永久测试入口（10 个 codec、2 个 child reuse）；参数矩阵涵盖多种输入组合。
- workspace doctests：**6 passed**；strict Clippy、fmt、source-size、依赖边界和两个 vendor provenance guard 通过。
- XIM 原始独立测试 **8 passed**；Taffy 发布包独立测试 **89 + 5 doctests passed**；GPUI min-content 回归 **4 passed**。
- bans / licenses / sources 通过；advisories 从 8 降到 **2**，仍返回失败状态，未忽略通告、未关闭扫描。新增生产图范围守卫通过。
- 1k/10k CPU baseline 已执行并记录到 [P6 progress](../phases/p6-performance/progress.md)；10k 稳定顺序 p50 从约 11.1 ms 降到 0.25 ms。该数据不代表真实应用输入/绘制耗时。

复现命令以 CONTRIBUTING 的完整门禁为准，新增：

```sh
cargo test --locked -p xiaomu-codec-markdown
cargo test --locked -p xiaomu-gpui --lib child_sync_tests
cargo bench --locked -p xiaomu-testkit --bench child_reconcile
python3 tools/check_dependency_advisory_scope.py
cargo deny check advisories
```

## 三平台 CI 验收

[PR #122](https://github.com/prtitrz/Xiaomu/pull/122) 的生产代码与测试提交
[`0ebb17733a1ac8a1c72f40ecdf7652890384c580`](https://github.com/prtitrz/Xiaomu/commit/0ebb17733a1ac8a1c72f40ecdf7652890384c580)
通过 [CI 40166314837](https://github.com/prtitrz/Xiaomu/actions/runs/40166314837)：

| 检查 | 结果 |
| --- | --- |
| Windows / macOS / Ubuntu | 全部 SUCCESS |
| policy / CI Success | SUCCESS |
| Ubuntu 新增 git2 生产依赖图守卫 | PASS |
| advisory 扫描 | 仍输出 0183 / 0184 两项 git2 通告；既有非阻塞策略不变 |

本段为后续文档提交，只追加验收证据；最终文档提交的重跑状态见 PR checks。
不将 CI SUCCESS 写成 advisory 无通告，也不替代原生平台操作验收。
原始审计的 4 个独立失败复现现已全部通过；resource_limits 的 5 个测试也在
release profile 通过（含 8192 层子进程防崩溃），补充 debug 证据。

## 保留的后续验收

- git2：维护者按版本/feature/目标平台/调用变化复查，最迟 2026-11-25；不宣称 crate 已修复。
- [原生矩阵](native-acceptance.md)：Windows/macOS 图片及 JPEG、真实字体/缩进、列宽拖动和新增 clipboard/输入能力仍需目标平台操作证据。本轮没有新做这些原生验收。
- P6：已启动 CPU 基线切片；编辑/粘贴/undo、真实 shaping/layout/paint、RSS/history、多 editor、virtualization Gate 待完成。
- P7：API/兼容/发布、通用 BlockRendererRegistry / LinkOpenService、screen-reader smoke 仍按规划后续交付。本次修复不把整个工程标成完成。
