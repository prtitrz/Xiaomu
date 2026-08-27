# P4 Atomic Node / Image / Extension Seam 设计

状态：规划

本文档补齐顶层路线中已经存在 `Image` / `HorizontalRule` / `Atomic` / `AssetService` 概念、但缺少明确实施阶段的问题。P4 先建立通用 atomic block contract，以 built-in HorizontalRule 验证无资源 atomic semantics，再以 Image 验证 asset capability，最后继续以 InlineAtom / custom renderer 验证 extension boundary。

## 1. 阶段目标

P4 要证明晓木可以在不把文件系统、网络、宿主数据库或 GPUI 类型带进 Core 的前提下，完整支持“文本之外的结构化内容”。

目标闭环：

```text
canonical Atomic node
        ↓
DocumentSession selection / navigation / transaction / history
        ↓
frontend renderer / hit-test
```

Image 在这个通用 atomic contract 上再增加：

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

同时：

```text
InlineAtom / CustomBlock
        ↓
renderer registry + capability callbacks
```

## 2. 范围

### P4.1 Generic Atomic Block + Built-in Image

必须交付：

```text
DocumentView 从 text-only traversal 推广为 editable text + atomic sequence
HorizontalRule keyboard traversal / NodeSelection / delete / copy / undo
Image canonical attrs / typed accessor contract
frontend-neutral AssetRef / ImageSource 语义
InsertImage（或等价 atomic-node insertion command）
NodeSelection / document-level atomic selection 接入 runtime
atomic node keyboard traversal
click-to-select image
Backspace / Delete image
copy / cut image 的 structured clipboard 表达
undo / redo image insertion / deletion
GPUI image renderer
async asset resolution
loading placeholder
error placeholder
intrinsic size / aspect-ratio layout
alt / title preservation
minimal accessibility fallback
```

### P4.2 Inline Atom / Extension Seam

必须交付：

```text
InlineAtom canonical representation
atom = one caret unit
atom navigation / delete / copy
InlineAtomRendererRegistry
BlockRendererRegistry
host capability callbacks
demo inline atom
demo custom block or renderer override
unknown extension payload preservation test
extension accessibility fallback
```

## 3. Image canonical semantics

`NodeKind::Image` 已存在并使用 `NodeContent::Atomic`。P4 不把图片像素、平台 texture handle 或宿主文件对象塞进 canonical document。

Image node 只保存稳定语义，例如概念上：

```text
source / asset_ref
alt
optional title
optional intrinsic width / height metadata
optional presentation attrs（仅稳定、可序列化且 frontend-neutral 的部分）
```

### 3.1 不保存宿主本地绝对路径作为通用 contract

禁止把：

```text
C:\Users\...\image.png
/Users/.../image.png
```

固化为晓木跨宿主 canonical identity。

宿主负责导入、下载、持久化与权限；晓木只持有 stable / opaque reference，或由 codec 明确导入的外部 URL source。

具体 source representation 在 P4.0 contract 中最终定型前应保持可演进，不提前把某个产品 asset id 格式写入 Core。

## 4. AssetService 边界

Host Contract 已预留 `AssetService`。P4 将其落成真正的 capability seam。

概念责任：

```text
Xiaomu asks: resolve(asset_ref)
Host decides: file / database / network / cache / permission
Frontend receives: renderable bytes/resource or typed failure
```

要求：

- Core 不依赖异步 runtime、文件 API 或图片解码库；
- runtime capability contract 不暴露 GPUI texture/image types；
- GPUI 负责把解析结果转换为自己的 render resource；
- resolve 可异步完成，回调到达时不能直接修改 canonical document；
- stale resolve result 必须按 node identity / source revision 判断是否仍可应用到 view cache。

## 5. Selection / navigation

当前 P2 的 DocumentView 只枚举 inline-bearing block，`NodeContent::Atomic` 仅绘制占位线且不参与 caret traversal。P4 必须把“可编辑文档顺序”从 text-only block sequence 推广为 text + atomic sequence。

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
→ one transaction
→ atomic node removed
→ selection 收敛到合法邻接位置
```

IME 永远不能进入 atomic node interior。

P4 不要求多选多个 atomic node 或任意 node-range selection；如果 structured clipboard 需要范围表达，可复用 P3 已稳定的 document selection/clipboard seam 再扩展。

## 6. 图片布局与表现边界

第一版只做 block image：

```text
Paragraph
Image
Paragraph
```

P4 不做：

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

图片解码/缓存策略可以由 frontend 或 host capability 实现，公开 contract 不绑定某个具体 image crate。

## 7. Clipboard / codec

P3 先建立 structured clipboard；P4 在其上增加 atomic payload。

要求：

```text
copy atomic/image node
→ structured Xiaomu payload when possible
→ semantic plain-text / URL fallback when meaningful
```

P4.5 建立真正的 Markdown baseline codec，而不是只为 Image 加一个孤立 parser。至少覆盖当时已经落地的 built-in 语义：

```text
paragraph / heading / quote
bullet / ordered list
basic marks / link
code block / hard break（若 P3 已交付）
horizontal rule
image
```

Image 的 Markdown 映射至少覆盖：

```markdown
![alt](source "title")
```

但 Markdown source URL 如何映射宿主 AssetRef 属 adapter/codec policy，不允许 Core 自己下载或复制文件。

## 8. Extension registry

P4 保留两个 registry：

```text
InlineAtomRendererRegistry
BlockRendererRegistry
```

extension 可提供：

```text
rendering
hit-test / action
optional command handlers
serialization payload schema
accessibility fallback
```

extension 不拥有宿主业务数据库；需要业务动作时通过 capability callback 请求 host 执行。

Built-in Image 应优先走内建 renderer，但其 asset resolving 必须复用同一 capability 原则，避免 built-in 特权穿透宿主边界。

## 9. 分阶段切片建议

### P4.0 Contract

```text
editable text + atomic traversal model
atomic document position / NodeSelection contract
Image attrs / AssetRef semantics
AssetService contract
structured clipboard extension shape
```

### P4.1 Atomic core/runtime

```text
HorizontalRule traversal / select / delete / copy
Image insert / select / delete / undo-redo
mapping / selection fallback
property / invariant tests
```

### P4.2 Image GPUI

```text
resolve / loading / error / render
layout / hit-test
keyboard traversal + click selection
```

### P4.3 InlineAtom

```text
canonical atom
one-caret-unit editing
copy/delete/navigation
```

### P4.4 Registry / host capabilities

```text
renderer registries
demo atom / custom block
LinkOpenService + link editing seam
capability callback integration fixture
accessibility fallback
```

### P4.5 Markdown Codec + Gate

```text
baseline built-in Markdown round-trip
Image / HorizontalRule / marks / links preservation
unknown payload preservation strategy documented
real-machine image + atomic interaction Gate
docs / architecture sync
```

## 10. Phase Gate

P4 只有在以下条件全部满足时关闭：

```text
text ↔ HorizontalRule ↔ text 可用键盘稳定导航与 NodeSelection
host 可通过公开 contract 插入一张图片
canonical document 只保存 frontend-neutral / host-neutral 图片语义
AssetService 能异步 resolve 图片且失败有占位
图片可由鼠标和键盘选中
方向键可在 text ↔ image ↔ text 之间稳定导航
Backspace / Delete / copy / cut / undo / redo 对 atomic/image 行为明确
structured clipboard 可承载 atomic image payload
一个 demo InlineAtom 作为 one-caret-unit 完整操作
一个 extension renderer/capability fixture 证明 Core 无宿主业务类型
Markdown baseline codec 对当前 built-in 语义 round-trip 不静默丢失
unknown extension/image attrs preservation 测试通过
accessibility fallback seam 存在
CI + 实机 Gate 全绿
```

P4 完成后，晓木才算真正跨过“只有文本 block 的编辑器”这一边界，再进入 Table。
