# 混合字号原型边界（2026-10-03）

本地迁移实验，尚未接入 `DocumentView`、生产缓存或 IME 绘制。`font_size` 与 `mixed_size` 均以 `cfg(test)` 隔离；标准 GPUI 0.2.2 与现有输入路径保持不变。它不代表显式字号已可编辑，也不改变宿主当前整篇只读策略。

## 已实现并执行的检查

- CSS 字号解析保留原始三态和字符串，支持绝对单位、em/rem/% 与明确上下文；未知表达式返回错误，512px 是阶段资源预算，不是文档 schema 限制。16 项解析测试
- 统一字号委托 stock GPUI；混排原型按真实字号 shape_line，合并基线、行高、换行、选区、hit/caret 与范围几何。25 项内核测试
- 仅保守 LTR 脚本可进入混排；复杂脚本、危险跨字号 shaping 接缝拒绝。这不是永久降级决定
- 预留 shaping bytes/calls 预算，二分定位样式 runs，单调 glyph 游标生成 scalar caret；换行仅使用安全 cluster 边界
- 整库701项测试及严格 Clippy 通过。GPUI test backend 是 NoopTextSystem；连字、非零 glyph origin、combining/ZWJ 等特定布局测试含合成 glyph 数据，不冒充真实字体、DPI、fallback 或原生 IME 验收

日志：`/workspace/shared/xiaomu-text-style-checks/font-size-kernel-tests.log`、`font-size-kernel-clippy.log`。独立静态复核已关闭重复全 runs 扫描、逐 scalar 重扫 glyph、fragment origin 不一致三项发现。

## 下一步与完整能力边界

穿云独立 `experiments/text-shaping-oracle` 已使用固定 SHA 的官方 Noto 字体实际运行：Cosmic 0.14.2 的 fi/ffi、Arabic 跨 12/48 字号仍形成跨边界 cluster；小号 override 也会缩小默认行高。否证成功不是混排能力通过，不能把现成 Buffer 接入当作完整修复。

继续可逆验证公共 API 的同一字体 face → Rustybuzz shaping → Swash raster → straight BGRA → GPUI RenderImage 路线。仍需证明上下文、双向/cluster、alpha、DPI、字体发现/fallback、跨平台差异和有界 CPU/逐窗 GPU 缓存。GPUI 私有 FontId/GlyphId 不允许伪造；不新增旧输入补丁。若完整方案需替换实际正文字体后端，应单独评估技术路线与产品代价后再发布。
