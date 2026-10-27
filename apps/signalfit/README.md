# SignalFit

SignalFit 是 EventKyu 的「信号适配器 + 离线媒体工具箱」。本目录是一个独立的 Tauri 2 应用骨架：当前重点交付可审查、可确认、可追踪的离线媒体工作流；实时信号适配仅提供稳定的模块边界与明确的 Unsupported 实现。

许可证：GPL-3.0-only。

## 当前功能

### 预设工具箱（已接入真实核心实现）

- 1080P H.264：libx264 或 macOS VideoToolbox，保持比例缩放与填充。
- ProRes 422：`prores_ks` profile 3、10-bit 4:2:2、24-bit PCM 音频。
- 竖屏模糊填充：分离背景/前景，背景放大裁切与模糊，前景居中。
- loudnorm 两遍：第一遍测量，使用填入的测量值执行线性第二遍。
- 超分三步链：ffmpeg 抽帧 → Real-ESRGAN / waifu2x ncnn Vulkan → ffmpeg 重组。

预设来自 `core/ai-orchestrator`，SignalFit 没有复制另一套 argv 模板。界面完整展示每一步的程序、独立 argv、解释、工作目录和预估耗时；用户批准前不会启动进程。

### AI 助手

- `MockProvider`：本地确定性演示，无网络、无需 Key，生成一个受工作目录约束的 H.264 示例计划。
- `OpenAiCompatibleProvider`：配置与 `LlmProvider` 边界已建立，真实 HTTP 请求明确留白并返回友好错误。
- AI 输出仍由核心 `generate_plan` / `parse_plan_json` 做 JSON 解析、可执行文件白名单与路径沙箱校验，随后进入与预设完全相同的用户确认流。
- 测试连接按钮目前仅为交互占位。

### 执行与日志

- `LocalProcessRunner` 使用 `std::process::Command` 直接传递 argv，不经过 Shell。
- stdout / stderr 由两个读取线程并发消费，避免管道积满；前端通过 Tauri event 接收逐行输出与 ffmpeg progress。
- 支持每进程超时、协作取消和 kill，保留完整尝试日志。
- 缺少 ffmpeg、ffprobe 或 ncnn CLI 时返回「未安装，请通过依赖管理安装」，不会尝试自行安装。
- 最近一次执行（包括失败执行）的完整日志可导出为文本。

### BYOK Key 存储

macOS 上通过 `/usr/bin/security` 的 `add-generic-password`、`find-generic-password`、`delete-generic-password` 实现真实 Keychain 读写：

- service：`fit.eventkyu.signalfit`；account：provider 名称。
- 前端只能写入、查询是否存在、删除，不能读取 Key 内容。
- Key 不写入明文文件，不进入应用状态日志或错误日志；Keychain 工具的诊断输出也不会直接透传。
- 非 macOS 平台返回 Unsupported，不退化为明文存储。

## 前端面板

`ui/` 使用原生 TypeScript + Vite，无 UI 框架：

1. **预设工具箱**：预设卡片 → 参数表单 → 计划预览 → 批准执行 → 进度与日志。
2. **AI 助手**：provider / Key 设置、ffprobe JSON 与需求输入 → 同一计划确认流。
3. **实时适配器**：开发中说明、输入/输出类型和 GPU 架构图，不伪装成可用功能。

## 模块划分

```text
apps/signalfit/
├── src-tauri/
│   ├── Cargo.toml                 # 空 [workspace]，脱离父 workspace
│   ├── tauri.conf.json            # Tauri 2 窗口、CSP 与前端构建配置
│   └── src/
│       ├── commands.rs            # commands、计划会话、执行报告与事件
│       ├── process.rs             # argv 进程、流读取、目录限定、超时/取消
│       ├── ai.rs                  # Mock 与 OpenAI 兼容 provider 边界
│       ├── keystore.rs            # macOS Keychain / 其他平台 Unsupported
│       ├── realtime/mod.rs        # 实时类型、trait 与 Unsupported 占位
│       └── lib.rs                 # Tauri 注册与启动
├── ui/                            # 原生 TS + Vite 三面板前端
└── scripts/generate_icon.py       # 原创应用图标再生成脚本（需 Pillow）
```

核心复用：

- `ai-orchestrator`：五个预设、`ExecutionPlan`、`PlanSession` 状态机、白名单与路径沙箱、`ExecutionEngine`、progress 解析、日志导出、`LlmProvider` / `KeyStore` trait。
- `runtime-manager` 暂未成为运行依赖；后续依赖管理界面可复用其 manifest 与健康检查。当前只给出安装指引，不自动下载工具。

## 构建与运行

要求：Rust（项目验证使用 1.99）、Node.js 与 npm。运行真实媒体计划还需要用户自行提供相应 CLI；本仓库骨架不会安装 ffmpeg。

```bash
cd apps/signalfit/ui
npm ci
npm run build

cd ../src-tauri
cargo check
cargo test
cargo run
```

`cargo run` 使用已构建的 `ui/dist`。前端热更新可在一个终端运行 `cd apps/signalfit/ui && npm run dev`，另一个终端运行 `cd apps/signalfit/src-tauri && cargo run`；调试配置指向 `http://localhost:1420`。

`src-tauri/Cargo.toml` 中的空 `[workspace]` 必须保留。请从该目录执行 Cargo 命令，不要把它加入仓库根 workspace。

## 安全设计

### 计划与状态机

`plan_validate` 会完成核心校验并进入 `AwaitingConfirmation`，返回 `plan_id`；`plan_approve` 只允许批准该待确认计划。`ExecutionEngine` 只接受 `Approved` 会话，因此无法通过直接调用 `plan_execute` 绕过校验或确认。

主要 commands：

| Command | 作用 |
| --- | --- |
| `list_presets` | 返回五个预设的卡片元数据 |
| `preset_plan` | 只生成 `ExecutionPlan`，不执行 |
| `plan_validate` | 白名单/路径校验并登记待确认会话 |
| `plan_approve` / `plan_reject` | 用户批准或拒绝 |
| `plan_execute` | 执行已批准计划，返回报告并推送事件 |
| `plan_cancel` | 取消待执行会话或终止正在运行的子进程 |
| `export_log` | 导出最近一次完整日志 |
| `ai_generate` | 经 provider 生成并校验计划 |
| `key_set` / `key_status` / `key_delete` | 受保护凭据管理，不读取到前端 |

### 进程与路径

- 可执行文件限定为 ffmpeg、ffprobe、Real-ESRGAN、waifu2x 核心白名单。
- 核心校验拒绝父目录穿越、URI/网络协议路径、越界绝对路径与已有 symlink 逃逸。
- 应用执行器还会 canonicalize 用户根目录和每一步工作目录，拒绝根目录之外的 cwd。
- argv 始终是独立参数；界面中的 Shell 风格引号只用于预览，不会拼成命令执行。
- AI 只接收元数据与需求文本；媒体文件本身不上传。真实 HTTP 接入后也必须保持无 Key 日志与同一计划校验纪律。

## 测试

```bash
cd apps/signalfit/src-tauri
cargo test
```

测试覆盖：五个预设构建与校验、未知参数拒绝、确认流不可绕过、Mock 计划、provider 名称约束、实时 Unsupported，以及使用 `/bin/echo`、`/bin/ls`、`/bin/sleep` 的真实进程测试。后者覆盖输出流、退出码、非 Shell 参数、缺失工具提示、cwd 逃逸、超时 kill 与取消 kill，不依赖 ffmpeg。

## 明确留白

1. **wgpu 实时管线**：设备/queue/surface、纹理同步、预览与输出后端未实现。
2. **libplacebo**：色彩管理、缩放、去 banding、tone mapping 仅标注接入点。
3. **Syphon / NDI / DeckLink / UVC 输入输出**：只有枚举和 `RealtimePipeline` 契约；占位实现返回 Unsupported，不 panic、不使用 `todo!()`。
4. **真实 LLM HTTP**：OpenAI 兼容 provider 尚无 HTTP client、连接测试与响应解析；当前可用的是本地 Mock。
5. **依赖自动安装/体检 UI**：当前只有友好缺失提示，后续接 `runtime-manager`。
6. **AI 自动修复执行**：核心支持 `execute_with_retry`，本应用当前使用无 LLM 修复的 `execute`，确保失败不会被自动改写后再次执行。

实时接入必须遵守「帧全程 GPU」：帧进入管线后，转换、缩放、色彩处理和输出之间不做 CPU readback；确实无法避免的采集上传或 SDK 下载只能留在后端边界，并需测量和记录。
