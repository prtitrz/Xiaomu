# ADR 0003：Undo 采用 apply 时记录的精确逆 transaction

状态：已接受
日期：2026-08-22

## 背景

P0.6 需要为 undo 建立语义基础。三个必须定死的点：

1. **逆的来源**：inverse 信息在什么时候生成？从最终 snapshot 反推需要重放中间状态，代价高且容易和 apply 语义漂移（P0.6 评审已经抓到一次 mark 继承规则的实现漂移，见 PR #12）。
2. **还原的强度**：undo 是"语义等价"（结构/文本/marks 一致但 NodeId 可以变化），还是"精确还原"（store 完全相等，被删子树的 NodeId 原样恢复）？selection、decoration、history anchor 都依赖 NodeId 稳定性；如果 undo 会换 id，所有持有旧 position 的子系统都要额外映射。
3. **逆的载体**：逆是一个普通 `Transaction`（走唯一 canonical mutation 路径），还是独立的 change-set 应用通道？

## 决策

1. **inverse steps 在 apply 引擎内部、应用每个 step 的同时记录**，因为只有此时能看到该 step 的 before-state。`AppliedTransaction::inverse()` 返回一个 `System` origin 的普通 `Transaction`。
2. **undo 精确还原**：对 `inverse().apply(&applied.document())` 的结果，store 与 root 与原 snapshot 完全相等——不止语义等价，被删子树的 NodeId 也原样恢复。这使得依赖 NodeId 的 anchor 在 undo 后无需任何修补。
3. **`RemoveNode` 的逆是公开 step `RestoreSubtree`**：以原 NodeId 与 payload 整体回插子树。它不是通用 copy / move 原语：调用方无法铸造 NodeId，payload 只能来自同一文档 lineage 的历史 snapshot；所有 id 必须当前不存在，冲突时原子失败。
4. **逆 step group 按 step 反序组合**，group 内坐标与其原 step 产生的中间状态一致；多 step transaction 的逆不需要重放中间文档。
5. `ReplaceText` 的逆必须按 `replace_text` 的真实继承规则（首个 end 触及 `range.start` 的 run）计算要剥离的 marks；实现必须与编辑规则共享同一条判定，禁止两处各写一份边界规则。
6. P0 不引入 history 栈 / coalescing / selection 恢复的编排；那是 runtime/session 层在本 seam 之上的职责。

## 备选方案

- **语义等价还原（re-insert 换新 id）**：拒绝。每次 undo 都会让 selection、decoration、history anchor 失效一批 NodeId，把复杂性转嫁给所有下游；精确还原的成本只是 RestoreSubtree 的 id 冲突校验。
- **从 snapshot 反推 inverse**：拒绝。需要重放或保存全部中间状态，且规则容易与 apply 实现漂移；在 apply 时记录天然一致。
- **独立的 change-set 应用通道**：拒绝。会形成第二条 mutation 路径，违反"Transaction 是唯一 canonical mutation 路径"的 P0 硬约束。
- **inverse 只到 change-set 表示、不物化为 Transaction**：P0.6 曾保持开放；物化为普通 Transaction 让 undo、mapping、未来协作 rebase 共用同一词汇表，成本低且可测试性更好，故选择物化。

## 后果

- `AppliedTransaction` 携带 inverse 是长期公开契约；下游可以直接基于它构建 undo 栈而无需理解 change-set 内部结构。
- NodeId 在"删除后 undo"场景下保持稳定，成为可以依赖的语义承诺。
- `RestoreSubtree` 是公开可构造的 step，其防误用依赖：id 不可铸造、冲突原子失败、full-tree validation。P0.7 已在 rustdoc 中明确该契约。
- inverse 记录使 apply 的内存占用略增（逆 step 携带旧文本与子树 payload）；由 benchmark 驱动再优化，不提前冻结表示。

## 何时重新审视

- 协作层（OT/CRDT adapter）落地时：collaborative undo 语义与本 ADR 的 local undo 不同（见 planning.md §6.2），adapter 应建立自己的 undo 契约，而不是改写本决策。
- 若 benchmark 证明逐 step 记录 inverse 的成本显著，可以改为惰性生成，但"精确还原 + 共享继承规则"两点不得放弃。
