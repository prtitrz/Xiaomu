# 晓木文档入口

先看 [planning 当前里程碑与路线](planning.md)，再看对应阶段的 `progress.md`。**执行只沿 planning 一条主线；审计报告不形成第二套排期。**

## 按问题查文档

| 要回答的问题 | 入口 | 文档职责 |
| --- | --- | --- |
| 现在做到哪里，下一步做什么？ | [planning](planning.md) | 阶段顺序、范围、Gate 与待排期事项的唯一主线 |
| 用户现在能直接做什么，哪些只有底层接口？ | [交付边界](planning.md#delivery-boundaries) | 区分引擎能力、宿主接入与可操作功能；不把阶段 CLOSED 当作所有产品功能已完成 |
| 代码当前实际上怎么组织？ | [architecture](architecture.md) | 已实现的架构事实，不承诺未来功能 |
| 某阶段怎么设计、如何验收？ | 下方阶段导航 | design / overview 记录契约；progress 记录执行状态、CI 与原生验收证据 |
| 为什么选择这套长期契约？ | [ADR](adr/README.md) | 保留重要设计决策及理由 |
| 如何贡献、遵守哪些约束？ | [CONTRIBUTING](../CONTRIBUTING.md)、[engineering-rules](engineering-rules.md) | 工作流、依赖边界、测试与质量规则 |

## 阶段导航

| 阶段 | 设计 / 总览 | 进度与验收证据 |
| --- | --- | --- |
| P0 Core Contract | [design](phases/p0-core-contract/design.md) | [progress](phases/p0-core-contract/progress.md) |
| P1 Single Block Input | [design](phases/p1-single-block-input/design.md) | [progress](phases/p1-single-block-input/progress.md) |
| P2 Document Tree | [design](phases/p2-document-tree/design.md) | [progress](phases/p2-document-tree/progress.md) |
| P3 Visual Lines / Cross-block / History | [design](phases/p3-cross-block-history/design.md) | [progress](phases/p3-cross-block-history/progress.md) |
| P4 Structured Content / Extension | [overview](phases/p4-structured-content-extension/overview.md)、[inline atom](phases/p4-structured-content-extension/inline-atom.md)、[atomic media](phases/p4-structured-content-extension/atomic-media.md) | [progress](phases/p4-structured-content-extension/progress.md) |
| P5 Table | [overview](phases/p5-table/overview.md)、[design](phases/p5-table/design.md) | [progress](phases/p5-table/progress.md) |
| P6 Performance / P7 Stabilization | [planning](planning.md) | 尚未建立阶段执行文档；启动时再拆分 |

CI 与原生交互验收是两类证据，不可互相替代。P4 后补的 Windows 原生验收记录位于 [P5 progress](phases/p5-table/progress.md)，P4 文档保留引用。

可选宿主展示能力：[段落视觉对齐与几何边界](block-alignment.md)。

可选宿主字号能力：[固定样式、同源 admission 与受限原生混排](mixed-font-size.md)。

## 维护规则

已结束的审计移入 [历史归档](archive/README.md)，不与当前路线并列。阶段契约、验收记录和 ADR 保留；只有追溯旧问题时才需要读归档。

后续发现缺口时，先核实现有代码和验收证据，再把接受的工作及优先级写入 planning / phase contract。已完成、延期、待决策要明确区分，不能只在审计结尾留下一条无人跟进的建议。

独立编辑器的普通历史快照同步：[committed snapshot import](snapshot-import.md)。
