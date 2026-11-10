# oak-mcp / Quercus 能力缺口分析

对照对象：`media/shuili/参考成片.mp4`（38.17s、720p30、H.264+AAC、中文字幕）
与 `oak-mcp 0.1.0`（依赖 crates.io `quercus-engine-rs 0.7.0` / `quercus-engine 0.7.0` /
`quercus-task`、`quercus-node` 0.5.2）。

结论：**基础剪辑链路可用；要“接近原片”，缺的不是剪辑命令，而是调色、字幕、混音、
素材分析/匹配这几类工具，以及导出链路的几个引擎 bug。**

## 1. 参考片拆解

- 38.17s，约 18–22 个镜头：大门/标志石 → 红色指示牌/LED 屏 → 学生队伍行进 →
  “一泓清水北上”石碑与红马甲讲解员采访 → 展馆投影/展板/走廊 → 年轻讲解员采访 → 回到大门。
- 全程下方中文字幕（旁白式），音频是“人声 + 音乐床”。
- 画面明显经过调色（对比度、饱和度、亮度都高于原始素材）。

### 调色差距（同一场景抽帧，PIL 统计）

| 画面 | mean RGB | 亮度标准差（对比度） | 平均饱和度 |
|---|---:|---:|---:|
| 参考成片 0.5s（大门） | 128 / 134 / 140 | 95.8 | 0.185 |
| 素材 C0501 1s | 109 / 111 / 114 | 49.5 | 0.049 |
| 我们未调色的成片 1s | 117 / 119 / 119 | 44.9 | 0.042 |

原始 Sony 素材是低对比、低饱和的 Log 风格；参考片大约需要 **2 倍对比度、3–4 倍饱和度**
并整体提亮，属于“Log→709 + 风格化调色”，不是简单加一点 Contrast。

## 2. 能力矩阵

图例：✅ 有且可用；◐ 有但不完整/不暴露；❌ 没有。

| 需求 | 引擎 | MCP 暴露 | 缺口 |
|---|---|---|---|
| 多片段剪辑、切割、裁剪、滑移 | ✅ | ✅ | — |
| 滚动编辑 / Ripple 裁剪 / 区域删除 | ✅ | ❌ | MCP 缺命令 |
| 转场 | ◐ 只有默认 dissolve + 时长 | ◐ 只能加在接缝 | 引擎无转场类型/参数；MCP 无边缘转场 |
| 调色节点 | ✅ Color Correct、Grade、Three-Way、White Balance、Saturation、Gamma、OCIO LUT、OCIO Grading、Display Transform | ◐ 只能通过 `effect_add`/`effect_set_param` 手工拼 | MCP 无调色工具、无参数预设、无 LUT 管理 |
| 输入色彩变换（S-Log→709） | ◐ OCIO/LUT 节点在，但无自动识别/配置管理 | ◐ 需手工指定 LUT 文本参数 | 无 LUT 资产/色彩空间工具 |
| 示波器/直方图/矢量图 | ❌（只有音频波形） | ❌ | 引擎无视频分析 API |
| 自动调色匹配/参考片比对 | ❌ | ❌ | 无 |
| 文字/标题 | ✅ Text/Text3 + 341 个系统字体（`font_families()`） | ❌ | MCP 无文字工具、无字体管理 |
| 字幕轨/SRT | ◐ 有字幕轨类型、`SubtitleParams`、SRT 容器、导出 `subtitles_enabled` | ❌ | 引擎无“创建/导入字幕片段”命令；MCP 无字幕工具 |
| 音频混音 | ◐ Volume/Pan + 音频渲染 + 波形 | ◐ 导入/放置/`render_audio` | 无音量自动化/淡入淡出/ducking；关键帧导出失效 |
| 音乐/旁白/节拍同步 | ◐ 可导入音频；有波形提取 | ❌ 波形未暴露 | 无音乐库、无节拍检测、无对白对齐 |
| 多机位 | ✅ | ❌ | MCP 未暴露 |
| 代理（4K 性能） | ✅ `generate_proxy` | ❌ | MCP 未暴露；4K50p 调试构建导出约 5× 实时 |
| 剪辑板/复制粘贴/嵌套 | ✅ Copy/Paste/Nest | ❌ | MCP 未暴露 |
| 撤销分组 | ✅ `undo_group_begin/end` | ❌ | 复杂编辑一步一个撤销点 |
| 项目色彩/输出设置 | ✅ `state::color_*`、OCIO 配置 | ❌ | MCP 无项目设置工具 |
| 素材分析/镜头检测 | 模型职责 | `preview_frame` 可按需给帧 | 不是引擎缺口；批量抽帧只是效率便利 |
| 导出预设/质量 | ◐ 有格式/码率/像素格式 | ◐ 部分 | 无 CRF/预设；字幕开关未暴露 |
| 时间码/帧号 | ✅ 有理数/时间基 | ◐ 只暴露秒 | 帧级精确对位不便 |
| 保存/重开 | ❌ 丢格式/标记（见 bug 清单） | ◐ sidecar 兜底 | 引擎序列化缺失 |

## 3. 引擎能力缺口（不是 bug，是没有这个功能）

1. **视频分析/示波器缺失**：没有 histogram / waveform（视频）/ vectorscope / 色彩统计 API，
   只有音频波形。调色只能“盲调”，无法量化对比参考片。
2. **字幕没有创建/导入 API**：只有字幕轨类型、`SubtitleParams`、SRT 容器和导出开关；
   SDK 没有“放字幕片段/导入 SRT”的命令。
3. **转场只有默认 dissolve + 时长**：`AddTransitionAtSeam/AtEdge/Default`、`SetTransitionLength`；
   没有过渡类型（wipe/slide/…）或参数。
4. **音频后期很薄**：只有 Volume/Pan；没有 EQ、压缩、限制器、ducking、淡入淡出自动化
   （音量关键帧能用，但导出忽略关键帧，见 bug 清单）。
5. **色彩管理没有“输入变换/自动识别”**：OCIO/LUT 节点在，但没有“这条素材是 S-Log3，
   自动套 709”的流程，也没有 LUT 资产/配置管理。
6. **没有镜头检测/相似度/参考片比对**：无法自动找出“参考片第 3 秒对应哪条素材的哪一帧”。

## 4. 引擎 bug（详见上一轮清单）

- 导出音频 montage 硬编码 `stream_index: 0` → 视频内嵌音轨导出静音。
- 导出忽略 `track.muted`。
- 导出只送静态效果参数 → 关键帧在预览有效、导出无效。
- 长 `render_audio` 后同进程导出卡死（`repro/issue4-hang/`）。
- 视频短于音频 → `Failed to write frame`；纯音频导出必失败。
- `SequenceSnapshot::length` 缓存不更新。
- `.ove` 丢序列格式、标记、工作区。
- 导出忽略 `export_length_num/den`。
- `render_frame_f32` 返回工作空间像素，无显示变换说明。

## 5. MCP 工具缺口（引擎有，但没暴露）

按“复刻参考片”的优先级：

### P0（直接卡住“像原片”）

> 参考片的镜头分析由模型完成；MCP 只需要能按需取帧（`preview_frame`），
> 不需要引擎提供镜头检测。

1. **调色工具**
   - `color_grade`：对 clip/调整层设置 Color Correct（saturation/contrast/gamma/gain/offset）、
     White Balance（temperature/tint）、Three-Way（shadows/mid/highlights）。
   - `lut_apply`：OCIO LUT（`lut_file_in` 文本 + `lut_dir_in` combo）；支持 S-Log3→709。
   - `color_stats`：单帧/区间 RGB 均值、直方图、亮度对比度、饱和度，用于和参考片对齐。
2. **文字/字幕**
   - `text_add` / `text_place` / `text_set`（`CreateTextFootage`、`PlaceTextClip`、`SetNodeTextValue`）。
   - `fonts_list` / `fonts_add_dir`（引擎已有 341 个系统字体）。
   - 字幕：需要引擎先补“创建字幕片段/导入 SRT”的 API，MCP 才能包装。
3. **音频后期**
   - `audio_gain` / `audio_fade`（Volume 效果 + 关键帧，但依赖导出关键帧修复）。
   - `waveform`（引擎 `extract_waveform` 已有，MCP 未暴露）。
   - 导出必须修复音轨问题，否则音乐/旁白都白做。
4. **素材分析/镜头匹配**
   - `media_thumbnails` / `contact_sheet`：按时间抽帧拼图。
   - `shot_detect`：帧差/场景切换检测。
   - `frame_similarity`：把参考片抽帧与素材抽帧做感知哈希匹配（纯 MCP 侧可做）。
   - 这些是“自动剪得像原片”的基础。

### P1

5. **转场**：`transition_add` 增加 type 参数（需引擎先支持）；补 `transition_add_edge`。
6. **撤销分组**：`undo_group_begin/end`，让一条复杂编辑只占一个撤销点。
7. **代理**：`proxy_generate/status/delete`，4K50p 素材调试构建下导出约 5× 实时。
8. **时间码/帧号**：工具参数同时接受帧号/时间码，便于帧级对位。
9. **导出**：字幕开关、CRF/质量预设、音频单独导出（修 bug 后）。

### P2

10. **多机位**：`multicam_*`（引擎已有）。
11. **剪辑板/嵌套**：copy/paste/nest（引擎已有）。
12. **项目色彩设置**：working space / output spec / OCIO 配置。
13. **进度通知**：MCP `notifications/progress`，长导出目前只能轮询。

## 6. 对“剪得跟原片一样”的可行性判断

- **结构相似**：可行。需要全量素材 + 抽帧/相似度匹配；当前 MCP 没有分析工具，
  只能人工看缩略图，效率低。
- **调色相似**：引擎节点足够（Log 素材 + OCIO LUT/Color Correct/Grade/Three-Way），
  但 MCP 没有调色工具和示波器，只能盲调；且没有 S-Log 输入变换的资产/流程。
- **字幕/音乐**：字幕缺 API；音乐可导入但混音/ducking 与关键帧导出有问题。
- **成片质量**：即使剪/调到位，导出音频 bug 与卡死问题必须先修，否则无法交付。

## 7. 证据与复现

- 参考片抽帧：`media/shuili/ref_all/`、`media/shuili/ref_sheet_1..4.jpg`
- 调色统计脚本：见本文第 1 节（PIL 统计）。
- 效果参数转储：`target/capability_dump.py`（Color Correct/Grade/3-Way/WB/LUT/Volume/Pan 参数）
- 引擎 bug 复现：`repro/issue4-hang/`（卡死）、`target/diag_*.py`（音轨/静音/关键帧/WAV 导出）
- 成片：`media/shuili/oak_cut_audio.mp4`（基础剪辑 + workaround 音频）
