# P4B Atomic Block / Image / Asset Capability 设计

> 状态：**PLANNED AFTER P4A**

P4B 补齐顶层路线中已经存在 `Image` / `HorizontalRule` / `Atomic` / `AssetService` 概念、但此前缺少明确实施阶段的问题。

它是统一 P4 的后半段。P4A 先解决 mixed-inline coordinate 与 inline atom；P4B 再把 document traversal 从 text-only 扩展到 block atomic / media，并建立真正的 asset capability seam。

## 1. 阶段目标

P4B 要证明晓木可以在不把文件系统、网络、宿主数据库或 GPUI 类型带进 Core 的前提下，完整支持 block-level 非文本结构化内容。

通用 atomic 闭环：

```text
canonical Atomic node
        ↓
DocumentSession selection / navigation / transaction / history
        ↓
frontend renderer / hit-test
```

Image 在此基础上增加：

```text
host imports / owns asset
        ↓
stable AssetRef / image attrs
        ↓
Xiaomu Image node (Atomic)
        ↓
AssetService resolve
        ↓
GPUI image layout / paint / hit-test
```

P4A 已验证的 capability 原则继续沿用：renderer / action 不持有宿主业务数据库对象，宿主业务动作通过窄 capability seam 执行。

## 2. 与 P4A 的边界

P4A 负责：

```text
InlinePoint / atom ordinal
InlineAtom canonical identity
atom transaction / mapping / inverse
mixed-inline Runtime editing
InlineAtomRendererRegistry
host capability callback
```

P4B 负责：

```text
atomic block traversal / NodeSelection
HorizontalRule canonical interaction
Image canonical attrs / insertion
AssetRef / AssetService
GPUI image resolve / layout / paint / hit-test
atomic/image clipboard
Markdown baseline codec
P4 final integration Gate
```

P4B 不重新定义 `TextOffset` 或 inline atom order，也不把 Image 做成 text sentinel。

## 3. Image canonical semantics

`NodeKind::Image` 已存在并使用 atomic content。P4B 不把图片像素、平台 texture handle、宿主文件对象或本地绝对路径 identity 塞进 canonical document。

Image node 只保存稳定、可序列化、frontend-neutral 的语义，例如：

```text
source / asset_ref
alt
optional title
optional intrinsic width / height metadata
optional stable presentation attrs
```

### 3.1 不保存宿主绝对路径作为通用 identity

禁止把：

```text
C:\Users\...\image.png
/Users/.../image.png
```

固化为跨宿主 canonical identity。

宿主负责导入、下载、持久化、权限与缓存；晓木只持有 stable / opaque reference，或由 codec 明确导入的外部 URL source。

具体 source representation 在 P4.7 contract 中定型，不提前把某个产品 asset id 格式写入 Core。

## 4. AssetService 边界

Host Contract 已预留 `AssetService`。P4.7 将其落成真正 capability seam。

概念责任：

```text
Xiaomu asks: resolve(asset_ref)
Host decides: file / database / network / cache / permission
Frontend receives: renderable bytes/resource or typed failure
```

要求：

- Core 不依赖异步 runtime、文件 API 或图片解码库；
- Runtime capability contract 不暴露 GPUI texture/image types；
- GPUI 负责把解析结果转换为自己的 render resource；
- resolve 可异步完成，回调不能直接修改 canonical document；
- stale resolve result 必须按 node identity / source revision 判断是否仍可进入 view cache；
- host-specific storage key 只能作为 opaque value 穿过公开 seam。

## 5. Selection / navigation

当前 document editing traversal 主要围绕 inline-bearing block。P4.6 必须把“可编辑文档顺序”推广为 text + atomic sequence。

期望行为：

```text
Paragraph A | caret
HorizontalRule
Image
Paragraph B

Right
→ HorizontalRule NodeSelection
Right
→ Image NodeSelection
Right
→ Paragraph B start

Left
→ Image NodeSelection
Left
→ HorizontalRule NodeSelection
Left
→ Paragraph A end
```

点击 atomic block：

```text
NodeSelection(HorizontalRule | Image)
```

删除：

```text
NodeSelection + Backspace/Delete
→ one logical history change
→ atomic node removed
→ selection 收敛到合法邻接位置
```

IME 永远不能进入 atomic node interior。

P4B 第一版不要求任意 node-range selection 或多个 atomic node 的专门 selection type；需要跨范围复制时优先扩展 P3 已稳定的 document selection / structured clipboard seam。

## 6. 图片布局与表现边界

第一版只做 block image：

```text
Paragraph
Image
Paragraph
```

P4B 不做：

```text
inline image inside text
float / text wrap
crop editor
free transform
multi-image gallery
caption editor
complex responsive layout
```

GPUI renderer 至少处理：

```text
loading
loaded
failed
max content width
preserve aspect ratio
optional intrinsic dimensions
selection affordance
hit-test
```

图片解码/缓存策略可由 frontend 或 host capability 实现，公开 contract 不绑定具体 image crate。

## 7. Clipboard / codec

P3 已建立 structured clipboard；P4A 会增加 inline atom fragment；P4B 再增加 atomic/image payload。

要求：

```text
copy atomic/image node
→ structured Xiaomu payload when possible
→ semantic plain-text / URL fallback when meaningful
```

P4.9 建立真正的 Markdown baseline codec，而不是只为 Image 增加孤立 parser。至少覆盖届时已落地的 built-in 语义：

```text
paragraph / heading / quote
bullet / ordered list
basic marks / link
code block / hard break
horizontal rule
image
```

Image Markdown 映射至少覆盖：

```markdown
![alt](source "title")
```

Markdown source URL 如何映射宿主 `AssetRef` 属 adapter/codec policy；Core 不下载、不复制文件。

## 8. Renderer / capability 关系

P4A 建立 `InlineAtomRendererRegistry` 与 host capability callback 后，P4B 可以继续建立或稳定：

```text
BlockRendererRegistry
AssetService
LinkOpenService（若届时纳入同一 host capability layer）
```

extension 可提供：

```text
rendering
hit-test / action
optional command handlers
serialization payload schema
accessibility fallback
```

Built-in Image 可以使用内建 renderer，但 asset resolving 必须遵守同一 capability 原则，不能形成 built-in 特权通道穿透宿主边界。

## 9. 分片计划

### P4.6 Atomic Block Contract

```text
editable text + atomic traversal model
NodeSelection / document-level atomic position contract
HorizontalRule keyboard traversal / select / delete / copy / undo
atomic insertion/removal mapping and selection fallback
property / invariant tests
```

Gate：`text ↔ HorizontalRule ↔ text` 可以纯键盘稳定导航、选中、删除、复制和 undo/redo。

### P4.7 Image Canonical Model / AssetService

```text
Image attrs / typed accessor contract
frontend-neutral AssetRef / ImageSource semantics
InsertImage or equivalent atomic insertion command
AssetService contract
host-neutral error/result model
```

Gate：host 可通过公开 contract 插入一张图片；canonical snapshot 不保存宿主文件对象；AssetService 能解析 stable reference。

### P4.8 GPUI Image / Atomic Interaction

```text
async resolve
loading / error placeholder
layout / paint / hit-test
intrinsic size / aspect-ratio handling
click-to-select image
keyboard traversal
Backspace / Delete
undo / redo
minimal accessibility fallback
```

Gate：`text ↔ Image ↔ text` 的鼠标、键盘、focus、selection、scroll 行为稳定；异步 resolve 不污染 canonical document。

### P4.9 Clipboard / Markdown / P4 Final Closeout

```text
atomic/image structured clipboard
plain-text / URL fallback
baseline built-in Markdown round-trip
Image / HorizontalRule / marks / links preservation
unknown payload / attrs preservation
realistic media + extension integration fixture
multi-editor isolation
Unicode + atom + atomic matrix
Windows final real-machine Gate
architecture / planning / progress final sync
CI Success
```

P4.9 通过后，且 P4A Gate 已成立，才允许 **P4 = CLOSED** 并进入 P5 Table。

## 10. P4B / P4 Final Gate

至少满足：

```text
text ↔ HorizontalRule ↔ text 可用键盘稳定导航与 NodeSelection
host 可通过公开 contract 插入一张图片
canonical document 只保存 frontend-neutral / host-neutral 图片语义
AssetService 异步 resolve 图片且失败有占位
图片可由鼠标和键盘选中
方向键可在 text ↔ image ↔ text 之间稳定导航
Backspace / Delete / copy / cut / undo / redo 对 atomic/image 行为明确
structured clipboard 可承载 atomic image payload
P4A demo InlineAtom 已作为 one-caret-unit 完整操作
extension renderer/capability fixture 证明 Core/Runtime 无宿主业务类型
Markdown baseline 对当前 built-in 语义 round-trip 不静默丢失
unknown extension/image attrs preservation 测试通过
accessibility fallback seam 存在
三平台 CI + Windows 实机 Gate 全绿
```

P4 完成后，晓木才真正跨过“只有文本 block 的编辑器”边界，再进入 Table。
