import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

type PresetId =
  | "h264-1080p"
  | "prores-422"
  | "vertical-blur"
  | "loudnorm-two-pass"
  | "superresolution";

interface PresetDescriptor {
  id: PresetId;
  name: string;
  description: string;
  tools: string[];
}

interface ToolPlan {
  program: string;
  args: string[];
  explanation: string;
  estimated_minutes: number | null;
  workdir: string;
  inputs: string[];
  outputs: string[];
}

interface ExecutionPlan {
  steps: ToolPlan[];
}

interface ValidatedPlan {
  planId: string;
  plan: ExecutionPlan;
}

interface LoudnormMeasurement {
  input_i: number;
  input_tp: number;
  input_lra: number;
  input_thresh: number;
  target_offset: number;
}

interface PresetParams {
  input: string;
  output: string;
  workdir: string;
  width?: number;
  height?: number;
  crf?: number;
  encoder?: string;
  upscaleEngine?: string;
  scale?: number;
  fps?: number;
  loudnorm?: LoudnormMeasurement;
}

interface ExecutionReportView {
  plan: ExecutionPlan;
  attempts: Array<{ exitCode: number | null }>;
  logText: string;
}

interface OutputEvent {
  planId: string;
  stream: "stdout" | "stderr";
  line: string;
}

interface ProgressEvent {
  planId: string;
  progress: {
    frame: number | null;
    fps: number | null;
    out_time_ms: number | null;
    progress: "continue" | "end" | null;
  };
}

interface StateEvent {
  planId: string;
  state: "executing" | "done" | "failed" | "cancelled";
  message: string | null;
}

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Missing #app");

app.innerHTML = `
  <div class="shell">
    <header class="topbar">
      <a class="brand" href="#" aria-label="SignalFit 首页">
        <span class="brand-mark" aria-hidden="true"><i></i><i></i><i></i></span>
        <span><strong>SignalFit</strong><small>EVENTKYU / LOCAL MEDIA</small></span>
      </a>
      <div class="local-badge"><span></span> 本地执行 · 明示确认</div>
    </header>

    <main>
      <section class="hero">
        <div>
          <p class="eyebrow">OFFLINE TOOLBOX / 01</p>
          <h1>把媒体处理，<br><em>留给本地。</em></h1>
        </div>
        <p class="hero-copy">可审查的执行计划、逐行实时日志、系统级密钥保护。<br>媒体文件不上传，命令不经过 Shell。</p>
      </section>

      <nav class="tabs" aria-label="SignalFit 功能面板">
        <button class="tab active" data-panel="toolbox"><span>01</span> 预设工具箱</button>
        <button class="tab" data-panel="ai"><span>02</span> AI 助手</button>
        <button class="tab" data-panel="realtime"><span>03</span> 实时适配器 <b>BETA</b></button>
      </nav>

      <section id="panel-toolbox" class="panel">
        <div class="section-heading">
          <div><p class="eyebrow">CURATED RECIPES</p><h2>选择处理预设</h2></div>
          <p>五个经过路径沙箱与白名单校验的本地工作流</p>
        </div>
        <div id="preset-cards" class="preset-grid" aria-live="polite"></div>

        <form id="preset-form" class="card parameter-card">
          <div class="card-title"><span class="step-number">A</span><div><h3>参数</h3><p id="selected-preset-name">选择预设并设置输入输出</p></div></div>
          <div class="form-grid three">
            <label>输入文件<input id="input" value="input.mov" required autocomplete="off"></label>
            <label>输出文件<input id="output" value="output.mp4" required autocomplete="off"></label>
            <label>工作目录<input id="workdir" value="/tmp" required autocomplete="off"></label>
          </div>
          <div id="resolution-fields" class="form-grid two option-group">
            <label>画布宽度<input id="width" type="number" value="1920" min="1"></label>
            <label>画布高度<input id="height" type="number" value="1080" min="1"></label>
          </div>
          <div id="h264-fields" class="form-grid two option-group">
            <label>编码器<select id="encoder"><option value="libx264">libx264（软件）</option><option value="h264_videotoolbox">VideoToolbox（硬件）</option></select></label>
            <label>CRF<input id="crf" type="number" value="18" min="0" max="51"></label>
          </div>
          <div id="upscale-fields" class="form-grid three option-group" hidden>
            <label>超分引擎<select id="upscale-engine"><option value="realesrgan">Real-ESRGAN</option><option value="waifu2x">waifu2x</option></select></label>
            <label>倍率<select id="scale"><option value="2">2×</option><option value="3">3×</option><option value="4">4×</option></select></label>
            <label>重组帧率<input id="fps" type="number" value="30" min="1" step="0.001"></label>
          </div>
          <div id="upscale-crf" class="form-grid two option-group" hidden>
            <label>重组 CRF<input id="upscale-crf-value" type="number" value="18" min="0" max="51"></label>
          </div>
          <div id="loudnorm-fields" class="option-group" hidden>
            <p class="field-help">填入第一遍 loudnorm JSON 中的测量值；计划仍包含测量步骤，便于复核。</p>
            <div class="form-grid five">
              <label>measured_I<input id="input-i" type="number" value="-20" step="any"></label>
              <label>measured_TP<input id="input-tp" type="number" value="-2" step="any"></label>
              <label>measured_LRA<input id="input-lra" type="number" value="7" step="any"></label>
              <label>measured_thresh<input id="input-thresh" type="number" value="-30" step="any"></label>
              <label>offset<input id="target-offset" type="number" value="0.5" step="any"></label>
            </div>
          </div>
          <div class="form-actions"><p>所有相对路径均限定在工作目录内</p><button class="button primary" type="submit">生成执行计划 <span>→</span></button></div>
        </form>
      </section>

      <section id="panel-ai" class="panel" hidden>
        <div class="section-heading">
          <div><p class="eyebrow">BYOK PLANNER</p><h2>描述需求，保留决定权</h2></div>
          <p>只发送 ffprobe 元数据与需求文本，不发送媒体内容</p>
        </div>
        <div class="ai-layout">
          <form id="provider-form" class="card">
            <div class="card-title"><span class="step-number">A</span><div><h3>Provider 与 Key</h3><p>Mock 可离线演示；真实 HTTP 留白</p></div></div>
            <label>Provider<select id="provider-kind"><option value="mock">MockProvider · 本地演示</option><option value="openai">OpenAI 兼容接口 · 骨架</option></select></label>
            <div id="openai-settings" class="option-group" hidden>
              <label>Base URL<input id="base-url" type="url" value="https://api.openai.com/v1"></label>
              <label>Model<input id="model" value="gpt-4o-mini"></label>
              <label>API Key<input id="api-key" type="password" autocomplete="off" placeholder="只写入 macOS Keychain"></label>
              <div class="key-actions">
                <button id="save-key" class="button secondary" type="button">保存到 Keychain</button>
                <button id="delete-key" class="button ghost" type="button">删除 Key</button>
              </div>
            </div>
            <button id="test-provider" class="button ghost full" type="button">测试连接（占位）</button>
            <p id="provider-status" class="status-line">MockProvider 无需 Key，不会发起网络请求。</p>
            <label>AI 计划工作目录<input id="ai-workdir" value="/tmp" required></label>
          </form>
          <form id="ai-form" class="card">
            <div class="card-title"><span class="step-number">B</span><div><h3>媒体上下文与需求</h3><p>元数据在发送前会验证并限长</p></div></div>
            <label>ffprobe 元数据 JSON<textarea id="metadata" rows="8" spellcheck="false">{"streams":[],"format":{"duration":"12.0"}}</textarea></label>
            <label>需求文本<textarea id="requirement" rows="4" placeholder="例如：制作适合场馆备份播放的 H.264 文件" required></textarea></label>
            <p class="field-help">Mock 演示固定规划 input.mp4 → output.mp4；真实模型接入后仍须经过同一确认流。</p>
            <button class="button primary full" type="submit">生成并校验计划 <span>→</span></button>
          </form>
        </div>
      </section>

      <section id="panel-realtime" class="panel" hidden>
        <div class="development-banner">
          <div class="pulse-icon"><span></span></div>
          <p class="eyebrow">REALTIME ADAPTER / IN DEVELOPMENT</p>
          <h2>实时信号适配链路，正在开发中。</h2>
          <p>当前版本聚焦可完整使用的离线媒体工具箱。以下接口与边界已预留，不会假装已有实时处理能力。</p>
        </div>
        <div class="architecture card">
          <div class="arch-node"><small>INPUT SOURCE</small><strong>Syphon · NDI</strong><span>DeckLink · UVC</span></div>
          <div class="arch-link">GPU TEXTURE →</div>
          <div class="arch-node accent"><small>PROCESSING</small><strong>wgpu</strong><span>libplacebo 接入点</span></div>
          <div class="arch-link">GPU TEXTURE →</div>
          <div class="arch-node"><small>OUTPUT TARGET</small><strong>Syphon · NDI</strong><span>DeckLink · Preview</span></div>
        </div>
        <div class="realtime-notes">
          <article><span>01</span><h3>帧全程 GPU</h3><p>进入管线后不做阶段间 CPU readback；采集上传或 SDK 下载必须隔离、测量并记录。</p></article>
          <article><span>02</span><h3>显式 Unsupported</h3><p>RealtimePipeline 占位实现返回清晰错误，不 panic，也不使用 todo!()。</p></article>
          <article><span>03</span><h3>平台后端留白</h3><p>Syphon / NDI 输入输出、wgpu 设备与 libplacebo 色彩处理将在后续里程碑接入。</p></article>
        </div>
      </section>

      <section id="plan-workspace" class="plan-workspace" hidden>
        <div class="card plan-card">
          <div class="plan-header">
            <div class="card-title"><span class="step-number">B</span><div><h3>计划预览与确认</h3><p id="plan-id"></p></div></div>
            <span id="plan-state" class="state-badge">等待确认</span>
          </div>
          <div id="plan-steps" class="plan-steps"></div>
          <p class="argv-note">argv 仅供审查；实际执行使用独立参数，不经过 Shell。</p>
          <div class="form-actions">
            <button id="reject-plan" class="button ghost" type="button">拒绝计划</button>
            <button id="approve-plan" class="button primary" type="button">确认并执行 <span>▶</span></button>
            <button id="cancel-plan" class="button danger" type="button" hidden>取消执行</button>
          </div>
        </div>
        <div class="card execution-card">
          <div class="execution-heading"><div><p class="eyebrow">EXECUTION</p><h3>执行进度与日志</h3></div><button id="export-log" class="button ghost" type="button">导出完整日志</button></div>
          <div class="progress-track"><div id="progress-bar"></div></div>
          <p id="progress-status" class="status-line">尚未开始执行</p>
          <pre id="log-output" aria-live="polite">等待计划确认…</pre>
        </div>
      </section>
    </main>
    <footer><span>SignalFit / EventKyu</span><span>GPL-3.0 · LOCAL-FIRST</span></footer>
  </div>
`;

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) throw new Error(`Missing #${id}`);
  return found as T;
}

let presets: PresetDescriptor[] = [];
let selectedPreset: PresetId = "h264-1080p";
let activePanel = "toolbox";
let pending: ValidatedPlan | null = null;
let executing = false;
let logLines: string[] = [];
let logRenderQueued = false;

function showPanel(name: string): void {
  activePanel = name;
  document.querySelectorAll<HTMLButtonElement>(".tab").forEach((tab) => {
    tab.classList.toggle("active", tab.dataset.panel === name);
  });
  ["toolbox", "ai", "realtime"].forEach((panel) => {
    element(`panel-${panel}`).hidden = panel !== name;
  });
  element("plan-workspace").hidden = name === "realtime" || pending === null;
}

document.querySelectorAll<HTMLButtonElement>(".tab").forEach((tab) => {
  tab.addEventListener("click", () => showPanel(tab.dataset.panel ?? "toolbox"));
});
document.querySelector(".brand")?.addEventListener("click", (event) => {
  event.preventDefault();
  showPanel("toolbox");
});

function renderPresetCards(): void {
  const container = element("preset-cards");
  container.replaceChildren();
  for (const preset of presets) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `preset-card${preset.id === selectedPreset ? " selected" : ""}`;
    button.dataset.preset = preset.id;

    const top = document.createElement("span");
    top.className = "preset-top";
    const title = document.createElement("strong");
    title.textContent = preset.name;
    const marker = document.createElement("i");
    marker.textContent = preset.id === selectedPreset ? "✓" : "↗";
    top.append(title, marker);
    const description = document.createElement("span");
    description.textContent = preset.description;
    const tools = document.createElement("small");
    tools.textContent = preset.tools.join("  +  ");
    button.append(top, description, tools);
    button.addEventListener("click", () => selectPreset(preset.id));
    container.append(button);
  }
}

function selectPreset(id: PresetId): void {
  selectedPreset = id;
  const preset = presets.find((item) => item.id === id);
  element("selected-preset-name").textContent = preset?.description ?? "设置输入输出";
  element<HTMLInputElement>("output").value = id === "prores-422" ? "output.mov" : "output.mp4";
  element("resolution-fields").hidden = !["h264-1080p", "vertical-blur"].includes(id);
  element("h264-fields").hidden = id !== "h264-1080p";
  element("upscale-fields").hidden = id !== "superresolution";
  element("upscale-crf").hidden = id !== "superresolution";
  element("loudnorm-fields").hidden = id !== "loudnorm-two-pass";
  renderPresetCards();
}

function numericValue(id: string): number | undefined {
  const value = element<HTMLInputElement>(id).value;
  return value === "" ? undefined : Number(value);
}

function presetParams(): PresetParams {
  const params: PresetParams = {
    input: element<HTMLInputElement>("input").value,
    output: element<HTMLInputElement>("output").value,
    workdir: element<HTMLInputElement>("workdir").value,
    width: numericValue("width"),
    height: numericValue("height"),
  };
  if (selectedPreset === "h264-1080p") {
    params.crf = numericValue("crf");
    params.encoder = element<HTMLSelectElement>("encoder").value;
  }
  if (selectedPreset === "superresolution") {
    params.crf = numericValue("upscale-crf-value");
    params.upscaleEngine = element<HTMLSelectElement>("upscale-engine").value;
    params.scale = numericValue("scale");
    params.fps = numericValue("fps");
  }
  if (selectedPreset === "loudnorm-two-pass") {
    params.loudnorm = {
      input_i: Number(element<HTMLInputElement>("input-i").value),
      input_tp: Number(element<HTMLInputElement>("input-tp").value),
      input_lra: Number(element<HTMLInputElement>("input-lra").value),
      input_thresh: Number(element<HTMLInputElement>("input-thresh").value),
      target_offset: Number(element<HTMLInputElement>("target-offset").value),
    };
  }
  return params;
}

element<HTMLFormElement>("preset-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  await runAction("approve-plan", async () => {
    const plan = await invoke<ExecutionPlan>("preset_plan", {
      presetId: selectedPreset,
      params: presetParams(),
    });
    await preparePlan(plan);
  });
});

async function preparePlan(plan: ExecutionPlan): Promise<void> {
  pending = await invoke<ValidatedPlan>("plan_validate", { plan });
  executing = false;
  logLines = [];
  element("log-output").textContent = "计划已校验，等待你的确认…";
  element("progress-bar").style.width = "0%";
  element("progress-bar").classList.remove("indeterminate");
  element("progress-status").textContent = "尚未开始执行";
  element("plan-state").textContent = "等待确认";
  element("plan-state").className = "state-badge";
  renderPlan();
  showPanel(activePanel);
  element("plan-workspace").scrollIntoView({ behavior: "smooth", block: "start" });
}

function quoteForPreview(value: string): string {
  return /^[a-zA-Z0-9_./:+,=@%-]+$/.test(value) ? value : `'${value.replaceAll("'", `'"'"'`)}'`;
}

function renderPlan(): void {
  if (!pending) return;
  element("plan-id").textContent = `${pending.planId} · ${pending.plan.steps.length} 个步骤`;
  const container = element("plan-steps");
  container.replaceChildren();
  pending.plan.steps.forEach((step, index) => {
    const article = document.createElement("article");
    article.className = "plan-step";
    const header = document.createElement("div");
    const title = document.createElement("strong");
    title.textContent = `步骤 ${index + 1} / ${step.program}`;
    const estimate = document.createElement("span");
    estimate.textContent = step.estimated_minutes === null ? "耗时未预估" : `约 ${step.estimated_minutes} 分钟`;
    header.append(title, estimate);
    const explanation = document.createElement("p");
    explanation.textContent = step.explanation;
    const argv = document.createElement("code");
    argv.textContent = [step.program, ...step.args].map(quoteForPreview).join(" ");
    const workdir = document.createElement("small");
    workdir.textContent = `workdir: ${step.workdir}`;
    article.append(header, explanation, argv, workdir);
    container.append(article);
  });
  updateExecutionButtons();
}

function updateExecutionButtons(): void {
  element<HTMLButtonElement>("approve-plan").disabled = executing || !pending;
  element<HTMLButtonElement>("reject-plan").disabled = executing || !pending;
  element("approve-plan").hidden = executing;
  element("reject-plan").hidden = executing;
  element("cancel-plan").hidden = !executing;
}

element("approve-plan").addEventListener("click", async () => {
  if (!pending || executing) return;
  const planId = pending.planId;
  try {
    await invoke("plan_approve", { planId });
    executing = true;
    updateExecutionButtons();
    element("plan-state").textContent = "执行中";
    element("plan-state").className = "state-badge running";
    element("progress-bar").classList.add("indeterminate");
    element("progress-bar").style.width = "38%";
    element("progress-status").textContent = "本地进程已启动，正在等待输出…";
    logLines = [];
    renderLog();
    const report = await invoke<ExecutionReportView>("plan_execute", { planId });
    logLines = report.logText.split("\n");
    renderLog();
    element("progress-bar").classList.remove("indeterminate");
    element("progress-bar").style.width = "100%";
    element("progress-status").textContent = `执行完成 · ${report.attempts.length} 次进程调用`;
  } catch (error) {
    element("progress-bar").classList.remove("indeterminate");
    element("progress-status").textContent = errorMessage(error);
    if (logLines.length === 0) {
      logLines = [`[error] ${errorMessage(error)}`];
      renderLog();
    }
  } finally {
    executing = false;
    updateExecutionButtons();
  }
});

element("reject-plan").addEventListener("click", async () => {
  if (!pending || executing) return;
  try {
    await invoke("plan_reject", { planId: pending.planId });
    pending = null;
    element("plan-workspace").hidden = true;
  } catch (error) {
    element("progress-status").textContent = errorMessage(error);
  }
});

element("cancel-plan").addEventListener("click", async () => {
  if (!pending || !executing) return;
  try {
    await invoke("plan_cancel", { planId: pending.planId });
    element("progress-status").textContent = "已请求取消，正在终止子进程…";
  } catch (error) {
    element("progress-status").textContent = errorMessage(error);
  }
});

function renderLog(): void {
  element("log-output").textContent = logLines.length ? logLines.join("\n") : "进程运行中，等待日志…";
  const log = element("log-output");
  log.scrollTop = log.scrollHeight;
}

function queueLogRender(): void {
  if (logRenderQueued) return;
  logRenderQueued = true;
  requestAnimationFrame(() => {
    logRenderQueued = false;
    renderLog();
  });
}

element("export-log").addEventListener("click", async () => {
  try {
    const text = await invoke<string>("export_log");
    const blob = new Blob([text], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `signalfit-${new Date().toISOString().replaceAll(":", "-")}.log.txt`;
    link.click();
    URL.revokeObjectURL(url);
  } catch (error) {
    element("progress-status").textContent = errorMessage(error);
  }
});

element<HTMLSelectElement>("provider-kind").addEventListener("change", () => {
  const isOpenAi = element<HTMLSelectElement>("provider-kind").value === "openai";
  element("openai-settings").hidden = !isOpenAi;
  element("provider-status").textContent = isOpenAi
    ? "Key 只进入系统 Keychain；真实 HTTP 请求尚未接入。"
    : "MockProvider 无需 Key，不会发起网络请求。";
});

element("save-key").addEventListener("click", async () => {
  const input = element<HTMLInputElement>("api-key");
  try {
    await invoke("key_set", { provider: "openai", key: input.value });
    input.value = "";
    element("provider-status").textContent = "API Key 已保存到 macOS Keychain，界面不会保留其内容。";
  } catch (error) {
    element("provider-status").textContent = errorMessage(error);
  }
});

element("delete-key").addEventListener("click", async () => {
  try {
    const deleted = await invoke<boolean>("key_delete", { provider: "openai" });
    element("provider-status").textContent = deleted ? "Keychain 中的 API Key 已删除。" : "Keychain 中没有可删除的 API Key。";
  } catch (error) {
    element("provider-status").textContent = errorMessage(error);
  }
});

element("test-provider").addEventListener("click", () => {
  const isMock = element<HTMLSelectElement>("provider-kind").value === "mock";
  element("provider-status").textContent = isMock
    ? "MockProvider 本地可用，无需连接测试。"
    : "真实 HTTP 连接测试尚未实现；此处仅为交互占位。";
});

element<HTMLFormElement>("provider-form").addEventListener("submit", (event) => event.preventDefault());
element<HTMLFormElement>("ai-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const kind = element<HTMLSelectElement>("provider-kind").value;
  try {
    const plan = await invoke<ExecutionPlan>("ai_generate", {
      config: {
        kind,
        baseUrl: element<HTMLInputElement>("base-url").value,
        model: element<HTMLInputElement>("model").value,
        workdir: element<HTMLInputElement>("ai-workdir").value,
      },
      ffprobeMetadata: element<HTMLTextAreaElement>("metadata").value,
      requirement: element<HTMLTextAreaElement>("requirement").value,
    });
    await preparePlan(plan);
  } catch (error) {
    element("provider-status").textContent = errorMessage(error);
  }
});

async function runAction(_buttonId: string, action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    element("progress-status").textContent = errorMessage(error);
    if (!pending) {
      pending = null;
      element("plan-workspace").hidden = true;
      window.alert(errorMessage(error));
    }
  }
}

function errorMessage(error: unknown): string {
  return typeof error === "string" ? error : error instanceof Error ? error.message : String(error);
}

await listen<OutputEvent>("signalfit-output", (event) => {
  if (event.payload.planId !== pending?.planId) return;
  logLines.push(`[${event.payload.stream}] ${event.payload.line}`);
  queueLogRender();
});

await listen<ProgressEvent>("signalfit-progress", (event) => {
  if (event.payload.planId !== pending?.planId) return;
  const update = event.payload.progress;
  if (update.progress === "end") {
    element("progress-bar").classList.remove("indeterminate");
    element("progress-bar").style.width = "100%";
  }
  const fields = [
    update.frame === null ? null : `frame ${update.frame}`,
    update.fps === null ? null : `${update.fps.toFixed(2)} fps`,
    update.out_time_ms === null ? null : `out_time_ms ${update.out_time_ms}`,
  ].filter((value): value is string => value !== null);
  if (fields.length) element("progress-status").textContent = fields.join(" · ");
});

await listen<StateEvent>("signalfit-state", (event) => {
  if (event.payload.planId !== pending?.planId) return;
  const labels: Record<StateEvent["state"], string> = {
    executing: "执行中",
    done: "已完成",
    failed: "失败",
    cancelled: "已取消",
  };
  element("plan-state").textContent = labels[event.payload.state];
  element("plan-state").className = `state-badge ${event.payload.state}`;
  if (event.payload.message) element("progress-status").textContent = event.payload.message;
});

try {
  presets = await invoke<PresetDescriptor[]>("list_presets");
  renderPresetCards();
  selectPreset(selectedPreset);
} catch (error) {
  element("preset-cards").textContent = `无法加载预设：${errorMessage(error)}。请通过 Tauri 应用打开此界面。`;
}
