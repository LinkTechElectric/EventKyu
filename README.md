# EventKyu

开源的漫展 / 演出舞台切 cue 工作站。

> 当前状态：**设计阶段**（M0 技术验证筹备中）。设计文档见 `docs/` 目录，欢迎讨论。

## 是什么

EventKyu 面向漫展舞台与现场演出场景，目标是填补开源世界「跨平台 + 视频 cue + 双窗口输出」的空位（QLab / Mitti / PVP 均为 macOS 商业软件，开源侧暂无成熟方案）。

### 三大核心功能

- **暖场音乐播放**：独立播放列表、循环 / 随机、与正式 cue 交叉淡入淡出
- **链接自由播放**：粘贴链接即点即播（Cart 触屏格布局），默认 yt-dlp 下载，插件可接管特定平台
- **正式舞台时间线编排**：GO 键 + pre-wait / post-wait / auto-follow / Fade cue 的完整 cue 状态机

### 进阶能力

- 平台媒体抓取：B 站 / 抖音（主仓库插件），小红书 / 网易云（第三方插件，风险隔离）
- 软件内播放 PPT：LibreOffice 转换双窗播放（默认）+ 系统 Office 原生放映（增强）
- 帧共享输出：**Syphon（macOS）/ Spout（Windows）/ NDI（双平台）**
- 专业音频信号管理：分设备（声卡 / 虚拟输出 / 系统输出）独立电平与电平表、实时降噪（RNNoise）、声卡输入路由至虚拟输出
- 副软件 SignalFit：外部信号（Syphon / NDI / 采集卡 / UVC）统一变换输出 1080P + AI 编排（BYOK）的离线格式转换 / 超分工具箱

## 技术架构（摘要）

| 层 | macOS | Windows |
|---|---|---|
| 应用壳 | Swift（AppKit + SwiftUI）+ Metal | Tauri 2 + Rust |
| 视频内核 | libmpv（IINA 模式 GL 嵌入） | libmpv |
| 音频引擎 | Core Audio（AUHAL 逐输出链） | WASAPI |
| 帧共享 | Syphon Metal + NDI | Spout + NDI |
| 共享核心 | ← Rust staticlib / UniFFI → | Rust crates |

完整设计文档：

- [可行性调研报告](docs/漫展切cue软件可行性报告.md)：四轮调研，覆盖 cue 软件格局、平台抓取、PPT 播放、Syphon/NDI、缩放超分、音频引擎、yt-dlp 下载层
- [技术架构设计](docs/技术架构设计.md)：v2.0，含双平台渲染 / 音频管线、依赖清单与内置运行时下载器、构建分发矩阵、里程碑与 ADR

## 合规声明

- 下载功能仅限用户下载其有权下载的内容；NDI® is a registered trademark of Vizrt NDI AB（NDI Runtime 由用户自装，本项目不分发）。

## License

GPL-3.0
