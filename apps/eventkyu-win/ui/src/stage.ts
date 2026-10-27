import "./stage.css";
import { engineStatus } from "./api";
import type { EngineSnapshot } from "./api";

function byId(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error(`missing stage UI element #${id}`);
  return element;
}

const nameOutput = byId("stage-cue-name");
const stateOutput = byId("stage-state");
const kindOutput = byId("stage-cue-kind");
const errorOutput = byId("stage-error");

let busy = false;

function render(snapshot: EngineSnapshot): void {
  const running = snapshot.cues.find((cue) => cue.state === "running");
  nameOutput.textContent = running?.name ?? "待机";
  stateOutput.textContent = running ? "NOW RUNNING" : "STANDBY";
  kindOutput.textContent = running
    ? `${running.kind.toUpperCase()} · CUE ${running.id}`
    : "等待运行中的 Cue";
  document.body.dataset.active = String(Boolean(running));
}

async function poll(): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    render(await engineStatus());
    errorOutput.textContent = "";
  } catch (error) {
    errorOutput.textContent = error instanceof Error ? error.message : String(error);
  } finally {
    busy = false;
  }
}

void poll();
window.setInterval(() => void poll(), 50);
