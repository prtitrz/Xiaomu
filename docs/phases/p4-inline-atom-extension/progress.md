# P4 Inline Atom / Extension Seam Progress

## Current status

P4 已启动。P3 已于 2026-09-01 完成 Windows 最终实机 Gate，并通过 PR #53 squash merge 到 `main`（`f1167e97f2cac852fc56933c631b37978adcac9f`），阶段状态 `P3 = CLOSED`。

当前 P4.1 分支 / PR：

```text
feat/p4.1-inline-coordinate-contract
PR #50 (Draft)
```

P4.1 已基于 P3-closed 的新 `main` 做干净 tree transplant，旧施工提交收敛为单一 P4.1 commit；root `architecture.md / planning.md` 已同步。当前只等待最新 docs-only current-head CI Success，随后即可按既定 Draft workaround 建非 Draft merge PR 并 squash merge。

## P4.1 Inline Coordinate Contract

- [x] P4 overall design
- [x] mixed-inline coordinate ADR 0005
- [x] `InlinePoint` Core value type
- [x] pure-text validation / conversion
- [x] Runtime `DocumentPosition` compatibility seam
- [x] Core `StepMap / ChangeMap` mapping seam
- [x] GPUI `DocumentView` focus/selection projection seam
- [x] P0/P1/P2/P3 full regression
- [x] root `architecture.md / planning.md` sync after P3 closeout rebase
- [ ] docs-only final head CI Success

Gate：没有 atom 的现有文档行为零语义回归；`TextOffset` 继续严格表示 UTF-8 byte coordinate；P4.1 不引入 sentinel / fake byte；非零 atom ordinal 在 canonical placement 建立前 fail closed。

实现事实：

```text
TextPoint
↕ exact while atom_index == 0
InlinePoint(node_id, text_offset, atom_index, affinity)

Core
  InlinePoint::validate
  StepMap::map_inline_point
  ChangeMap::map_inline_point

Runtime
  DocumentPosition::from_inline_point
  DocumentPosition::as_inline_point
  DocumentSelection::from_inline_points

GPUI
  DocumentView::inline_focus_point
  DocumentView::inline_selection_points
```

- `TextOffset / TextRange` 保持 ADR 0001 的 UTF-8 byte contract。
- `InlinePoint` 用同一 text boundary 上的 `atom_index = 0..=N` 区分相邻 atom 之间的 canonical caret gap。
- P4.1 当前 document 尚无 atom placement，因此 `atom_index != 0` 必须返回 typed failure；Runtime 不提前保存 document 无法验证的状态。
- 现有 `TextPoint`、P0-P3 Runtime selection 与 GPUI navigation/editing path 均保留，P4.1 只增加兼容/projection seam。
- 复核 P4.2 时确认一个关键约束：atom seam 两侧可能共享同一个 `TextOffset`，因此旧 `ReplaceText(TextRange)` 无法区分“在 atom 前输入”和“在 atom 后输入”。ADR 0005 已明确要求后续 mutation/mapping 消费 `InlinePoint.atom_index`，不能只在 selection/view 层保存 ordinal。

验证证据：

- 首轮 Core CI 只暴露 `inline_mapping.rs` rustfmt 差异；修正后 Ubuntu fmt/Clippy/workspace tests 通过。
- Runtime / GPUI seam 首轮 CI 仍只暴露 rustfmt module ordering / assertion formatting；修正后进入真实编译与全量 regression。
- implementation head `a2edcb35e4634b85294725ea1d19278276e754ae` 的 CI run #295：policy、source-size、dependency boundary、cargo-deny/advisory、Ubuntu fmt/Clippy/workspace all-targets、Windows/macOS workspace all-targets 与 aggregate `CI Success` 全绿。
- P3 closeout merge 后，P4.1 以 `f1167e97f2cac852fc56933c631b37978adcac9f` 为新 parent 重放自身 11 个文件，新基线未携带旧 P3 文档树；最终 current-head CI 作为 P4.1 merge Gate。

## P4.2 Canonical Inline Atom

- [ ] `AtomKind`
- [ ] `InlineAtomContent`
- [ ] stable `NodeId` atom identity
- [ ] `InlineContent` ordered atom placement
- [ ] full-tree validation / parent lookup
- [ ] `InsertInlineAtom` / `RemoveInlineAtom`
- [ ] atom-aware text replacement contract
- [ ] mapping / inverse
- [ ] adjacent atom invariant tests

P4.2 Gate 额外要求：atom seam 上文本输入不能把 `(text_offset, atom_index)` 降格为裸 `TextOffset` 后丢失 canonical 顺序。

## P4.3 Runtime Atom Editing

- [ ] one-caret-unit Left / Right
- [ ] atomic Backspace / Delete
- [ ] mixed text + atom selection
- [ ] atom-aware text input
- [ ] structured clipboard atom fragment
- [ ] plain fallback via `fallback_text`
- [ ] IME cannot enter atom
- [ ] history / undo / redo selection regression

## P4.4 GPUI Renderer / Host Capability

- [ ] `InlineAtomRendererRegistry`
- [ ] demo atom renderer
- [ ] mixed inline layout / paint
- [ ] hit-test
- [ ] accessibility fallback
- [ ] host capability callback
- [ ] missing renderer fallback

## P4.5 Integration / Closeout

- [ ] realistic extension fixture
- [ ] multi-editor extension isolation
- [ ] Unicode + adjacent atom matrix
- [ ] Windows real-machine Gate
- [ ] architecture / planning / progress final sync
- [ ] source-size / dependency / fmt / Clippy / tests
- [ ] final CI Success

## P4 Phase Gate

- [ ] demo atom is a true one-caret-unit canonical value
- [ ] two adjacent atoms are navigable and independently deletable
- [ ] copy/cut/paste/undo/redo preserve atom semantics
- [ ] unknown/missing renderer has deterministic fallback
- [ ] extension action crosses a host capability seam without host business types entering Core/Runtime
- [ ] accessibility always has fallback text
- [x] existing P0-P3 behavior remains green on P4.1 implementation head
- [ ] final CI Success
