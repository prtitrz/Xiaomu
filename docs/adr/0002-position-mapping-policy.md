# ADR 0002：Position mapping 采用显式 bias 与显式 deleted 结果

状态：已接受
日期：2026-08-23

## 背景

P0.5 建立 StepMap / ChangeMap。一个旧 position 穿过 transaction 映射到新文档时，存在两类必须提前定死的语义：

1. **边界歧义**：替换文本区间、插入 child 都会产生"旧位置有两个合理新位置"的边界。例如 caret 恰好停在插入点上，映射后应留在插入文本之前还是之后？
2. **删除语义**：目标节点被移除时，position 是静默 clamp 到最近存活位置，还是显式报告失败？

静默 clamp 是编辑器 mapping 的经典错误来源：selection、decoration、history anchor 会被悄悄推到错误位置，且很难被测试发现。

## 决策

1. Mapping API 显式区分 `MappedPosition::Mapped(T)` 与 `MappedPosition::Deleted`。目标位于被删子树内的 position / gap / selection 一律返回 `Deleted`，Core 层永不静默 clamp。
2. 边界歧义由调用方传入 `MapBias`（`Start` / `End`)显式解析：
   - 落在被替换 range 内部的 offset 解析到 replacement 的起点或终点；
   - 恰好在空 range 插入点上的 offset 同样按 bias 解析（Start→原地，End→越过插入文本）；
   - 恰好在 NodeInserted 插入点上的 NodeGap 按 bias 解析；非空替换区间的终点 offset 视为"区间之后"，平移 delta。
3. `TextSelection` 的两端向外解析：较早端点取 Start、较晚端点取 End，使覆盖被替换内容的 selection 继续覆盖 replacement；collapsed selection 两端同用 Start 保持 collapsed。
4. 删除 child 时，指向被删 child 的 NodeGap 在原 index 存活（成为前一个兄弟之后、后一个兄弟之前的边界），仅其后的 gap 平移 -1。
5. Mapping 是纯坐标算术，不查询 snapshot；映射结果与任何 stale 坐标一样，使用前必须针对目标 snapshot 重新校验。
6. ChangeMap 由 apply 引擎产出，无公开构造入口；属性与 mark steps 不产生 step map 条目。

## 备选方案

- **静默 clamp 到最近合法位置**：拒绝。丢失"位置已失效"的信息，decoration / history anchor 会被悄悄污染，违反 P0 的 explicit-change-information 原则。
- **单一固定 bias（如一律 Start）**：拒绝。连续输入、选区扩展等场景需要不同方向；把选择权交给调用方成本极低且可测试。
- **Mapping 内部持有 snapshot 并保证结果合法**：拒绝。会让 mapping 依赖具体 revision，阻碍组合与未来 rebase；纯算术 + 事后校验已满足正确性。

## 后果

- 所有需要位置稳定的子系统（selection、decoration、history、未来协作 rebase）共享同一套显式语义，不允许各自维护 offset 修补逻辑。
- 调用方必须处理 `Deleted` 分支；这是刻意的 API 摩擦。
- 空 range 插入点的 bias 语义在 #9 修复后成立并有回归测试保护。

## 何时重新审视

- 引入跨 block selection 或 decoration anchor 时，若发现 bias 需要更细粒度（例如 per-endpoint affinity），可扩展 MapBias 而不推翻本决策。
- 未来协作 rebase 若要求 mapping 携带因果信息，应新增 ADR 取代本文档，而不是就地改写语义。
