import "./style.css";
import {
  downloadResolve,
  engineAddCue,
  engineEvents,
  engineGo,
  engineTick,
  runtimeSelfcheck,
  stageToggleFullscreen,
} from "./api";
import type { CueKind, CueSnapshot, EngineEvent, EngineSnapshot } from "./api";

function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`missing UI element #${id}`);
  return element as T;
}

const cueList = byId<HTMLTableSectionElement>("cue-list");
const emptyCues = byId("empty-cues");
const goButton = byId<HTMLButtonElement>("go");
const form = byId<HTMLFormElement>("cue-form");
const kindInput = byId<HTMLSelectElement>("cue-kind");
const sourceInput = byId<HTMLInputElement>("cue-source");
const sourceField = byId("source-field");
const errorOutput = byId("last-error");
const eventLog = byId("event-log");
const connection = byId("connection");
const events: EngineEvent[] = [];
let actionBusy = false;
let pollBusy = false;
let hasSnapshot = false;

const stateLabels: Record<string, string> = {
  stopped: "停止",
  preWait: "预备",
  running: "运行中",
  postWait: "后等待",
  paused: "暂停",
  completed: "完成",
  error: "错误",
};

function formatRemaining(cue: CueSnapshot): string {
  if (cue.phaseRemainingMs === null) return "—";
  return `${(cue.phaseRemainingMs / 1000).toFixed(1)} s`;
}

function makeCell(text: string, className = ""): HTMLTableCellElement {
  const cell = document.createElement("td");
  cell.textContent = text;
  cell.className = className;
  return cell;
}

function renderCue(cue: CueSnapshot, index: number): HTMLTableRowElement {
  const row = document.createElement("tr");
  row.className = `cue-row state-${cue.state}`;
  if (cue.selected) row.classList.add("selected");
  row.append(
    makeCell(String(index + 1).padStart(2, "0"), "index-cell"),
    makeCell(cue.name, "name-cell"),
    makeCell(cue.kind.toUpperCase()),
    makeCell(stateLabels[cue.state] ?? cue.state, "state-cell"),
    makeCell(formatRemaining(cue), "remaining-cell"),
  );
  return row;
}

function render(snapshot: EngineSnapshot): void {
  hasSnapshot = true;
  cueList.replaceChildren(...snapshot.cues.map(renderCue));
  emptyCues.hidden = snapshot.cues.length > 0;
  byId("cue-count").textContent = `${snapshot.cues.length} CUES`;
  byId("clock").textContent = `Engine ${snapshot.nowMs.toLocaleString()} ms`;
  byId("running-count").textContent = String(
    snapshot.cues.filter((cue) => cue.state === "running").length,
  );
  byId("paused-count").textContent = String(
    snapshot.cues.filter((cue) => cue.state === "paused").length,
  );
  byId("completed-count").textContent = String(
    snapshot.cues.filter((cue) => cue.state === "completed").length,
  );
  const next = snapshot.cues.find((cue) => cue.selected);
  byId("next-cue").textContent = next?.name ?? "列表已走完";
  goButton.disabled = !next || actionBusy;
}

function showError(error: unknown): void {
  errorOutput.textContent = error instanceof Error ? error.message : String(error);
}

function clearError(): void {
  errorOutput.textContent = "";
}

function appendEvents(incoming: EngineEvent[]): void {
  if (incoming.length === 0) return;
  events.push(...incoming);
  if (events.length > 200) events.splice(0, events.length - 200);
  eventLog.textContent = events
    .slice()
    .reverse()
    .map((event) => JSON.stringify(event))
    .join("\n");
}

async function poll(): Promise<void> {
  if (pollBusy) return;
  pollBusy = true;
  try {
    const [snapshot, incoming] = await Promise.all([engineTick(), engineEvents()]);
    render(snapshot);
    appendEvents(incoming);
    connection.textContent = "引擎在线";
    connection.classList.add("online");
  } catch (error) {
    connection.textContent = "引擎连接异常";
    connection.classList.remove("online");
    showError(error);
  } finally {
    pollBusy = false;
  }
}

async function runAction(action: () => Promise<void>): Promise<void> {
  if (actionBusy) return;
  actionBusy = true;
  goButton.disabled = true;
  try {
    await action();
    clearError();
  } catch (error) {
    showError(error);
  } finally {
    actionBusy = false;
  }
}

function optionalNumber(input: HTMLInputElement): number | null {
  return input.value.trim() === "" ? null : Number(input.value);
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  void runAction(async () => {
    const kind = kindInput.value as CueKind;
    const snapshot = await engineAddCue({
      name: byId<HTMLInputElement>("cue-name").value,
      kind,
      source: kind === "wait" ? null : sourceInput.value,
      durationMs: optionalNumber(byId<HTMLInputElement>("cue-duration")),
      preWaitMs: Number(byId<HTMLInputElement>("cue-pre-wait").value),
      postWaitMs: Number(byId<HTMLInputElement>("cue-post-wait").value),
    });
    render(snapshot);
  });
});

kindInput.addEventListener("change", () => {
  const isWait = kindInput.value === "wait";
  sourceField.hidden = isWait;
  sourceInput.required = !isWait;
});

goButton.addEventListener("click", () => {
  void runAction(async () => render(await engineGo()));
});

document.addEventListener("keydown", (event) => {
  const target = event.target as HTMLElement | null;
  if (
    event.code === "Space" &&
    !event.repeat &&
    hasSnapshot &&
    !target?.matches("input, select, textarea, button, summary")
  ) {
    event.preventDefault();
    void runAction(async () => render(await engineGo()));
  }
});

byId("stage-toggle").addEventListener("click", () => {
  void runAction(async () => {
    const result = await stageToggleFullscreen();
    appendEvents([{ type: "stageFullscreen", fullscreen: result.fullscreen }]);
  });
});

byId("download-resolve").addEventListener("click", () => {
  void runAction(async () => {
    const output = byId("download-output");
    output.textContent = "正在生成计划…";
    const result = await downloadResolve(byId<HTMLInputElement>("download-url").value);
    output.textContent = JSON.stringify(result, null, 2);
  });
});

byId("runtime-selfcheck").addEventListener("click", () => {
  void runAction(async () => {
    const output = byId("runtime-output");
    output.textContent = "正在读取 manifest 并检查私有安装…";
    output.textContent = JSON.stringify(await runtimeSelfcheck(), null, 2);
  });
});

sourceInput.required = true;
void poll();
window.setInterval(() => void poll(), 50);
