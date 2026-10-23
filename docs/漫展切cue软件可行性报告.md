# 漫展切 cue 开源软件 — 开发可行性调研报告

> 调研时间：2026-10-21
> 目标产品：面向漫展舞台的开源切 cue 软件，三大核心功能为「暖场音乐播放、链接自由播放、正式舞台时间线编排」，辅以平台媒体抓取、软件内 PPT 播放、Syphon/NDI 输出。

---

## 一、总体结论

**整体可行性：可行（技术无硬障碍），但有两个「高建议降级」的需求。**

| 需求 | 可行性 | 一句话结论 |
|---|---|---|
| 暖场音乐播放 | ✅ 高 | 成熟音频引擎即可，无难点 |
| 链接自由播放（即点即播） | ✅ 高 | 对应行业通用 Cart/触屏格布局，有开源蓝本 |
| 正式舞台时间线编排 | ✅ 高 | cue 状态机模型有 QLab/LiSP 双份成熟设计可借鉴 |
| B 站视频抓取 | 🟡 中高 | BBDown 生态成熟，扫码登录可用；批量高频有限流风险 |
| 抖音视频抓取 | 🟡 中 | 游客态抓公开视频可行；登录态风控强、签名维护成本高 |
| 小红书视频抓取 | 🔴 中低 | 风控最严、签名更新快、封号风险最高，建议做可选插件 |
| 网易云歌单/歌曲抓取 | 🔴 中（法律风险明确） | 原 NeteaseCloudMusicApi 已因律师函删库，不建议做核心依赖 |
| 软件内播放 PPT（双窗控制） | ✅ 高（有取舍） | 转图片/PDF 双窗播放最稳；动画保真需依赖系统 Office |
| Syphon 输出 | ✅ 高 | BSD 开源协议，绑定库成熟，零许可障碍 |
| NDI 输出 | 🟡 高（有合规约束） | 技术成熟，但运行时专有，须引导用户自装 NDI Runtime |

**市场空位真实存在**：目前开源世界没有一款「跨平台 + 视频 cue + 双窗口输出」的舞台播放软件。QLab、Mitti、PVP 均为 macOS 商业软件；开源的 Linux Show Player（LiSP）仅支持 Linux 且主打音频，社区讨论中连视频 cue 都靠外挂 mpv 实现。这个空位正是本项目的机会。

---

## 二、三大核心功能的技术方案

### 2.1 暖场音乐播放

- 实现简单：音频引擎可选 miniaudio / NAudio / JUCE，或直接复用 libmpv/libvlc 的音频通路。
- 建议功能形态：独立播放列表 + 循环/随机 + 淡出到正式 cue（crossfade）。
- 开源参考：[liveplay](https://github.com/tdoukinitsas/liveplay)（Electron + C++ 音频核心，AGPL-3.0，跨平台）、[simplecue](https://github.com/stoatworks-labs/simplecue)（JUCE，MIT）。

### 2.2 链接自由播放（即点即播）

- 对应 LiSP 的 **Cart 布局**：触屏按钮格，每个格子绑定一个媒体（视频/音频/链接解析后的内容），点即播、再点即停，互不干扰。
- 「链接播放」即粘贴 B 站/抖音链接 → 后台解析/下载 → 入 cue。技术链路见第四节。
- 参考：[LiSP Cart layout 文档](https://linux-show-player-users.readthedocs.io/en/latest/list_layout.html)。

### 2.3 正式舞台时间线编排（cue list）

这是产品的核心，好在有完整成熟的设计模型可直接借鉴：

- **cue 三段时序**：每条 cue = pre-wait → action(duration) → post-wait。
- **自动跟进链**：auto-continue（触发后经 post-wait 启动下一条）/ auto-follow（当前 cue 播完后启动下一条）。
- **Fade cue**：独立 cue 类型，对运行中的 cue 做音量/透明度渐变（duration + curve）。
- **cue 状态机**：Stop / Pre-wait / Running / Post-wait / Paused / Error，动作含 Start/Stop/Pause/Resume 及各自的 faded 变体。
- **交互范式**：顶部大 GO 键 + playhead 推进 + 右侧运行中 cue 面板——QLab、LiSP、QPlayer 均如此，用户习惯已固化，直接沿用。
- 参考：[QLab 文档](https://qlab.app/docs/v3/general/getting-started/)、[LiSP cue 文档](https://linux-show-player-users.readthedocs.io/en/latest/cues/index.html)。

**结论：cue 逻辑实现难度低，真正的工程难点在视频渲染的现场稳定性。**

---

## 三、技术栈选型建议

| 层 | 推荐 | 备选 | 说明 |
|---|---|---|---|
| UI 壳 | **Tauri**（Rust + WebView） | Electron | Tauri 包体 <10MB、内存约为 Electron 1/4；若团队不熟 Rust 或需大量 Node 生态（node-syphon 等），选 Electron 更省事 |
| 视频内核 | **libmpv**（GPL-2.0+） | libvlc（LGPL）、GStreamer（LGPL） | libmpv 性能口碑最佳，render API 可嵌入自研 OpenGL/Metal 表面；项目若定 GPL-3.0 则无冲突；想留宽松许可余地选 libvlc/GStreamer 动态链接 |
| 音频内核 | 复用 libmpv / miniaudio | JUCE | 暖场音乐与视频共通路可简化架构 |
| 多窗口 | 框架原生多窗口 | — | 主控窗口 + 舞台输出窗口（全屏绑定指定显示器）；应急方案：spawn `mpv --screen=1 --fs` 独立进程投第二屏（LiSP 社区已验证可行） |
| 开源协议 | GPL-3.0 | — | 对齐 LiSP/QPlayer 的社区惯例，同时规避 libmpv 许可问题 |

**架构要点：控制 UI 与播放渲染必须解耦。** QLab/Mitti 均把输出 surface 作为独立渲染目标，NDI/Syphon/第二屏只是不同的「输出口」。

---

## 四、平台媒体抓取：分平台可行性

### 4.1 B 站 —— 可行性最高，建议作为首发支持

- **[BBDown](https://github.com/nilaoda/BBDown)**：专用下载器，支持扫码登录（`BBDown login`）、SESSDATA cookie、TV/APP token；普通账号登录可拿 1080P，大会员可到 4K/8K/HDR。
- **[bilibili-api (Nemo2011)](https://github.com/Nemo2011/bilibili-api)**：Python 库，持续维护，覆盖登录与非公开接口。
- 风险（中）：高频抓取会触发 -412 风控限流；分发大会员清晰度内容有版权风险。

### 4.2 抖音 —— 可行但建议走游客态

- **[f2 (Johnserf-Seed)](https://github.com/Johnserf-Seed/f2)**：开源 a_bogus 签名，登录态可抓收藏/点赞。
- **[Douyin_TikTok_Download_API (Evil0ctal)](https://github.com/Evil0ctal/Douyin_TikTok_Download_API)**：自托管 REST API，支持免登录游客身份抓取 + 无水印下载。
- yt-dlp 对抖音支持不稳定，随反爬升级反复失效。
- 风险（高）：滑块/设备指纹/行为检测，登录 cookie 抓取易封号；建议只抓公开视频、走游客态。

### 4.3 小红书 —— 风控最严，建议仅做可选插件

- **[MediaCrawler (NanmiCoder)](https://github.com/NanmiCoder/MediaCrawler)**（5.2 万 star）：扫码/手机号/cookie 登录，可下载笔记视频/图文。
- **[xhs (reajason)](https://pypi.org/project/xhs/)**：Web 接口封装，内置 X-S 签名。
- 风险（高，四平台最高）：官方文档明示「小红书风控非常严格」，需 CDP 连接真实浏览器降低滑块概率；X-S 签名更新极快，维护成本高。

### 4.4 网易云音乐 —— ⚠️ 法律风险最明确，不建议做核心依赖

- **关键先例：原版 NeteaseCloudMusicApi（Binaryify）2024 年 1 月被网易发律师函后删库停更**，指控涉及侵犯著作权、破坏计算机信息系统、不正当竞争。这是本项目最直接的风险警示。
- 社区续作 [api-enhanced](https://github.com/neteasecloudmusicapienhanced/api-enhanced) 仍可用：手机验证码/二维码登录、歌单抓取、`/song/url/v1` 取播放 URL（CDN mp3/flac，可直接流媒体播放，但 URL 有时效需重取）；VIP 歌曲需 VIP 账号 cookie。
- **建议**：软件内置「本地音乐导入」为主，网易云抓取若要做，做成用户自行部署的可选插件并显著提示风险，开源主仓库不直接分发。

### 4.5 抓取功能的工程形态建议

- 统一抽象为「链接解析器」插件层：粘贴链接 → 识别平台 → 调对应解析后端 → 下载/流媒体 → 入 cue。
- B 站、抖音（游客态）进主仓库；小红书、网易云以第三方插件协议开放，风险隔离。

---

## 五、软件内播放 PPT（双窗控制）

没有完美的开源 PPT 渲染引擎，四条路线按保真度递增：

| 方案 | 保真度 | 依赖 | 双窗控制 | 许可 |
|---|---|---|---|---|
| 转图片/PDF 播放（LibreOffice headless 转换） | 无动画，静态帧 | LibreOffice（MPL-2.0，无传染性） | 完全自研：控制窗缩略图/备注/计时 + 输出窗全屏 | ✅ 安全 |
| 转 HTML 播放 | 部分动画 | 转换链复杂 | 自研双窗 | ✅ |
| LibreOffice UNO 远程控制 Impress | 中等（平滑切换/3D/触发器还原不全） | LibreOffice | 自带 Presenter Console 双屏 | ✅ |
| 系统 Office 自动化（macOS AppleScript / Windows COM） | 100% 原生 | 用户自装 Office | 用 Office 自带演讲者视图 | ✅（无分发问题） |

**推荐：双轨制。**
1. **默认轨**：LibreOffice headless 把 PPTX 转 PDF/图片序列 → 自研双窗播放器（控制窗：缩略图网格、备注、计时器；输出窗：全屏放映，支持黑屏/跳页）。零额外依赖、可随软件分发、切 cue 延迟最低。代价是动画拍平——引导用户在导出时接受。
2. **增强轨**：检测到用户装有 Office 时，提供「原生放映模式」（AppleScript/COM 控制翻页），保真度封顶。

ONLYOFFICE/Collabora 嵌入（iframe + Docker）面向 Web 服务端场景，做桌面放映引擎过重，不推荐；ONLYOFFICE CE 还是 AGPL-3.0。

---

## 六、Syphon 与 NDI 输出

### 6.1 Syphon（macOS）—— 无障碍

- BSD-3-Clause 开源协议，基于 IOSurface 的 GPU 零拷贝纹理共享，VDMX/Resolume/MadMapper 生态标配。
- 绑定成熟：[node-syphon](https://github.com/benoitlahoz/node-syphon)（Electron，Intel+Apple Silicon，1080p 接收延迟约 8ms）、[texture-bridge-core](https://www.jsdelivr.com/package/npm/@napolab/texture-bridge-core)（Electron 离屏渲染 sharedTexture 直发）、[syphon-python](https://github.com/cansik/syphon-python)（MIT）。
- Windows 等价物：Spout。

### 6.2 NDI —— 技术可行，分发有合规约束

- NDI 6 标准 SDK 仍为 **royalty-free**（Advanced SDK 才收费）。
- **开源项目的标准做法（OBS/DistroAV 模式，NDI 官方认可）**：
  - 动态链接 libNDI，SDK 头文件（MIT）可随源码分发；
  - **不可再分发 NDI Runtime 二进制**——安装器引导用户自行下载安装；
  - 应用/文档中标注「NDI® is a registered trademark of Vizrt NDI AB」并链接 ndi.video；
  - 产品名不能含「NDI」字样（需书面授权）；
  - 发布时使用 30 天内的新版 SDK。
- NDI 无开源替代实现，DistroAV 即「开源壳 + 专有运行时」模式。

### 6.3 同时输出 Syphon + NDI 的实现路径

渲染管线：视频帧 → 离屏纹理（Metal/IOSurface）→ 两路分发：
1. IOSurface 直接 publish 给 Syphon Server（GPU 零拷贝）；
2. 同帧映射/读回，打包为 NDI video frame，调 `NDIlib_send_send_video_async_v2` 发送。

Electron 路线用 offscreen rendering 的 sharedTextureHandle + texture-bridge-core/node-syphon 最省事；Qt/Tauri 原生侧用 OpenGL FBO + Syphon ObjC 桥。

---

## 七、开发路线图建议（MVP → 完整版）

| 阶段 | 内容 | 说明 |
|---|---|---|
| **MVP** | cue list（GO 键、pre/post-wait、auto-follow、fade）+ libmpv 视频/音频播放 + 双窗口输出 + 暖场音乐列表 + Cart 即点即播 | 纯本地文件，验证核心架构与现场稳定性 |
| **V1** | B 站链接解析（BBDown/bilibili-api）、PPT 转图片双窗播放、Syphon 输出 | 覆盖漫展 80% 场景 |
| **V2** | 抖音游客态解析、NDI 输出（引导装 Runtime）、Office 原生放映模式、OSC/MIDI 外部控制（Stream Deck/Companion） | 外部控制协议是行业标配，QLab/Mitti/Ontime 均有 |
| **插件生态** | 小红书、网易云以插件协议开放（用户自担风险） | 法律风险隔离 |

## 八、风险清单（按严重度）

1. 🔴 **网易云抓取的法律风险**（已有律师函删库先例）——不做核心依赖。
2. 🔴 **小红书/抖音登录态抓取的账号封禁风险**——游客态 + 插件化。
3. 🟡 **NDI 运行时分发限制**——引导用户自装，产品命名避开「NDI」。
4. 🟡 **libmpv GPL 绑定**——项目定 GPL-3.0 即可化解。
5. 🟡 **平台签名算法频繁更新**（a_bogus / X-S / wbi）——解析层插件化，便于热更新。
6. 🟢 **PPT 动画保真度**——双轨制 + 用户预期管理。

---

## 九、竞品/参考项目一览

| 项目 | 类型 | 技术栈 | 可借鉴点 |
|---|---|---|---|
| [QLab](https://qlab.app) | 商业（macOS） | 原生 | cue 模型、交互范式的事实标准 |
| [Mitti](https://imimot.com/mitti) | 商业（macOS） | 原生/Metal | NDI/Syphon 输出、多实例同步 |
| [Linux Show Player](https://github.com/FrancescoCeruti/linux-show-player) | 开源 GPL-3.0（Linux） | PyQt + GStreamer | cue 状态机、List/Cart 布局 |
| [QPlayer](https://github.com/space928/QPlayer) | 开源 GPL-3.0（Win） | C# WPF | QLab 式 UI 的 Windows 实践 |
| [Ontime](https://github.com/cpvalente/ontime) | 开源 | Electron | rundown/舞台计时，可互补或集成 |
| [DistroAV](https://github.com/DistroAV/DistroAV) | 开源 GPL-2.0 | OBS 插件 | NDI 合规分发模式的范本 |
| [BBDown](https://github.com/nilaoda/BBDown) | 开源 | C# | B 站登录抓取 |
| [MediaCrawler](https://github.com/NanmiCoder/MediaCrawler) | 开源 | Python | 多平台登录抓取架构 |

---

## 十、副软件：分辨率变换 → 1080P 信号输出（2026-10-23 补充调研）

### 10.1 结论

**完全可行，且无需自研核心算法。** 关键架构建议：

- **主软件播放的画面：不要用副软件，直接在主软件内置缩放输出。** 主软件已是 libmpv 栈，libplacebo 的 `force_original_aspect_ratio=decrease` 参数内置「缩放 + 留边到 1080P」，实现可能只是几行渲染配置，且零额外延迟。
- **副软件的合理定位：独立「信号适配器」**——承接主软件之外的外部信号（采集卡画面、别的 App 的 Syphon、网络 NDI 源），做统一变换后输出 1080P。
- **AI 超分不进实时主链路**：Real-ESRGAN 在 RTX 4090 上 480p 输入仅约 60fps，1080p 4x 需 12GB 显存；Anime4K 是 GLSL shader（非神经网络），可实时但只适合动漫素材，可作可选 shader。

### 10.2 缩放算法选型

| 方案 | 质量 | 性能（1080p 级实时） | 许可 | 建议 |
|---|---|---|---|---|
| FFmpeg swscale（bilinear/bicubic/lanczos） | 实测算法间耗时差异在噪声范围内，换好算法几乎零成本 | 轻松实时 | LGPL/GPL | 下变换 lanczos、上变换 bicubic |
| zscale（zimg） | 与 swscale 难分高下，但色彩空间/range 处理更严谨 | 实时 | WTFPL | 色彩转换时优先 |
| **libplacebo**（mpv 渲染库） | spline36 / ewa_lanczos(Jinc) / 抗振铃 / gamma 正确缩放，质量最高 | Vulkan/OpenGL/D3D11 GPU 加速 | LGPL-2.1+（可协商 BSD） | **首选**，FFmpeg 8.0 已内置其滤镜 |
| AI 超分（Real-ESRGAN/Anime4K） | 特定内容更优 | 神经网络方案不满足实时；Anime4K 可实时 | 各异 | 仅作可选 shader / 离线预渲染 |

### 10.3 信号输入输出形态

- **Syphon 输入/输出**：零拷贝 IOSurface，开销约 1ms，已有 Rust 绑定 [syphon-rs](https://github.com/BlueJayLouche/syphon-rs)（MIT/BSD，wgpu/Metal），与 Tauri 栈天然匹配。注意 Syphon 无读写同步锁，高负载需双缓冲。
- **NDI 输入/输出**：OBS + obs-ndi/DistroAV 是成熟先例；因编码压缩约增加 1 帧（约 16ms）延迟。
- **采集卡输入**：Blackmagic DeckLink SDK 免费跨平台（OBS、GStreamer decklinksrc 原生支持）；UVC 免驱卡走 AVFoundation，选 YUY2 未压缩模式可省一次 CPU 解码。
- **宽高比策略**：拉伸/裁切/留边三模式；竖屏转横屏的模糊填充背景 = split → 放大裁切 → boxblur → overlay，GPU 上等价两次采样 + 一次模糊 pass，一个滤镜图完成。

### 10.4 延迟预算

- GPU 缩放本身：bilinear <1ms，bicubic 1–3ms，lanczos 3–8ms——**缩放不是瓶颈**。
- 端到端瓶颈在采集/传输：PCIe 采集卡 15–40ms，USB 卡 40–150ms，NDI 网络 50–120ms。
- 舞台建议：端到端 ≤ 2 帧（约 33ms @60fps），上限 60–80ms，超 100ms 明显可感知。
- **关键工程纪律：帧全程留在 GPU**（纹理 → shader 缩放 → 输出纹理），不做 GPU→CPU→GPU 往返。

### 10.5 副软件形态建议

轻量「信号适配器」：Syphon / NDI / DeckLink / UVC 输入 → libplacebo GPU 缩放（含宽高比策略）→ Syphon / NDI / DeckLink 输出。Rust + syphon-rs + wgpu，与主软件同栈。若输入只来自主软件内部帧，则内置模块明显更优（进程间传帧：Syphon 约 1ms 零拷贝，NDI +16ms 且有压缩损失，CPU 共享内存 1080p60 RGBA 约 500MB/s 带宽挤占）。

---

## 十一、主软件新增：音频信号管理（2026-10-23 补充调研）

### 11.1 结论

**可行，但有一个关键架构决策：libmpv 的音频「能控不能取」。** 要做真正的软件内混音/闪避/矩阵路由，必须用 FFmpeg 自解码建独立音频管线，mpv 只出画面；简易方案则是每个 mpv 实例用 `--audio-device` 单独指定输出设备（Core Audio/WASAPI 支持独占），实现「按 cue 路由到不同声卡」但无软件内总线。

### 11.2 音频引擎选型

| 引擎 | 能力 | 许可 | 建议 |
|---|---|---|---|
| **miniaudio** | 单文件 C 库，内置混音节点图 `ma_node_graph`，低延迟口碑好 | 公有领域 / MIT-0 | **首选**（与自研混音管线最匹配） |
| **cpal**（Rust） | 纯 Rust 低层音频 I/O，无 C++ 依赖，与 Tauri 天然契合 | Apache/MIT | Tauri 栈首选 I/O 层，混音自写 |
| JUCE | 能力最全（混音图/DSP/插件托管） | AGPLv3 / 商业双许可（Indie $40/月起） | 能力溢出且许可是约束，不推荐 |
| PortAudio / RtAudio | 纯 I/O 库，不含混音器 | MIT 类 | 备选 |
| AVAudioEngine | macOS 原生：混音、installTap 电平表、系统 AudioUnit 限幅器 | 系统自带 | 若只做 macOS 版可用 |

### 11.3 架构模型：照搬 QLab「patch + 两级矩阵」

```
每路音源（暖场音乐/视频/PPT 伴音/麦克风）
  → per-cue 通道（增益、淡入淡出、声像）
  → cue 内矩阵（crosspoint）
  → patch（一个 patch = 一个输出设备，如主 PA 声卡 / 返听 / 直播采集）
  → patch 矩阵 → 设备物理通道
```

- **主总线末端挂限幅器**（永远放滤镜链最后，OBS 实践共识：噪声抑制 → 门限 → 压缩 → 限幅）。
- **Ducking 闪避**：以麦克风为侧链源的压缩器（OBS 压缩器滤镜已内置 Sidechain/Ducking 选项，模型可直接借鉴）。
- **电平表**：installTap/vDSP 取 buffer 算平均功率做 VU 表，现场必备。
- **虚拟声卡（BlackHole / VB-Cable）的角色**：只做应用间路由与预监听（PFL），不作为到调音台的主路径（BlackHole 有已知时钟漂移问题）；到调音台走物理 USB 声卡 / Dante。

### 11.4 延迟与唇同步

- 可察觉阈值（ITU-R BT.1359）：音频超前 ≤45ms / 滞后 ≤125ms；投影大屏链路常引入 50–200ms 视频延迟，需音频延迟补偿对齐。
- 现场返听要求苛刻得多：理想 <4ms。
- macOS 走 Core Audio 原生低延迟（QLab/Mitti 均基于它），Windows 走 ASIO/WASAPI 独占。
- **若把视频音频抽进自研管线，必须以 mpv 画面时钟为基准重新对齐**，否则破坏 mpv 内部 A/V 同步。

### 11.5 分级实现建议

| 级别 | 方案 | 能力 |
|---|---|---|
| L1（MVP 即可做） | 每 mpv 实例 `--audio-device` 路由 + `--af` 滤镜链做增益/淡入淡出 | 按 cue 路由到不同声卡，无软件内总线 |
| L2 | miniaudio/cpal 自建混音引擎，音乐/PPT 伴音走自研管线，视频仍走 mpv | 混音矩阵、VU 表、限幅、ducking |
| L3 | 视频音频也自解码（FFmpeg）进总线，mpv 仅出画面 | 全量 QLab 级音频管理，需处理唇同步 |

---

## 十二、更新后的路线图

| 阶段 | 内容 |
|---|---|
| **MVP** | cue list + libmpv 播放 + 双窗口 + 暖场音乐 + Cart 即点即播 + **音频 L1（按设备路由 + 增益/淡入淡出）+ 内置 1080P 缩放输出（libplacebo 参数级实现）** |
| **V1** | B 站链接解析、PPT 转图片双窗播放、Syphon 输出 + **音频 L2（混音矩阵 / VU 表 / 限幅 / ducking）** |
| **V2** | 抖音游客态解析、NDI 输出、Office 原生放映、OSC/MIDI 外部控制 + **独立信号适配器副软件（Syphon/NDI/DeckLink/UVC → 1080P）** |
| **V3 / 插件生态** | 音频 L3（视频音频进总线）、AI 超分可选 shader、小红书/网易云插件（用户自担风险） |

---

## 十三、副软件内置工具：AI 编排 + 本地执行的格式转换/超分（2026-10-23 第三轮调研）

### 13.1 模式定义

用户自填 LLM API Key（BYOK）→ AI 读取素材的 ffprobe 元数据、理解用户的自然语言需求 → **生成参数化的本地工具调用计划** → 本地开源工具（FFmpeg / ncnn-vulkan 系超分 CLI）执行实际转换与超分。**AI 只做决策与编排，不上传素材、不做云端处理**——素材全程不出本机。

### 13.2 本地超分工具现状（macOS 视角）

**最现实路线：FFmpeg 拆帧 → ncnn-vulkan 系便携二进制逐帧超分 → FFmpeg 合成。** 这些工具为 MIT/BSD 许可，便携二进制可直接随开源软件分发。

| 工具 | 许可 | 维护状态 | 说明 |
|---|---|---|---|
| [realesrgan-ncnn-vulkan](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan) | MIT | 最新 release v0.2.0（2022），二进制仍被广泛使用 | 提供 macOS 可执行文件，经 MoltenVK 在 M 系列上可运行；仅图像，视频需自行拆帧 |
| [waifu2x-ncnn-vulkan](https://github.com/nihui/waifu2x-ncnn-vulkan) | MIT | 活跃（2025-09 仍有 release） | 动漫素材首选，1x/2x 倍率 |
| [Video2X](https://github.com/k4yt3x/video2x) | AGPL-3.0 | 活跃（v6.4.0） | **无原生 Apple Silicon 构建**，不适合直接集成；其性能数据可作参考 |
| [Upscayl](https://github.com/upscayl/upscayl) | AGPL-3.0 | 活跃 | Electron GUI，证明 ncnn 路线在 M 系列 Mac 原生可行，但仅图像 |
| Real-ESRGAN 原版（PyTorch） | BSD-3 | 事实停更 | 不做分发底座 |

**性能预期管理**：按 Video2X 实测推算，1 分钟 1080p 视频超分约需 5–16 分钟（独显 PC）；M 系列经 MoltenVK 预计同级或更慢，按「10 分钟级」设计，必须给进度条与后台队列。显存门槛约 4GB（1080p×2）。

### 13.3 LLM 编排本地命令行：有成熟先例

已有开源先例验证模式可行：[llmpeg](https://github.com/ali-master/llmpeg)（多 provider BYOK + 预设 + 直接执行）、[ffmpeg-ai](https://pypi.org/project/ffmpeg-ai/)（本地 Ollama + RAG 离线版）、[ai-ffmpeg-cli](https://github.com/Indraparama940/ai-ffmpeg-cli)（GUI + 命令预览）。

**核心工程纪律（安全红线）**：
1. `ffprobe -show_format -show_streams -of json` 元数据截断后喂给 LLM；
2. LLM 用 function calling / JSON Schema 输出**结构化计划**（程序名 + argv 数组），用 `Command::new().args()` 起进程，**绝不经过 shell**，天然免疫命令注入；
3. 参数白名单：只允许注册过的工具（ffmpeg / ffprobe / realesrgan-ncnn-vulkan / waifu2x-ncnn-vulkan），输入输出路径强制限定在用户工作目录内，永不生成 `rm`/`sudo`；
4. **执行前向用户展示命令全文 + 自然语言解释 + 预估耗时，确认后才运行**；
5. 用 ffmpeg `-progress pipe:1` 解析进度；失败时把 stderr 尾部回喂 LLM 自动修正重试（限次数），全程日志可回读导出。

### 13.4 BYOK 设置页与安全

- provider 下拉 + Key 输入 + 「测试连接」按钮（坏 Key 不落库）；保存后只显示前缀。
- Key 存 macOS Keychain（Rust `keyring` crate）或 Electron `safeStorage`，**不写明文、不进 git**；流量只从本机直连 provider。

### 13.5 离线兜底：手动模式与 AI 模式共用同一套执行管线

手动模式 = 参数化预设模板生成同结构的「任务计划」，与 AI 计划走相同的「预览 → 确认 → 执行 → 日志」流程。AI 生成的方案可「另存为模板」沉淀为个人预设。

建议内置预设：
1. 通用转 1080P H.264（`libx264 -crf 18`，Mac 可切 `h264_videotoolbox` 硬编提速数倍）；
2. ProRes 422 舞台母带；
3. 竖屏转横屏模糊填充（split + boxblur + overlay）；
4. 批量转码队列（并发限制、失败重试、完成通知）；
5. 音频响度归一（`loudnorm` 两遍法）——漫展现场高频需求。

首次启动检测不到 Key 时默认落在手动模式，AI 面板放「去设置 Key」入口而非报错。

### 13.6 更新后的副软件完整形态

```
┌─ 实时信号适配器（原定位，不变）
│   Syphon/NDI/DeckLink/UVC 输入 → libplacebo GPU 缩放 → 1080P 输出
│
└─ 离线媒体工具箱（新增）
    ├─ 手动模式：预设模板 → 参数化计划
    ├─ AI 模式（BYOK）：ffprobe 元数据 + 自然语言需求 → LLM 生成计划
    └─ 统一执行层：白名单校验 → 用户确认 → argv 执行 → 进度/日志/重试
        工具链：FFmpeg（转码/封装/响度）+ ncnn-vulkan 系 CLI（超分）
```

风险与注意点：
- 🟡 超分耗时是「10 分钟级」的离线批处理，UI 上必须与实时信号链路明确区隔；
- 🟡 ncnn-vulkan 的 macOS 二进制依赖 MoltenVK（已被 Khronos 标记 deprecated 转社区维护），目前运行正常，长期需关注 KosmicKrisp 迁移；
- 🟢 全部工具 MIT/BSD/ LGPL，可随 GPL-3.0 主项目分发便携二进制；Video2X/Upscayl 为 AGPL，只参考不集成。

---

## 十四、下载层设计：默认 yt-dlp + 插件接管（2026-10-23 第四轮调研）

### 14.1 架构定义

```
链接解析请求
  → 平台识别
  → 已安装该平台插件？── 是 → 插件方法下载（BBDown / MediaCrawler / 自研解析…）
                      └─ 否 → yt-dlp 默认下载
```

- **默认层：yt-dlp** 覆盖全部支持平台，保证开箱即用；
- **插件层**：用户安装某平台的插件后，该平台的下载改走插件方法（更高画质、更稳的风控对抗、登录态歌单抓取等）；插件失效时回退 yt-dlp。

### 14.2 yt-dlp 集成方式（已核实）

- **许可无障碍**：The Unlicense（公有领域），二进制或 Python 库均可随 GPL-3.0 软件自由再分发（保留 THIRD_PARTY_LICENSES.txt）。
- **推荐集成形态**：调用官方 standalone 二进制（自带 Python 运行时，用户零依赖，约 10–30MB，Stacher、Seal 等 GUI 的通行做法）；若主软件已外带 Python 运行时，也可 `import yt_dlp` 用库方式——**库方式对插件机制更灵活**（见 14.3）。
- **更新是运维刚需**：平台改版会让旧 extractor 几周内失效。yt-dlp 按日期发版、有 stable/nightly/master 通道、`yt-dlp -U` 原地自更新。软件内必须做：**启动时自动检查更新 + 失败回退内置版本**（参考 Stacher 的做法）。注意 2026 年起官方提示旧版二进制的 certifi MITM 风险，保守更新；YouTube 完整支持还需外部 JS 运行时（Deno）配合 yt-dlp-ejs。

### 14.3 「插件接管」可直接复用 yt-dlp 官方插件机制

yt-dlp 自 2022.01 起支持 `yt_dlp_plugins.extractor` 命名空间插件，且**插件 extractor 优先于内置 extractor**——正好实现「装插件后特定平台改走插件方法」：

- 库方式集成：pip / PYTHONPATH 安装插件包，最灵活；
- 二进制方式：放二进制旁 `yt-dlp-plugins/` 目录（PyInstaller 版不支持 PYTHONPATH）；
- ⚠️ 官方对插件代码不做审查、无条件 import——**插件必须做签名/来源审核或显式用户确认**，这是安全红线。

自研插件层（第四节的「链接解析器插件」）与 yt-dlp 官方机制可并存：BBDown（B 站）、MediaCrawler（小红书）、f2（抖音）等以自研插件协议接入，yt-dlp 系增强走官方插件目录。

### 14.4 三个目标平台的 yt-dlp 支持现状

| 平台 | 内置 extractor | 状态 |
|---|---|---|
| B 站 | ✅ | 可用；720P 以上需登录 cookie，大会员内容需大会员 cookie——**这正是 BBDown 插件存在的价值** |
| 抖音 | ✅ | 需要新鲜 cookie（不必登录，`--cookies-from-browser` 即可）；2025 年起裸跑不稳定，插件兜底常见 |
| 小红书 | ✅ | 裸跑得 720p，带 cookie 更高画质；extractor 失效 issue 仍在跟踪，属「能用但需跟进」状态 |

结论：**三平台 yt-dlp 均能兜底，但高画质/登录态/稳定性都要靠插件层**——默认层与插件层的分工与实际需求吻合。

### 14.5 法律风险先例

- 2020 年 RIAA 依 DMCA §1201 投诉 youtube-dl，GitHub 一度下架，EFF 介入后 **2020-11-16 恢复仓库**，GitHub 此后对 §1201 类投诉改为人工复核并设开发者辩护基金。美国托管/分发层面风险低。
- ⚠️ 但德国汉堡法院 2023/2024 年在 Uberspace 案中认定 youtube-dl 违反德国反规避法——**欧洲法域风险更高**。
- 对策：文档与 UI 声明「仅下载用户有权下载的内容」；结合此前结论——小红书/网易云相关插件不进主仓库，风险隔离策略不变。
