# EventKyu Windows（Tauri 2 壳）

`apps/eventkyu-win` 是 EventKyu 的 Windows 控制窗 + Stage 输出窗壳。它直接复用仓库内的 `cue-core`、`download-core` 和 `runtime-manager`，不复制核心调度逻辑。项目许可证沿用 EventKyu 的 GPL-3.0-only。

## 模块划分

```text
apps/eventkyu-win/
├── src-tauri/
│   ├── Cargo.toml                 # 顶部空 [workspace]，与父 workspace 隔离
│   ├── tauri.conf.json            # control / stage 双窗、Vite、NSIS、WebView2
│   ├── capabilities/default.json  # 两窗共享最小 core 权限
│   └── src/
│       ├── lib.rs                 # Tauri Builder、State<Mutex<Engine>>、命令注册
│       ├── commands/engine.rs     # Cue DTO、GO/tick/status/events 与单测
│       ├── commands/download.rs   # 平台识别、yt-dlp 模拟 argv 与单测
│       ├── commands/stage.rs      # Stage 全屏；Windows monitor 枚举
│       └── runtime_check.rs       # 真实 manifest、离线自检、占位 Transport
└── ui/
    ├── index.html + src/control.ts# 控制窗；50 ms tick 与事件轮询
    ├── stage.html + src/stage.ts  # 黑底输出；50 ms 只读状态轮询
    └── src/api.ts                 # 原生 TS IPC 类型与命令封装
```

控制窗是唯一调用 `engine_tick` 的页面；Stage 只调用 `engine_status`，因此两个窗口不会竞争推进引擎时钟。`engine_events` 使用 drain 语义，事件由控制窗消费。

## Windows 构建要求

1. **Windows 10/11 x86_64**，Rust stable（rustup）及 `x86_64-pc-windows-msvc` target。
2. **Visual Studio 2022 Build Tools**：安装“使用 C++ 的桌面开发”，包括 MSVC x64/x86 生成工具和 Windows 10/11 SDK。Tauri 的 NSIS 打包与 MSVC 链接依赖这些组件。
3. **Node.js 20+ 与 npm**（本机验证使用 Node.js 24.21.0 / npm 11.12.1）。
4. **Tauri 2 CLI**：开发/打包可安装 `cargo install tauri-cli --version "^2"`，然后使用 `cargo tauri dev` / `cargo tauri build`。单独的 `cargo check` / `cargo test` 不需要 CLI。
5. **WebView2 Evergreen Runtime**：开发和运行机器需要 WebView2。配置默认是：

   ```json
   "webviewInstallMode": { "type": "embedBootstrapper" }
   ```

   联网安装版使用内嵌 bootstrapper。**离线演出版**应在 `src-tauri/tauri.conf.json` 将 `type` 改为 `offlineInstaller`，在可联网的构建机上生成内嵌完整 Evergreen 离线安装包的 NSIS 安装器；演出机不应依赖临时下载 Runtime。

6. `src-tauri/Cargo.toml` 顶部的空 `[workspace]` 必须保留。它使 Windows 壳拥有独立 lockfile/target，避免 Windows 专用依赖进入 macOS 根 workspace；三个 core crate 仍通过 path 依赖复用。

## 开发、构建与检查

```powershell
cd apps/eventkyu-win/ui
npm install
npm run dev                 # Vite: 127.0.0.1:1420
npm run build               # tsc --noEmit + Vite 双 HTML 构建

cd ../src-tauri
cargo check
cargo test
cargo tauri dev             # 另一终端；CLI 自动调用前端 dev script
cargo tauri build           # Windows 上生成 NSIS 安装器
```

也可先 `npm run build`，再在 `src-tauri` 使用 `cargo run --release` 运行已构建前端。实际 Windows 窗口、NSIS、WebView2 安装与多显示器行为必须在 Windows 实机验证。

### Tauri commands

| Command | 输入 | 行为 / 返回 |
| --- | --- | --- |
| `engine_add_cue` | `{ input: { name, kind, source?, preWaitMs, durationMs, postWaitMs } }` | `kind` 为 `audio` / `video` / `wait`；返回完整 `EngineSnapshot`。Wait 必须有 `durationMs`，媒体必须有 `source`。 |
| `engine_go` | 无 | 触发 playhead，返回快照。 |
| `engine_tick` | 无 | `tick_now()`，返回快照。 |
| `engine_status` | 无 | 不推进时钟，返回快照。 |
| `engine_events` | 无 | drain 核心事件并转成 camelCase JSON。 |
| `download_resolve` | `{ url }` | 返回平台、`yt-dlp` resolver、模拟 argv 和输出路径；`execution = "planOnly"`。 |
| `runtime_selfcheck` | 无 | 编译时读取根 `manifest/manifest.json`，过滤无当前平台资产的组件并返回 `ShowtimeReport`。 |
| `stage_toggle_fullscreen` | 无 | 切换 `label = "stage"` 窗口全屏；Windows 返回显示器枚举及当前显示器。 |

自检根目录默认为系统临时目录下的 `eventkyu-win/runtime`，可用 `EVENTKYU_RUNTIME_DIR` 覆盖。macOS 主机检查会按 macOS 清单键体检；Windows 构建则使用 `windows-x86_64`。

## 已在本机验证（macOS 开发机）

- `cd apps/eventkyu-win/src-tauri && cargo check`：**通过**，Tauri 2.12.5；可解析双窗配置、全部 commands 和三个 path 依赖。
- `cargo test`：**通过，8 passed / 0 failed**；另有 main 与 doc-test 目标均通过。
- `cargo fmt`：已执行。
- `cd apps/eventkyu-win/ui && npm install`：**通过**，15 packages，npm 报告 0 vulnerabilities。
- `npm run build`：**通过**，TypeScript strict 检查与 Vite 7.3.7 构建成功；`dist/index.html` 和 `dist/stage.html` 均生成。

未在本机验证：Windows 实机窗口、Windows target 交叉编译、NSIS、WebView2 安装流程、多显示器/全屏、媒体输出、真实下载与联网体检。

## 明确留白 / 后续接入点

- **libmpv**：当前没有音视频解码、渲染或音频设备输出。`MediaActionRequested` 仅进入事件日志。媒体适配器应消费 Play/Pause/Resume/Stop，并在播放自然结束时调用核心 `notify_media_completed`（尚需补对应壳命令/桥接）。
- **Spout**：没有 D3D11 纹理、共享纹理或 Spout sender。接入点位于 Stage 输出链：libmpv 解码帧 → 合成/渲染 → Spout2 sender；需要在 Windows 原生桥中处理 COM/D3D11 生命周期与像素格式。
- **NDI**：没有 NDI SDK sender、接收器或 Runtime 安装。清单中的 `ndi-runtime` 保留 EULA gate；发送端应与 Spout 共用合成后的帧，但独立处理 NDI 视频帧格式和发送生命周期。
- **真实下载进程**：`download_resolve` 固定 `simulate = true`，绝不启动进程。后续用 `DownloadJob + DownloadEngine::download` 接入 Windows `ProcessRunner`，关闭模拟后才可执行；需实现取消、stdout/stderr 逐行进度、退出码与安装目录策略。
- **真实 Transport / VersionProbe**：`OfflineTransport` 的 GET/HEAD 均返回“未接入网络”，`OfflineVersionProbe` 也不启动程序。后续 Transport 必须实现重定向、Range/206、超时和错误映射；VersionProbe 应安全调用私有安装的可执行文件。
- **显示器选择**：Windows 已用 Tauri 的 wry/tao monitor API 写出枚举代码，但 UI 目前仅切换当前 Stage 窗全屏，尚未实现“移动到指定屏再全屏”。
