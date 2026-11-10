# 教程系列规划：从零手写一个视频剪辑 MCP（Quercus）

> 定位：面向会 Rust、想理解 MCP 与 NLE 引擎结合方式的中高级开发者。
> 协议层使用官方 `rmcp` SDK；教程聚焦“工具 + 引擎”，手写 JSON-RPC 只作为进阶附录。
> 产出：一套可运行的分章代码仓库 + 中文图文教程 + 一条真实成片。

## 1. 系列目标

读完并跟做后，读者能独立完成：

1. 把 `quercus-engine-rs`（crates.io 0.5.x）接进一个 Rust 进程；
2. 手写一个符合 MCP 2025-06-18 的 stdio 服务器（initialize/tools/resources/prompts）；
3. 用工具完成“建工程 → 导入素材 → 剪辑 → 效果 → 预览 → 导出 → 验收”；
4. 处理真实引擎坑：时间基、`sequence.length` 缓存、预览色彩、音轨、关键帧导出、
   工程持久化；
5. 写单元/协议/端到端测试，打包带 `$ORIGIN` rpath 的二进制并接入 MCP 客户端；
6. 完成一条 30 秒左右的真实短片（模型选镜头、调色、导出、QC）。

## 2. 目标读者与前置

- Rust：能读写 `serde`、`Result`、`Arc/Mutex` 级别代码；不要求 async。
- 视频：知道时间线/片段/轨道/转场/关键帧即可。
- 环境：Linux x86-64（macOS/Windows 作为附录），Rust 1.85+。
- 需要提前准备：Quercus 的 pinned FFmpeg 9.0 静态前缀、OpenColorIO 2.x。
- 不需要：CUDA、GPU、FFmpeg 源码、任何 MCP SDK。

## 3. 系列结构（6 部分 / 17 章）

| 章 | 标题 | 核心产出 | 预计篇幅 |
|---|---|---|---|
| 01 | 为什么给视频引擎写 MCP | 架构图 + 最终 demo | 1200 字 |
| 02 | 环境与最小验证 | `--check` 二进制 + MCP 客户端握手 | 1800 字 |
| 03 | 手写 JSON-RPC 2.0 / stdio | `rpc.rs` + 回车分隔帧 | 2200 字 |
| 04 | MCP 工具系统 | `tools/mod.rs` + 第一个工具 | 2200 字 |
| 05 | EditorSession 与工程 | 建/开/存 + 快照 | 2000 字 |
| 06 | 素材与时间线 | import/place/linked + timeline JSON | 2400 字 |
| 07 | 预览帧与色彩 | PNG image content + 显示变换 | 1800 字 |
| 08 | 异步导出 | 任务注册表 + ffprobe 验收 | 2200 字 |
| 09 | 剪辑操作 | trim/split/move/ripple + undo | 2400 字 |
| 10 | 转场与多轨 | seam/edge + 时长 + 类型现状 | 1800 字 |
| 11 | 效果链与关键帧 | effect/params/keyframes + 调色节点 | 2600 字 |
| 12 | 音频 | 链接音轨 + render_audio + 导出坑 | 2200 字 |
| 13 | 生产化健壮性 | 会话/任务/panic/日志/超时 | 2000 字 |
| 14 | 测试体系 | 单元 + 协议 + e2e + CI | 2200 字 |
| 15 | 打包与分发 | release + rpath + 客户端配置 | 1800 字 |
| 16 | Capstone 实战 | 30s 真实短片 + QC 报告 | 3000 字 |
| 17 | 进阶路线 | 资源/提示词/OFX/proxy/字幕/HTTP | 1800 字 |

节奏建议：每周 2 章，约 9 周；每章 30–60 分钟跟做。

## 4. 每章统一卡片

每章固定六段，方便系列化：

1. **本章目标**（3 条以内）
2. **前置检查点**（上一章 tag + 测试通过）
3. **新增代码**（文件 + 关键 diff，控制在 200 行内）
4. **验收**（可执行命令 + 预期输出）
5. **常见坑**（来自真实踩坑）
6. **练习**（1–2 个，带参考答案思路）

---

## 5. 分章设计

### Part 0：导论

**01 为什么给视频引擎写 MCP**
- 目标：讲清 MCP 的位置（LLM ↔ 工具 ↔ 引擎），以及为什么“剪辑”适合做成工具。
- 内容：stdio/JSON-RPC 架构图；oak-mcp 与 quercus-engine-rs 的边界；40+ 工具的取舍；
  “模型负责叙事/选镜头，引擎负责确定性编辑”的分工。
- Demo：播放最终 30s 成片 + 一次“建工程→导入→剪辑→导出”的录屏。
- 坑：不要把“分析原片”做成工具；模型看图就行。
- 练习：画出你能想到的 10 个剪辑工具，并标注只读/破坏性。

**02 环境与最小验证**
- 目标：跑通引擎，不碰 MCP。
- 内容：FFmpeg 9.0 pinned 构建与 `FFMPEG_DIR`；OCIO 2.x 与 ocio-sys bundled 的
  GCC 问题（`OCIO_INSTALL_DIR=/usr`、`OCIO_RS_LINK=dynamic`）；`cargo build`；
  `--check` 创建 session/sequence；MCP 客户端（Claude Desktop/Inspector）配置。
- 验收：`oak-mcp --check` 输出 `ok: true`、`render_manager: true`。
- 坑：`~/.cargo` 只读时用项目内 `CARGO_HOME`；dylib rpath；系统 FFmpeg 不兼容。
- 练习：把 `--check` 改成同时渲染一张 320×180 的 PNG。

### Part 1：协议

**03 用 rmcp 搭起 MCP 服务器**
- 内容：请求/通知/错误对象；`-32700/-32600/-32601/-32602/-32603`；批量；
  字符串 id；换行分隔；stdout 只能有协议、日志走 stderr；EOF 退出。
- 代码：`src/rpc.rs`（~150 行）+ `main` 读循环。
- 验收：管道发送 `initialize`/`ping`/非法 JSON，观察响应与退出码。
- 坑：一行 JSON 不能 pretty-print；通知不能回包；批量空数组。
- 练习：加 `--verbose` 把每个方法打到 stderr，不影响 stdout。

**04 MCP 工具系统**
- 内容：`initialize` 能力协商、`tools/list`、`tools/call`；inputSchema；
  annotations（readOnly/destructive/idempotent）；工具注册表；`Args` 类型化解析；
  `ToolReply` 文本/structuredContent/image；工具错误用 `isError` 而不是 JSON-RPC error。
- 代码：`tools/mod.rs` + `project_new`。
- 验收：`tools/list` 里有工具；调用 `project_new` 返回 session/sequence；
  未知工具得到 `-32602`。
- 坑：JSON 数字精度（NodeId 用字符串）；`arguments` 必须是对象；工具 panic 要捕获。
- 练习：给 `project_new` 加 `fps` 校验并写单测。

### Part 2：引擎基础

**05 EditorSession 与工程**
- 内容：`EditorSession::new/open/save`；`create_sequence`；`snapshot`；
  节点 id 是会话内 token；`Rational` 与帧率；**时间基方向**（帧时长 vs 帧率）；
  `sequence.length` 是缓存值，要用片段出点推导 `content_length`。
- 代码：`state.rs` 会话表 + `project_new/open/save`。
- 验收：建 960×540@25；保存后用 `project_open` 重开；信息一致。
- 坑：`CreateSequence` 的 width/height/rate 必须同时给；工程路径与 sidecar。
- 练习：给 `project_info` 增加 `content_length` 与轨道统计。

**06 素材与时间线**
- 内容：`media_import`（canonicalize 路径）；`PlaceFootageClip` 与
  `PlaceFootageClipsLinked`；track kind/index；秒→帧时间戳换算；
  timeline JSON 结构（tracks/clips/in/out/media_in）。
- 代码：`tools/media.rs`、`tools/clips.rs`（place/linked）。
- 验收：导入一条 4K50p 素材，放 0–3s，`timeline_get` 能看到 1 个视频 + 1 个音频片段。
- 坑：`in_ts/out_ts` 用序列帧率；linked 至少两个 placement；素材时长不足要 clamp。
- 练习：实现“自动按 3 秒均分放置 N 段”的工具。

**07 预览帧与色彩**
- 内容：`render::render_frame_f32`；PNG 编码；MCP image content（base64）；
  渲染输出是**工作色彩空间**，要用 `utils::working_to_display_target` 转到显示，
  否则预览偏暗（我们踩过：均值 56 vs 导出 118）。
- 代码：`image.rs` + `preview_frame`。
- 验收：同一时间点预览与导出的平均亮度接近（±3%）。
- 坑：尺寸上限/性能；非有限像素；alpha 处理。
- 练习：加 `thumbnail`（指定最小边）并复用预览逻辑。

**08 异步导出**
- 内容：`task::export` 与 `EncodingParams`；`video_time_base` 是**帧时长**
  （1/25，不是 25/1）；`export_length` 引擎未使用，长度从片段推导；
  Task 注册表；`task_wait` 超时；ffprobe 校验。
- 代码：`tools/render.rs`（export/task_*）。
- 验收：导出 5s 720p30 mp4，ffprobe 时长 5.00s、分辨率正确、有音频流。
- 坑：导出长度 0；音频 montage `stream_index` 的引擎 bug；`task_wait` 的 600s 上限。
- 练习：导出后自动跑 ffprobe 并返回结构化“验收报告”。

### Part 3：剪辑

**09 剪辑操作**
- 内容：trim/split/move/slip/slide/ripple delete；`AddTransitionAtSeam`；
  undo/redo 是进程全局；`undo_group_begin/end` 把多步合成一步。
- 代码：`clip_*` 工具 + `history` 工具。
- 验收：一段 6s 素材切两刀、裁掉中段、撤销/重做后时间线回到预期。
- 坑：split 的 primary 可能为空；undo 栈跨 session；删除后孤儿节点。
- 练习：实现 `clip_batch`（对多个片段执行同一操作）。

**10 转场与多轨**
- 内容：seam/edge 转场、half 参数、转场块与片段重叠；引擎的
  `transitions.rs` 有 Cross Dissolve/Fade/Wipe/Slide 四种 + `type_in`，
  但 SDK 命令还没暴露类型；多视频/音频轨组织。
- 代码：`transition_add`；演示双轨。
- 验收：两段之间 0.8s 交叉溶解，抽帧看到叠加。
- 坑：转场需要素材手柄；时长超过片段会失败；类型选择待引擎 API。
- 练习：给 `transition_add` 预留 `type` 参数（引擎就绪后接线）。

### Part 4：效果与音频

**11 效果链与关键帧**
- 内容：`effect_add`/`effect_chain`/`effect_params`/`effect_set_param`；
  NodeValue 类型（float/int/bool/rational/color/vec）；关键帧；
  调色节点（Color Correct/Grade/Three-Way/WB/Saturation/OCIO LUT）；
  **预览会评估关键帧、导出只送静态值**（引擎 bug）——教读者如何检测/规避。
- 代码：`tools/effects.rs`；一个“参考帧调色”小工具。
- 验收：静态 opacity 0.5 导出亮度减半；关键帧 0→1 在预览中生效（导出限制标注）。
- 坑：OCIO LUT 的 `lut_file_in` 是文本路径；参数类型不匹配会失败；效果名/标签。
- 练习：写 `grade_like(reference_frame)`：采样参考帧统计，给出 Color Correct 参数建议。

**12 音频**
- 内容：音频作为 footage 导入/放置；linked A/V；`render_audio`（峰值/RMS/WAV/gain）；
  音频效果只有 Volume/Pan（且当前混音未接入）；导出音频的引擎 bug 与 workaround
  （render_audio → WAV → A2 → 新进程导出）。
- 代码：`render_audio` 工具 + 音频工作流脚本。
- 验收：导出音轨有声音（-20 dB 左右）；能生成混音 WAV。
- 坑：`stream_index 0`；mute 不生效；WAV 导出失败；长 render_audio 后导出卡死。
- 练习：实现“旁白+音乐”双轨，并给出 ducking 的伪代码（等引擎支持）。

### Part 5：生产化

**13 健壮性**
- 内容：会话/任务注册表；错误分类（协议/参数/引擎/内部）；panic 捕获；
  stderr 日志；超时与取消；MCP `logging/setLevel`、`notifications/progress`；
  资源释放（Drop Task/Session）。
- 代码：`state.rs` 完善 + 错误类型。
- 验收：并发调用/未知 session/超大参数/中断客户端都不崩。
- 坑：panic 会污染 stdout；Drop 会 join 运行中的 Task；通知不能回包。
- 练习：给长导出加 progress 通知（每 5%）。

**14 测试体系**
- 内容：单测（rpc/convert/image）；协议集成测试（spawn 二进制、驱动 stdio）；
  端到端（ffprobe 校验、音频音量、抽帧）；畸形输入；CI。
- 代码：`tests/protocol.rs` + `scripts/e2e_*.py`。
- 验收：`cargo test` 全绿；e2e 产出可播放文件；CI 矩阵（debug/release）。
- 坑：测试进程共用引擎全局 undo；临时目录；大素材不要进仓库。
- 练习：加一个“导出失败”注入测试（只读目录/无效编码）。

**15 打包与分发**
- 内容：release 构建；`$ORIGIN`/`@loader_path` rpath；把
  `libquercus_engine.so` 放在可执行文件旁；MCP 客户端配置；
  版本/文档/许可证（GPL-3.0-or-later）；安全（路径/命令注入）。
- 代码：`build.rs` + 打包脚本 + 示例配置。
- 验收：在干净目录运行 release 二进制并通过 `--check`。
- 坑：`cargo install` 不带 dylib；系统 FFmpeg/OCIO 版本；glibc。
- 练习：打一个 tar.gz，写安装说明。

### Part 6：实战与进阶

**16 Capstone：30 秒短片**
- 目标：用真实素材完成一条可交付短片。
- 流程：模型抽帧看素材 → 定镜头表 → 建工程 → 剪辑 → 一级调色（参考帧对齐）
  → 预览检查 → 导出 → ffprobe/音量/抽帧 QC → 写制作报告。
- 交付：成片 mp4、`timeline_get` JSON、导出报告、复盘（哪些工具不够用）。
- 评分：结构 30%、调色 25%、音频 15%、技术正确性 20%、报告 10%。
- 说明：字幕/音乐后期作为“引擎能力待补”的扩展题。

**17 进阶路线**
- MCP：resources/prompts、completion、HTTP/SSE transport、多客户端、认证。
- 引擎：OFX 插件、proxy/multicam、字幕轨、音频后期（EQ/ducking）、调色示波器。
- 工程：MCP progress、日志、遥测、错误码、版本迁移。
- 结语：手写 vs `rmcp` 框架的取舍，何时该换。

---

## 6. 教程仓库与检查点

```
quercus-mcp-tutorial/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI + stdio 循环
│   ├── rpc.rs           # 第 3 章
│   ├── server.rs        # 第 4 章
│   ├── state.rs         # 第 5/13 章
│   ├── convert.rs       # 第 5/6 章
│   ├── image.rs         # 第 7 章
│   └── tools/           # 第 4–12 章
├── tests/protocol.rs    # 第 14 章
├── scripts/             # e2e、素材生成、打包
├── assets/              # 3 条自制测试素材（ffmpeg 生成）
└── docs/                # 每章 Markdown
```

- Git tag：`ch01` … `ch17`，每章一个可运行检查点。
- 每章 README 顶部：`git checkout chNN && cargo test && ./scripts/smoke.sh`。
- 素材用 ffmpeg 生成（`testsrc2` + `sine`，1080p/4K 各一条），避免版权/体积问题；
  Capstone 用用户自己的素材，不进仓库。

## 7. 素材与环境脚本

- `scripts/make-assets.sh`：生成 1080p30、4K50p、含音频/无音频各一条，10s。
- `scripts/setup-env.sh`：检查 `FFMPEG_DIR`、OCIO、dylib、MCP 客户端配置。
- `scripts/smoke.sh`：发 `initialize`/`tools/list`/`project_new`/`timeline_get`，
  断言 JSON 字段。
- 所有脚本在 CI 中跑一遍。

## 8. 发布计划

- 中文图文优先（Markdown/MDX），代码英文注释；后续可出英文版。
- 平台：GitHub 仓库 + 博客/公众号；可选 3–5 分钟 screencast（每章一个 demo）。
- 节奏：每周 2 章；第 1–4 章先发（协议+最小引擎），收集反馈再发后续。
- 每章末尾放“本章检查点 + 下一章预告 + 常见问题”。
- 第 16 章成片作为系列宣传素材。

## 9. 依赖与风险

- **引擎版本**：锁定 `quercus-engine-rs 0.5.x`；引擎 API 变动时更新检查点。
- **已知 bug**：音频导出、关键帧导出、`.ove` 持久化、长 render_audio 卡死。
  教程要明确“这是引擎现状 + workaround”，第 13/16 章专门讲。
- **字幕/音频后期**：等引擎补 API 后再加“扩展章”，不阻塞主线。
- **性能**：4K50p 在 debug 构建下约 5× 实时；第 15 章讲 release/proxy。
- **平台**：主线 Linux；macOS/Windows 放附录（rpath/DLL）。

## 10. 系列完成后的交付物

1. 可运行的分章仓库（17 个 tag，全部 `cargo test` 通过）；
2. 17 篇中文教程（约 3.5 万字）+ 代码 diff；
3. 3 条测试素材生成脚本；
4. 一条 30 秒真实成片 + QC 报告；
5. 一份“Quercus 引擎缺口/路线图”附录（复用 docs/engine-capabilities.md 与
   docs/engine-audio-subtitle-spec.md）。
