# P1 Single Block Native Input 进度

状态：进行中

本文档只记录 P1 的执行状态和验证证据。长期架构事实放在 `docs/architecture.md`，P1 设计放在 `design.md`，顶层路线以 `docs/planning.md` 为准。

## 状态说明

```text
[ ] 未开始
[~] 进行中
[x] 已完成
[!] 阻塞 / 需要决策
```

## 当前状态

当前切片：**P1.5 Copy/Paste 与基础 marks 已完成；Windows 实机 Gate 全通过**

当前分支：`feat/p1-clipboard-marks`

前置状态：P0 已完成（PR #13）；P1.0 / P1.1 / P1.2 / P1.3 已合并（PR #14–#17）。

## P1.0 Phase Contract 与阶段骨架

- [x] 创建 `docs/phases/p1-single-block-input/design.md`
- [x] 创建 `docs/phases/p1-single-block-input/progress.md`
- [x] 记录 P0 移交的 7 项前置依赖归属（见下方决策记录）
- [x] 运行 source-size 与 dependency-boundary guard
- [x] 运行 `cargo fmt --all -- --check`
- [x] 运行 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 运行 `cargo test --workspace --all-targets`

完成证据：

```text
分支 feat/p1-phase-contract（本地）：
uv run python tools/check_source_size.py 全绿
uv run python tools/check_dependency_boundaries.py 全绿
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿：18 个 test target，
xiaomu-core 95 个测试全部通过（含 inverse_roundtrip 随机不变量）。
本 PR 的远端 CI Success 即 P1.0 Gate 证据。
```

## P1.1 GPUI 依赖引入

- [x] workspace 依赖表 pin crates.io `gpui = "=0.2.2"`
- [x] `tools/check_dependency_boundaries.py` ALLOWED 表核对（xiaomu-gpui → core + runtime 已声明，无需修改）
- [x] cargo-deny 策略核对 / 必要豁免（新增 NCSA）
- [x] 编译级 smoke（`gpui_platform_layer_links`，不依赖窗口环境）
- [x] Cargo.lock 提交（依赖树约 700 包）
- [x] architecture.md 同步

实现说明：

```text
gpui 0.2.2 是 crates.io 当前最新版（2025-10-22 发布，Apache-2.0），
按 planning §17 以 "=0.2.2" 精确 pin 在 workspace.dependencies，升级走单独 PR。
cargo-deny licenses 需要新增 NCSA：libfuzzer-sys（`(MIT OR Apache-2.0) AND NCSA`）
经 gpui → image → ravif → rav1e 链进入依赖树；NCSA 为 permissive、OSI 批准。
macOS 本机构建需要 Xcode Metal Toolchain 组件（新版 Xcode 拆分下载；
`xcodebuild -downloadComponent MetalToolchain`），因为 gpui build script
在构建期用 xcrun metal 预编译 Metal shader——这是 gpui 自身的构建要求，
与是否打开窗口无关；不使用 runtime-shaders / macos-blade 特性绕过，
避免改变生产渲染路径。
xiaomu-gpui 新增编译级 smoke：px/Hsla 解析 + 链接验证，无窗口、无显示服务器依赖；
crate 文档同步为 GPUI adapter 定位描述。
传递依赖存在 future-incompat 提示（block、proc-macro-error2），不阻塞当前构建。
CI 适配：gpui 的 test 二进制在 Linux 链接期需要 libxcb / libxkbcommon / libxkbcommon-x11，
ubuntu job 增加 apt 安装 libxcb1-dev / libxkbcommon-dev / libxkbcommon-x11-dev；
首轮 CI 证实 macOS runner 自带 Metal Toolchain，ubuntu 链接失败是唯一真实失败
（windows 因 matrix fail-fast 被连带取消，非自身错误）。
```

完成证据：

```text
分支 feat/p1-gpui-dependency：
cargo deny check bans licenses sources 全绿（NCSA 豁免后）
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace --all-targets 全绿（含 xiaomu-gpui gpui_platform_layer_links）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
三平台 CI（ubuntu / windows / macos）以本 PR 远端运行为准。
```

## P1.2 DocumentSession（runtime 编排层）

- [x] DocumentSession：原子 apply + SelectionUpdate resolution + notification seam
- [x] EditIntent → EditPlan（Core transaction + after-selection policy）
- [x] HistoryStack：基础 undo / redo + before/after selection
- [x] SessionOutcome：document change / selection-only change / no-op 分流
- [x] session 单元测试（插入/替换后的 caret、Deleted 拒绝、no-op、redo 清空、undo/redo selection 恢复）

实现说明：

```text
session 编辑流：intent → EditPlan(Transaction + SelectionUpdate) →
apply_with_changes → resolve after-selection → 原子替换 snapshot / selection /
history 并通知。任何失败（Core 拒绝 / selection 映射 Deleted / 新 selection 校验失败）
session 状态保持不变；undo/redo 失败时 history entry 原位放回。
after-selection policy：InsertText → CaretAfterReplacement（range.start + 插入字节长）；
Backspace/Delete → CaretAtEditStart；ToggleMark 与 raw apply → MapExisting
（ChangeMap 向外映射，Deleted 即拒绝，无 fallback）。
no-op 判定在 intent 层：边界 Backspace / Delete / caret 移动、collapsed+空文本 InsertText、
collapsed ToggleMark 返回 NoChange（不调 Core、不推 revision、不发通知、不写 history）；
raw apply 无 no-op 检测，空 transaction 也提交（符合设计 §4.1）。
undo/redo 重放 AppliedTransaction::inverse()（ADR 0003）并直接恢复记录的
before/after selection；undo 后新编辑清空 redo 栈。
caret 移动 / 删除按 Unicode scalar boundary（previous/next boundary 辅助函数），
Home / End 到 paragraph 逻辑首尾；combining sequence 只承诺 offset 合法不 panic。
ToggleMark：选区整体已带该 mark kind 才 RemoveMark，否则 AddMark；collapsed 无 pending-mark。
Core 最小扩展：InlineContent::offset_at（boundary 校验的 offset 构造，
与 TextBuffer::offset_at 对齐；runtime 不再需要重组拼接文本来构造合法 offset）。
```

完成证据：

```text
分支 feat/p1-document-session：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿（runtime 加入 missing_docs 门禁）
cargo test --workspace 全绿（新增 tests/session.rs 16 个集成测试 + intent 边界单元测试）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
cargo deny check bans licenses sources 全绿
```

## P1.3 GPUI 单块编辑基础

- [x] App / window 装配（`editor::run_single_block_editor`，含关窗退出）
- [x] 单 Paragraph block view（渲染 + caret / selection 绘制，marks 基础视觉映射）
- [x] InputHandler 文本输入 + 命令键 → intent 管线
- [x] Left / Right、Shift 选择、Paragraph Home / End、Backspace / Delete（含 SelectAll / Undo / Redo 绑定）
- [x] 最小 hit-test（点击 / 拖拽 → boundary 校验的 offset）
- [x] editor_harness 接入单段编辑器
- [x] Windows 实机键盘编辑闭环（英文直输 / 方向键 / 选择 / Home/End / Backspace/Delete / undo-redo 可用；中文 IME 表现与 macOS 相同的已知停损限制，归因同一条，修复属 P1.4）

实现说明：

```text
block_view/ParagraphView 持有 DocumentSession，键 / 鼠标全部翻译为 runtime EditIntent，
view 不直接修改文档；EntityInputHandler 的平台 UTF-16 range 一律经 input/utf16 转换
（surrogate 中点解析到所在字符边界，始终为合法 Core 坐标）。
ParagraphElement 在 paint 期注册 handle_input、shape_line 单行渲染、绘制 caret/selection，
并保存 layout/bounds 供 hit-test（closest_index_for_x → InlineContent::offset_at 校验）。
runtime 新增 PlaceCaret intent（绝对定位，hit-test 用）：选择类操作不产生 transaction、
不进 history；replace_text_in_range 的显式范围 = 两次 PlaceCaret + 一次 InsertText，
保持单条 undo 记录。
Left/Right 在有选区时先折叠到选区起 / 终点（编辑器惯例）；SelectAll = PlaceCaret(0) + extend 到段尾。
Undo/Redo 同时绑定 cmd-z / ctrl-z / cmd-shift-z / ctrl-shift-z / ctrl-y。
P1.3 停损项：replace_and_mark_text_in_range 立即按普通输入提交（无 composition 状态，
P1.4 落地状态机；当前 macOS IME 输入中文会逐字提交拼音字母，属已知限制）。
marks 视觉映射：Bold/Italic → 字重 / 字形，Underline/Strike → 装饰线；Code/Link 留 P1.5。
```

完成证据：

```text
分支 feat/p1-single-block-basics：
cargo fmt / clippy -D warnings / cargo test --workspace / 两个 guard 全绿
（新增 input/utf16 5 个单元测试、runtime PlaceCaret 集成测试）
本地 macOS 实机冒烟（人工执行 `cargo run -p xiaomu-editor-harness`）：
窗口正常打开、进程稳定、关窗即退出（exit 0）；
英文直接输入 / 方向键 / Shift 选择 / Home/End / Backspace / Delete / 撤销重做可用；
开着中文 IME 时发现两个问题（归因 P1.3 停损项，无 composition 状态）：
1. 连续输入拼音出现字符粘连（nihao → nnihhao 一类重复）；
2. 候选词提交后中文出现，但拼音字母残留在文档中。
两者均为 setMarkedText 路径被当作普通输入立即提交、且从不报告 marked range 所致，
属 P1.4 CompositionState 的修复范围（见 Regression Log）。
Windows 实机 Gate（键盘编辑闭环手动清单）待执行后补充证据。
```

## P1.4 IME Composition

- [x] UTF-16 range → TextOffset 转换层（P1.3 已建，本切片接入全部查询路径）
- [x] `CompositionState` + virtual text projection（`input/composition.rs` 纯状态机）
- [x] selected / marked / text / bounds 查询与连续 preedit update
- [x] begin / update / commit / cancel / focus-loss 状态转移测试
- [x] marked text 瞬态渲染（下划线 preedit，不写 canonical document）
- [x] commit 一次入历史 / cancel 恢复 composition 前状态
- [x] planning §8 Windows 矩阵实机执行（Microsoft Pinyin 连续 composition、候选窗、中文标点、中英混排、emoji / surrogate、combining marks、选区替换、焦点恢复）

实现说明：

```text
CompositionState（纯逻辑，无窗口依赖）：base_selection + base_range + preedit +
preedit_selected_utf16；project() 生成 virtual projection。
平台 callback 映射（针对 pin 的 gpui 0.2.2 核对 crates.io 源码）：
  macOS   setMarkedText → begin_or_update；insertText → commit；unmarkText → cancel/idle no-op
  Windows GCS_COMPSTR → begin_or_update；GCS_RESULTSTR → commit（非空）；
          WM_IME_COMPOSITION lparam==0 → 以空文本 replace_text_in_range 到达 → 按 cancel 处理
空 commit 即 cancel：真实 IME 结果串永不为空，且空插入会误删 base 选区。
InputHandler 全部查询（text_for_range / selected_text_range / marked_text_range /
bounds_for_range / character_index_for_point）改答 virtual projection；
composition 全程 document revision 不变（preedit 只存在于 adapter）。
commit 路径：两次 PlaceCaret（base_range）+ InsertText = 单笔 transaction、单条 undo。
cancel 路径：PlaceCaret 恢复 base_selection；焦点丢失经 window.on_focus_out 订阅取消。
Windows IME 可在 focus-out 送达前先提交普通无下划线文本；该原生平台结果可接受，
Gate 要求是最终不存在带下划线的僵尸 composition，且恢复焦点后可继续输入。
composing 期间键盘编辑动作被忽略（防止 canonical 编辑破坏 base range），鼠标点击先 cancel。
渲染：preedit 作为独立 underlined segment 参与排版，caret 定位到 preedit 内平台 selection 终点。
block_view/mod.rs 超 source-size 700 行硬限，EntityInputHandler impl 拆分至 block_view/input_handler.rs。
```

评审与实机调试补充（Windows，用户实测，已关闭）：

```text
原现象：preedit 状态机/查询/渲染数据全部正确（临时探针证实 prepaint/paint
都拿到含拼音的 virtual projection），但屏幕冻结；切窗口后 IME commit 才追上。
排查：GPUI_DISABLE_DIRECT_COMPOSITION=1 切换呈现路径后依旧，DComp 不是充分解释。
首次 begin 缺少 cx.notify() 是已确认并保留的晓木局部 bug 修复。
gpui 0.2.2 的 dispatch_key_event 可在 dirty 时执行 draw 而不 present，与上游
zed-industries/zed #61469 描述的 Windows presentation starvation 机制吻合，但不是
本次 preedit 不可见的直接根因。
失败实验：在 InputHandler/App update 尚未退出时同步调用 RedrawWindow(
RDW_INVALIDATE | RDW_UPDATENOW)，日志仍显示 prepaint/paint 而屏幕不更新。该调用可能
重入 GPUI request-frame，不能等同于回调外 watchdog；workaround 已撤回。
unsafe 政策恢复为 xiaomu-gpui crate-wide #![forbid(unsafe_code)]。
PresentMon 实机采集约 17.6 秒 / 1613 次 swap-chain present，包含 4.18s 等断档；
该数据保留为 GPUI 调度风险证据，但当时没有与 IME 日志严格对齐，不能单独证明因果。
随后在 gpui 0.2.2 Windows dispatch eager draw 后增加 present，实机现象完全不变，
故该补丁实验撤回，不 vendor GPUI，继续使用精确 pin 的 crates.io 0.2.2。
确定根因：collapsed composition 的 base_start == base_end；旧 projection mapper 对该
边界同时使用 prefix/suffix 语义，使 suffix 与 preedit 获得相同排序坐标且 suffix 先拼接。
结果是 preedit 被追加到整行末尾（长行已被 viewport 裁剪），caret 却按正确的 virtual
offset 在原插入点移动。时间戳日志直接显示 canonical 中间插入点对应的 prepaint 文本
仍以 `.n/.ni/.ni'hao` 结尾，闭合了这个矛盾。
修复：projection 显式构造 prefix / preedit / suffix；suffix 的 display start 始终加上
preedit 长度，不再用一个无 bias 的边界 mapper。新增 collapsed caret、跨 styled runs
替换与 idle style 保真回归测试。
修复后 Windows 实机复测通过：微软拼音在正文中间与末尾连续 composition 时，preedit
逐键实时显示并带下划线，caret 跟随；候选“你好”提交后在原插入点整体替换。时间戳
日志同步证明 prepaint 的 virtual text 为 prefix + preedit + suffix，不再追加到行末。
扩展矩阵通过：选区替换与单笔 undo/redo、中文标点、中英混排、emoji / surrogate、
combining marks 均正常；focus-out 若由 Windows IME 先提交则留下普通无下划线文本，
不会残留 marked range，恢复焦点后可继续输入。临时日志与 PresentMon 脚本合并前删除。
```

完成证据：

```text
分支 feat/p1-ime-composition：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace 全绿（新增 input/composition.rs 6 个状态机单元测试，19 个 test target）
tools/check_source_size.py 与 tools/check_dependency_boundaries.py 全绿
Windows 本机启动冒烟：harness 窗口正常打开、进程稳定（自动验证无窗口依赖部分全绿）。
Windows Microsoft Pinyin 手动矩阵全通过（连续 composition、候选提交、中文标点、
中英混排、emoji / surrogate、combining marks、选区替换、单笔 undo/redo、焦点恢复）。
P1.4 自动 Gate 与实机 Gate 均满足，可以合并。
```

## P1.5 Copy/Paste 与基础 marks

状态：进行中

- [x] runtime 纯文本 clipboard seam：`TextClipboard` trait + `normalize_paste_text`
- [x] `DocumentSession::selected_text`（跨 run 选区提取纯文本，collapsed 返回 None）
- [x] GPUI 平台绑定 `PlatformClipboard`（非文本剪贴板内容读为 None）
- [x] Copy / Cut / Paste 动作 + macOS / Windows 双键位绑定
- [x] Bold / Italic / Code / Underline / Strike 切换键位（复用 ToggleMark intent）
- [x] Code mark 视觉映射（半透明背景色块）；Link 留待后续（需属性编辑 UI）
- [x] Windows 实机 Gate：中英文复制粘贴 + mark 切换 + undo/redo 闭环
- [x] 实机回归修复：Esc 取消 composition 后键盘编辑被锁死（见 Regression Log）

实现说明：

```text
seam 形态：xiaomu-runtime::clipboard 只定义 trait 与归一化策略，不接触平台 API；
gxui 侧 PlatformClipboard<'a>(&'a App) 实现 trait，生命周期限于动作处理调用内。
copy/cut 取 session 选区纯文本（selected_text 按 run 重叠拼接，marks 不参与）；
paste 读入文本经 normalize_paste_text（CRLF/CR/LF → 单个空格，paragraph inline
text 不能含换行；多 block 粘贴语义留 P2+），空文本直接忽略、不清除选区。
cut = copy + EditIntent::Delete（非折叠选区整段删除），paste = InsertText intent，
mark 切换 = 复用 P1.2 的 ToggleMark intent（全带则移除否则添加，MapExisting 保选区）：
三者各为一笔 transaction、一条 history entry，满足“paste / mark 各为一个 undo 单元”。
composition 期间全部 clipboard / mark 动作被忽略（与既有编辑动作同一停损策略）。
Code 视觉映射用 TextRun.background_color 半透明色块而非换字体族：避免跨平台
monospace 字体名解析差异，且保持选区高亮可见。action 命名 Clipboard* 避免
与 std::marker::Copy 撞名。
block_view/actions.rs 从 mod.rs 分出（mod.rs 已在 review 区间，新增处理逻辑独立成文件）。
```

完成证据（自动部分）：

```text
分支 feat/p1-clipboard-marks：
cargo fmt --all -- --check 全绿
cargo clippy --workspace --all-targets -- -D warnings 全绿
cargo test --workspace 全绿（新增 clipboard 归一化 2 个单元测试 +
session selected_text 2 个集成测试，19 个 test target）
tools/check_source_size.py ok（block_view/mod.rs 544 行，review 区间提示）
tools/check_dependency_boundaries.py ok
```

实机验证（Windows，用户实测，已关闭）：

```text
中英文复制粘贴闭环正常；mark 切换（含 undo/redo 单元语义）正常。
实测中发现并当场修复一个 P1.4 回归（Esc 取消 composition 后方向键失效，
根因与修复见 Regression Log），修复后 Esc 无需点击即可继续键盘编辑、
重新输入拼音从头组词、光标回到输入前位置。P1.5 自动 Gate 与实机 Gate 均满足，可以合并。
```

## P0 移交的 P1 前置依赖与归属

P0.7 在 progress.md 记录了 7 项"进入 P1 前需要明确归属"的依赖，处置如下：

```text
1. Transaction 未携带 before/after selection 与 history_group
   → 归属 runtime：DocumentSession 持有 selection 与 history 栈，
     EditIntent 通过 runtime SelectionUpdate 明确 after-selection，普通 apply 才使用 ChangeMap fallback；
     Core contract 不动。
2. history 栈 / typing coalescing / undo selection 恢复
   → P1 在 runtime HistoryStack 实现基础 undo/redo 与 selection 恢复；
     undo = 重放 AppliedTransaction::inverse()（ADR 0003）；typing coalescing 留到 P3。
3. TextSelection 限单个 inline node 内
   → 维持；跨 block selection 属 P2 document selection。
4. SplitNode / JoinNodes / MoveNode / list / InlineAtom steps 未实现
   → 维持 P2/P4 范围；P1 单 block 内容编辑只使用 ReplaceText / AddMark / RemoveMark，
     不允许删除当前 inline node。
5. Grapheme-cluster 光标移动与视觉 affinity resolution
   → 属 frontend；P1 的 Left / Right / Backspace / Delete 按 Unicode scalar boundary，
     Home / End 到 Paragraph 逻辑首尾；视觉行导航与 grapheme 留作后续增强（ADR 0001 边界）。
6. GPUI 依赖尚未引入
   → P1.1 单独 PR 从 crates.io pin 精确版本（planning §17）。
7. commands.rs / history.rs 占位模块
   → command 行为与 history 编排按 planning §9 在 runtime 层落地；
     Core 占位模块保持不动，待 Core 真需要语义支持时再启用。
```

## 决策记录

这里只记录影响 P1 执行的决定。长期且难逆转的架构理由应进入 ADR。

### 2026-08-24（P1.5）

- clipboard seam 定为 runtime `TextClipboard` trait（write_text / read_text），平台绑定在 GPUI adapter；不引入通用 ClipboardService registry（宿主 capability service 属后续 Host Contract 阶段）。
- 粘贴文本中的行断符折叠为单个空格：paragraph inline text 不能含换行，多 block 粘贴语义留到 P2+ document-level 编辑；空剪贴板文本不清除选区。
- GPUI action 命名用 `ClipboardCopy` / `ClipboardCut` / `ClipboardPaste`，避免与 `std::marker::Copy` 撞名。
- Code mark 视觉映射用半透明背景色块而非 monospace 字体族切换：跨平台字体名解析差异大，且色块不影响选区高亮可见性；Link 留待有属性编辑 UI 的切片。
- cut = copy + Delete intent、paste = InsertText intent、mark 切换复用 ToggleMark：各为一笔 transaction、一条 history entry，无需新增 undo 机制。
- composition 期间忽略全部 clipboard / mark 动作，沿用 P1.4 停损策略（canonical 编辑会破坏 base range）。

### 2026-08-23（P1.0）

- selection 状态与 undo/redo 栈在 runtime 侧编排：新建 `DocumentSession`（xiaomu-runtime）持有 selection 与 history；每个 EditIntent 产出 runtime `EditPlan { transaction, selection_update }`，由 intent-specific `SelectionUpdate` 决定 after-selection，不扩展 Core Transaction contract（符合 planning §9、ADR 0003、P0.3 评审结论）。
- P1 不定义结构删除后的 selection fallback：任何导致当前 `TextSelection` 映射为 `Deleted` 的 transaction 都原子失败；父级 `NodeGap` / 邻近 block 收敛留到 P2 document-level selection。
- GPUI 从 crates.io 引入并以精确版本 pin（`gpui = "=x.y.z"`），升级走单独 PR；不使用 git revision 依赖。
- undo 实现为会话内重放 `AppliedTransaction::inverse()`，round-trip 要求 store 完全相等；history entry 保存 before/after selection，恢复时直接校验并使用对应 selection；typing coalescing / history grouping 留到 P3。
- caret 移动 P1 按 Unicode scalar boundary；Home / End 到 Paragraph 逻辑首尾；视觉行导航和 grapheme cluster 光标留作后续增强。
- IME `CompositionState` 位于 GPUI adapter，维护 base selection、preedit、preedit selection 与 virtual projection；composition 全程不写 canonical document，commit 单笔入历史，cancel 恢复 composition 前状态。
- 合法空操作返回 `SessionOutcome::NoChange`，不调用 Core apply，不增加 revision / notification / history。
- P1 文档沿用 P0 惯例：阶段文档中文，代码标识 / API 名 / 公开 rustdoc 用英文。

### 2026-08-22（P1.1）

- GPUI 采用 crates.io 发布版而非 git revision：`gpui = "=0.2.2"`（crates.io 当前最新），精确 pin 在 workspace.dependencies；升级只走单独 PR。
- cargo-deny licenses 新增 NCSA 豁免：来源是 libfuzzer-sys（libFuzzer 绑定）经 image → ravif → rav1e 的必然传递链，无法通过 feature 裁剪；NCSA 为 permissive、OSI 批准。
- macOS 构建 gpui 需要 Xcode Metal Toolchain 组件（build script 构建期预编译 Metal shader）。不采用 gpui 的 runtime-shaders / macos-blade 特性绕过，避免改变生产渲染路径；本地环境用 `xcodebuild -downloadComponent MetalToolchain` 一次性解决。
- xiaomu-gpui 的 P1.1 交付是编译级 smoke（px / Hsla 解析 + 链接验证），不引入窗口依赖；gpui 类型暂不进入任何公开 API。

### 2026-08-22（P1.2）

- Core 最小扩展 `InlineContent::offset_at`：runtime 需要 boundary 校验的 offset 构造（caret 移动 / after-selection resolution），此前只能在 runtime 侧重组拼接文本绕行；与 `TextBuffer::offset_at` 语义对齐。
- no-op 判定只在 intent 层；`DocumentSession::apply`（raw transaction）无 no-op 检测，空 transaction 正常提交并推进 revision，与设计 §4.1 一致。
- 删除当前 inline node 的 transaction 映射为 `Deleted` 时原子失败（`SessionError::SelectionDeleted`），P1 不做父级 NodeGap / 邻近 block 收敛（P2 document selection 统一定义）。
- ToggleMark 语义：选区内所有重叠 piece 均带该 mark kind 才移除，否则整体添加；collapsed selection P1 无 pending-mark，返回 NoChange。
- undo/redo 失败路径：history entry 原位放回（restore_undo / restore_redo），session document / selection 不受影响。
- runtime 加入 `#![warn(missing_docs)]`，与 core 的 rustdoc 门禁一致。

### 2026-08-22（P1.3）

- runtime 新增 `PlaceCaret` intent（绝对 offset 定位，hit-test / 编程式移动用）：选择类操作不产生 transaction、不进 history；显式范围的文本替换用"两次 PlaceCaret + 一次 InsertText"保持单条 undo 记录。
- UTF-16 转换集中在 `xiaomu-gpui::input::utf16`：surrogate 中点解析到所在字符边界，转换结果始终是合法 Core 坐标；平台 UTF-16 不泄漏出 adapter。
- Left/Right 在非空选区时先折叠到选区起 / 终点（编辑器惯例），再按 scalar boundary 移动。
- P1.3 停损：`replace_and_mark_text_in_range` 立即按普通输入提交（无 composition 状态）；macOS 上 IME 中文输入会逐字提交拼音字母，属已知限制，P1.4 用 CompositionState 状态机修正。
- GPUI 官方 `examples/input.rs`（随 crates.io 包发布）作为 InputHandler / 自定义 Element 模式的参考实现。
- 关窗退出：`on_window_closed` + `windows().is_empty()` 时 `quit()`（GPUI 默认关窗不退出）。

## P1 Phase Gate

P1 只有在以下条件全部满足后才能完成：

- [x] GPUI 依赖 pinned 引入，dependency-boundary guard 全绿
- [x] DocumentSession 编排 snapshot / selection / history，无绕过路径
- [x] selection 在任何公开读取点针对当前 snapshot 合法
- [x] undo / redo 恢复精确 store 状态与合理 selection
- [x] intent-specific SelectionUpdate 正确处理插入、替换、删除与 mark
- [x] Deleted update 原子失败；no-op 不增加 revision / notification / history
- [x] undo 后新编辑清空 redo
- [ ] IME composition 不触碰 canonical document，commit / cancel 语义正确
- [ ] 单 block 键盘编辑 + hit-test + clipboard + 基础 marks 实机可用
- [ ] 真实 IME（Microsoft Pinyin）+ selection + undo 手动 Gate 通过
- [x] session 纯逻辑测试在 CI 无显示器环境全绿
- [ ] 架构文档与实现一致
- [ ] P1 最终 `CI Success` 全绿

## Regression Log

- 2026-08-24（P1.5 实机，Windows）：拼音 composition 中按 Esc 取消后，preedit 正确消失，但方向键 / 键盘编辑失效，需再点击一次才恢复。
  根因：微软拼音的取消以空串 GCS_COMPSTR 到达 marked-text 路径（gpui 0.2.2 events.rs L674），旧实现把它当作普通 preedit update，CompositionState 残留为 composing 状态，所有编辑动作被 composing 停损分支吞掉；点击因 on_mouse_down 显式 cancel 才恢复。
  修复：新增 resolve_preedit_update 决策——活跃 composition 收到空 marked text 一律视为取消并恢复 base selection；空 payload 也不能启动新 composition。决策逻辑为纯函数并有单元测试；实机验收：Esc 后无需点击即可继续键盘编辑与再次输入。

- 2026-08-22（P1.3 实机）：macOS 开中文 IME 输入时拼音字符粘连、候选提交后拼音残留。
  根因是 P1.3 停损实现把 `replace_and_mark_text_in_range`（setMarkedText）立即按普通输入提交，
  且 `marked_text_range` 恒返回 None，平台无法跟踪 preedit 范围导致反复误替换。
  非停损路径（英文直输 / 键盘命令）不受影响。修复属 P1.4 CompositionState 状态机，
  修复时以本条为回归验收（连续拼音不粘连、提交后 preedit 清除）。
