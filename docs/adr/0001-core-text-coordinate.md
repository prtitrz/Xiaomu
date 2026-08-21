# ADR 0001：Core 文本坐标采用受校验的 UTF-8 byte offset

状态：已接受
日期：2026-08-22

## 背景

晓木需要一套统一的 Core text coordinate，用于 document position、selection、transaction、mapping 和 history。

可选坐标体系包括：

- UTF-8 byte offset；
- Unicode scalar-value index；
- UTF-16 code-unit offset；
- grapheme-cluster index。

Rust `str` / `String` 使用 UTF-8。部分平台文本 API，尤其 Windows 侧，会暴露 UTF-16 range。用户可见的 caret 行为未来又可能需要 grapheme-aware 语义。这几种边界解决的问题不同，不应被压缩成一个裸整数契约。

如果直接公开 raw byte offset，调用方可以构造落在 UTF-8 code point 中间的非法位置。如果一个 offset 一旦合法就被永久信任，同样不正确，因为文本修改后旧坐标可能已经 stale。

## 决策

`xiaomu-core` 使用 opaque `TextOffset`，其内部数值表示为 UTF-8 byte index。

普通外部调用方不能从任意 `usize` 直接构造 `TextOffset`。坐标通过 `TextBuffer` 获取，由 `TextBuffer` 校验范围和 UTF-8 character boundary。

已有 offset / range 每次应用到目标 buffer 时都重新校验。一个坐标在旧 revision 上合法，不代表它在新 revision 上仍然合法。

`TextRange` 使用半开区间 `[start, end)`，并要求顺序合法。预期的非法坐标和 range 返回 typed error，不 panic。

UTF-16 转换属于 platform/frontend adapter，不进入 Core coordinate contract。

Core text boundary 保证 Unicode scalar safety，但不定义 grapheme-cluster caret movement。未来 grapheme-aware navigation / deletion 可以在更高编辑层实现，最终仍落到经过校验的 `TextOffset` 执行 Core operation。

## 考虑过的替代方案

### UTF-16 code-unit offset

优点是与部分平台 API 一致；缺点是会把平台形态带进 Core，并迫使 Rust 字符串操作频繁转换。因此限制在 platform adapter。

### Unicode scalar-value index

概念上比 byte 更抽象，但转换到 Rust string byte position 时通常需要扫描或额外索引结构，并且仍然解决不了 grapheme 语义。

### Grapheme-cluster index

Grapheme 对用户可见 caret 行为很重要，但不适合作为低层 storage coordinate。分词规则更复杂，也不直接对应 Rust string slicing boundary。

### 公开 raw byte offset

实现最简单，但允许调用方构造落在 UTF-8 code point 内部的坐标，并把 validation 责任扩散到整个代码库。

## 影响

正面影响：

- Core coordinate 与 Rust string storage / slicing boundary 一致；
- UTF-8 validity check 集中在统一边界；
- UTF-16 保持为 adapter concern；
- 未来替换底层 text storage 时，只要保持语义 contract，可以继续隐藏在 `TextBuffer` 后面；
- stale-coordinate validation 是显式行为，不依赖偶然信任。

成本和约束：

- frontend 接收到 UTF-16 position 时必须转换；
- grapheme-aware editing 需要更高层 segmentation；
- `TextOffset` 在文本修改后可能失效，不能被当作永久 anchor；
- 如果未来 text storage 改变，性能优化可能需要额外 indexing support。

## 何时重新评估

出现以下情况之一时重新评估：

- profiling 证明 UTF-8 byte coordinate 阻碍了必要的 text-storage architecture；
- collaboration / persistent position identity 需要完全不同的 canonical coordinate model；
- grapheme-level semantics 无法干净地建立在 validated Core offset 之上；
- 某个跨平台 frontend coordinate contract 无法通过 adapter 隔离，且造成明显 correctness / performance 问题。
