# 可选原生混合字号能力（2026-10-06）

`xiaomu-gpui` 现在把字号解析与受限混排接入生产 view；能力默认关闭。
这不是通用浏览器排版、完整 Unicode 混排或真实平台 IME 验收。
Core、Runtime、canonical 字符串、事务/history 和 GPUI 0.2.2 pin 不变。
2026-10-03 的[原型记录](mixed-font-size-prototype.md)保留为历史证据。

## 公共 API 与宿主责任

- `font_size::FontSizeContext::new(parent_px, root_px, absolute_medium_px)`：
  三个明确的逻辑 CSS-pixel 基值，不从字号字符串猜测主题或继承
- `font_size::resolve_font_size(&StringAttribute, &FontSizeContext)`：
  只读解析，返回实际像素或 `FontSizeError`，从不改写源数据
- `text_size::TextSizeStyle::new(gpui::Font, FontSizeContext, line_height)`：
  `line_height` 是无量纲倍率，font 包含字重、斜体与 fallback
- `TextSizeStyle::with_color(Hsla)`：明确 base color，默认 black；GPUI 会按
  color/decoration equality 合并 shaping runs，因此颜色也必须在 policy/view 一致
- `TextSizeStyle::with_font_family(css_list)`：通过与 inline TextStyle 相同的
  native family resolver 解释 family list；无可用匹配保留 base font
- `TextSizeStyleProvider::style(&XiaomuDocument, &Node)`：纯、确定性的当前
  canonical block 几何；可检查 ancestor 以区分 table/header、heading、code
- `TextSizeStyleProvider::styles_for_document(&XiaomuDocument)`：可选单次遍历，
  返回所有且仅 inline nodes 的 style map；缺失/额外 key 明确拒绝，默认逐节点调用 style
- `TextSizeCapability::new(Arc<gpui::WindowTextSystem>, Rc<dyn TextSizeStyleProvider>)`：
  固定一份字体系统和 provider，clone 共享同一实例
- `TextSizeCapability::validate_document(&XiaomuDocument, &InlineAtomRendererRegistry)`：
  逐 inline block 执行与 view 同源的字体/字号投影及 width-independent admission
- `EditorInstance::with_text_size_capability(Rc<TextSizeCapability>)`：
  将同一能力传入 document/paragraph views；不自动创建或替换 SessionPolicy

宿主必须在初始 load 和最终 candidate policy 中调用 `validate_document`。
现有 `SessionPolicy::validate_document` 覆盖普通输入、raw transaction、staged
最终发布、Undo 与 Redo；拒绝发生于 canonical state/history/listener 发布之前。
仅挂载 view capability 不能替代 policy。没有 capability 的宿主仍使用原 uniform
路径，不能因为 Core 能保存 fontSize 就声称该路径支持字号编辑。

provider 是启用后几何的权威，不与 window 的隐式字号级联各自推断。
body、table、heading 和 code 的实际字号/行高均由宿主明确给出；code 的 family
可通过 `with_font_family` 使用同一个解析器。base color 也由 provider 明确给出，
启用 view 不能继承另一套 window color；TextStyle/Link 颜色与既有装饰仍按相同
规则叠加。即使字号/font 相同，颜色或装饰边界也可能影响 GPUI shaping 分段。
provider 不可访问可变 view 状态、重入 session 或产生外部副作用。
主题、字体目录或 renderer 语义改变后，必须重新验证；不能在 admission 后
悄悄替换共享对象。validation 与 view 必须使用同一 immutable atom registry，
custom renderer 对相同 canonical atom 的显示文字必须确定。
批量 provider 的值须与单节点 style 语义一致；policy 一次准备整图，DocumentView
同步 child 时一次准备并附加规范化样式，普通渲染不重复 ancestor 搜索。单独构造
的 ParagraphView 仍支持单节点 provider fallback。

## 保真与安全范围

文档投影使用实际 text runs、带 marks 的 intrinsic HardBreak LF，以及注册
atom renderer 的实际 label；空 label 按既有契约回退 fallback_text。
TextStyle color/family、Bold/Italic、Link、Underline/Strike/Code 与字号共用
同一组 display spans/native TextRun，不把 atom display bytes 当 canonical bytes。

支持的简单字号包括 px/pt/pc/in/cm/mm/q、em/rem/%、inherit/unset、initial/medium。
Missing、Null、空白/空串继承 parent，但 canonical 三态与原字符串仍原样保留。
其它 absolute keywords、larger/smaller、viewport/container/font-metric 单位、
calc/var 等函数及不支持的语法均拒绝；不修复、不 clamp、不以默认字号降级。
最终字号必须有限、严格大于零且不超过 512px；这是 renderer 资源约束，不是
canonical schema 或 CSS 本身的最大值。

`TextSizeError` 提供 block NodeId、display UTF-8 半开范围和 typed kind；
范围包含 atom labels，不是 Core 文本 offset。错误是能力诊断，不能据此改写
原文。非法 host line-height/caret metrics 同样 fail closed，包括空 block。

## Layout、缓存与输入

所有最终 size spans 相等时，仍交给 stock GPUI 的完整 paragraph shaping，
使用实际解析字号，不将 uniform 文本误交给受限 mixed algorithm。
uniform Unicode/bidi 支持保持 native backend 行为。

mixed 路径按真实字号 shape 最终行片段，建立共享 baseline 与逐行 height。
每片使用一次 stock `shape_text(..., wrap_width=None)`，保留原 `WrappedLine`
用于 native glyph/decoration paint，并让 caret/cluster geometry 共享其公开
`unwrapped_layout` Arc。GPUI 0.2.2 的 `shape_line` 在装饰相同的相邻 runs 上
可能丢掉后续 font-only 变化，因此生产 mixed 路径不调用它。没有第二次 carrier
shaping、额外预算、伪造 FontId 或私有 API；uniform 与 mixed 都保留真实 font
run 解析。虚拟 font backend 不区分实际字体 face，这不替代原生字体验收。
默认 block strut 是行框的下限；小字号不缩小继承行框。UAX14 在整逻辑段落上
计算 break opportunities，换行、emergency wrap 和 pointer hit 使用安全 cluster
边缘；平台/键盘 scalar offsets 使用 native x-for-index 语义，不发明 ligature
内部 GDEF caret metrics。literal LF、marked HardBreak、空行与末尾 LF 保持明确
的 display/canonical 投影关系。

paint、selection、caret、二维 hit-test、Home/End、desired-x vertical movement
与 native range geometry 消费同一份 `BlockTextLayout`。Center/Right 对每个
真实 visual row 计算偏移，混排片段直接绘制其原生装饰。启用字号能力的 cache
包含实际 run/font/style、resolved size spans、空块 typing size 和精确 fractional
width；preedit 与无确定宽度的 intrinsic probe 不复用旧缓存。
只要启用字号能力，缺失 alignment provider 也使用显式 Left 的逐行坐标；
未启用字号能力的 legacy 默认不变。拒绝的 layout 不保存可复用 cache key，
因此无 revision/epoch 变化的 pending mark 更正也能从失败状态恢复。

`TextSizeStyleProvider::caret_height(TextSizeCaretContext)` 是独立可选视觉接口：
context 同时提供 effective_size 与可选的显式 stored_size/before_size/after_size，
缺失、null、空字串不伪装成显式来源；优先级与阈值全部由宿主决定。
默认 None 使用行框，Some 使用居中的指定逻辑像素高度；必须有限、正数且
不超过 1024px。绘制后的同一 caret rectangle 用于当前 collapsed native bounds。
产品的字号菜单、阈值、caret 倍率与字体主题不写入 generic engine。

preedit 仍是 frontend overlay。创建/更新时执行同源 admission；不支持时清除
并拒绝该 overlay、消费本轮后续 composition commit，发出无正文的 `EditorRejection`
反馈，不把它降级为普通 replacement 删除选区。最终 commit 仍经过 host policy。
`TextSizePreedit`、`TextSizeInput` 与 `TextSizeLayout` 区分 overlay、提交和当前帧
几何拒绝，均经所属 DocumentView 转发。不可用布局不提供 caret/hit/native bounds，
也不继续接受该输入 surface 的普通编辑；canonical 数据始终保留。
这不更改原生 composition 协议，也不修复 stock X11 composing 时的同步旧-layout
查询或不同平台的候选框行为。

## 有界能力与验证边界

mixed admission 明确独立于 wrap width。每个 block 在 shaping 前预留至多
1 MiB repeated shaped bytes 与 4096 shape calls，包含后续 reflow 的保守上界；
不是文档总内存/RSS、整篇 CPU、provider 或所有字体目录遍历的上界。
Uniform 路径不受 mixed-work 预算限制。超过预算直接拒绝，不逐次尝试更窄换行
直到无界工作。未使用 batch seam 而逐 block 查找 ancestors 的 host provider 仍可能在大文档上
产生二次复杂度；本次能力不声称已完成 P6 长文档性能优化。

mixed 的脚本允许集为保守 LTR：Latin、Han、Hiragana、Katakana、Hangul、
Bopomofo、Common 与 Inherited，另受控制字符和 emoji ZWJ 检查约束。
Arabic、Indic、bidi controls 等复杂混排，grapheme/observed cluster 内字号边界，
跨字号上下文依赖及不可靠 native cluster geometry 均拒绝。
同一实际字号的完整文本不受该 mixed-script 限制。

回归覆盖解析保真、同源 marks/family/size、uniform 与 mixed 分流、脚本/接缝/
budget、custom atom label、marked HardBreak、ancestor style 与非法空块几何。
GPUI `TestPlatform` 使用 `NoopTextSystem`；内核合成 glyph/cluster fixtures 与
virtual view tests 不是实际字体连字、fallback、DPI、平台 glyph raster 或原生
IME 的证据。真实字体与宿主 GUI 必须单独验收，不能把历史 prototype 测试数
或其它版本的原生结果视为本次生产接入的验证结果。

## 本片本地门禁

2026-10-06 的最终生产接入代码通过 workspace/all-targets 1523 项测试（107
binaries，其中 GPUI 402 项）、6 项 rustdoc 和 8 项 vendored XIM 测试，以及
strict Clippy、fmt、source-size、dependency-boundary、XIM provenance 检查。
本地没有安装 cargo-deny；该项须由 CI 独立验证。上述数字只记录自动检查，
不代表宿主接入、实际字体 face、GUI 操作或平台 IME 已验收。
