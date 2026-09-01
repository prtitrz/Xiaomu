# P4 Structured Content / Extension 总览

> 状态：**IN PROGRESS**
>
> P3：**CLOSED（2026-09-01）**
>
> 当前施工线：**P4A Inline Atom / Extension Seam**

P4 的统一目标是让晓木从“只处理文本 block”跨到**结构化非文本内容**，同时保持 Core / Runtime 宿主中立、`TextOffset` 语义稳定、transaction / mapping / history 可组合。

P4 由两条连续子线组成，不再维护两套并列的 P4 规划。

```text
P4A Inline Atom / Extension Seam
  P4.1 Inline Coordinate Contract
  P4.2 Canonical Inline Atom
  P4.3 Runtime Atom Editing
  P4.4 GPUI Renderer / Host Capability
  P4.5 Inline Atom Integration Gate

P4B Atomic Block / Media
  P4.6 Atomic Block Contract
  P4.7 Image Canonical Model / AssetService
  P4.8 GPUI Image / Atomic Interaction
  P4.9 Clipboard / Markdown / P4 Final Closeout
```

执行顺序固定为：

```text
P3 cross-block text/history
        ↓
P4A mixed-inline coordinate + atom semantics
        ↓
P4B block atomic + image/media capability
        ↓
P5 Table
```

## 为什么先 P4A，再 P4B

Inline atom 会直接挑战 P0-P3 已稳定的文本坐标契约。同一个 UTF-8 byte boundary 上可以存在多个 atom，因此必须先解决 `InlinePoint`、atom ordinal、atom-aware transaction / mapping / selection / clipboard / IME 等基础语义。

Atomic block / Image 主要挑战另一组问题：`NodeSelection`、text ↔ atomic traversal、asset reference、异步 `AssetService`、image layout/render、atomic clipboard 与 codec。它们仍属于 P4，但在 mixed-inline 基础语义稳定后实施，避免两套复杂坐标问题同时展开。

## P4A 文档

详见 [`inline-atom.md`](./inline-atom.md)。

核心约束：

- `TextOffset` 永远只表示 canonical text 的 UTF-8 byte offset；
- inline atom 不使用 sentinel / fake byte；
- `InlinePoint(text_offset, atom_index)` 表达同一文本边界上的唯一 caret gap；
- `CursorAffinity` 只处理 soft-wrap / BiDi 等视觉歧义，不承载 atom order；
- atom seam 上的 mutation 必须最终消费 `atom_index`；
- extension action 通过 host capability seam，不把宿主业务类型带进 Core / Runtime。

## P4B 文档

详见 [`atomic-media.md`](./atomic-media.md)。

核心约束：

- `HorizontalRule` / `Image` 等 atomic block 进入统一 document traversal 与 `NodeSelection`；
- canonical Image 只保存 frontend-neutral / host-neutral 语义；
- 文件、数据库、网络、权限与缓存由 Host 拥有；
- `AssetService` 负责 opaque/stable asset reference 的解析；
- GPUI 负责 loading / loaded / failed、layout、paint、hit-test；
- atomic/image clipboard 与 Markdown codec 不允许静默丢失语义。

## P4 总 Gate

P4 只有在 P4A 与 P4B 都完成后才能 CLOSED。至少要求：

```text
Inline Atom
- demo atom 是真正 one-caret-unit canonical value
- 相邻两个 atom 可导航、选择、独立删除
- atom seam 输入不污染 UTF-8 TextOffset contract
- copy/cut/paste/undo/redo/mapping 保持 atom 语义
- missing renderer 有 deterministic fallback
- accessibility 使用 fallback_text

Atomic / Media
- text ↔ HorizontalRule ↔ Image ↔ text 可稳定键盘导航
- atomic/image 可鼠标与键盘选中、删除、复制、撤销
- host 可通过公开 contract 插入并解析图片
- canonical document 不保存宿主文件对象或本地绝对路径 identity
- AssetService 异步 resolve 有 loading/error fallback
- structured clipboard 可承载 atomic image payload
- Markdown baseline 对当前 built-in 语义 round-trip 不静默丢失

Integration
- extension / capability fixture 不引入宿主业务类型到 Core/Runtime
- unknown extension/image attrs preservation 成立
- Unicode + IME + history regression 全绿
- 三平台 CI、source-size、dependency-boundary、policy 全绿
- Windows 最终实机 Gate 通过
```

统一进度见 [`progress.md`](./progress.md)。
