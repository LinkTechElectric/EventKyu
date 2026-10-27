import { invoke } from "@tauri-apps/api/core";

export type CueKind = "audio" | "video" | "wait";
export type CueState =
  | "stopped"
  | "preWait"
  | "running"
  | "postWait"
  | "paused"
  | "completed"
  | "error";

export interface CueSnapshot {
  id: number;
  name: string;
  kind: string;
  source: string | null;
  state: CueState;
  selected: boolean;
  volume: number;
  opacity: number;
  phaseRemainingMs: number | null;
}

export interface EngineSnapshot {
  nowMs: number;
  playheadIndex: number | null;
  cues: CueSnapshot[];
}

export interface AddCueInput {
  name: string;
  kind: CueKind;
  source?: string | null;
  preWaitMs: number;
  durationMs: number | null;
  postWaitMs: number;
}

export type EngineEvent = Record<string, unknown> & { type: string };

export interface StageFullscreenState {
  fullscreen: boolean;
  activeMonitor: unknown;
  monitors: unknown[];
}

export const engineAddCue = (input: AddCueInput) =>
  invoke<EngineSnapshot>("engine_add_cue", { input });
export const engineGo = () => invoke<EngineSnapshot>("engine_go");
export const engineTick = () => invoke<EngineSnapshot>("engine_tick");
export const engineStatus = () => invoke<EngineSnapshot>("engine_status");
export const engineEvents = () => invoke<EngineEvent[]>("engine_events");
export const downloadResolve = (url: string) =>
  invoke<unknown>("download_resolve", { url });
export const runtimeSelfcheck = () => invoke<unknown>("runtime_selfcheck");
export const stageToggleFullscreen = () =>
  invoke<StageFullscreenState>("stage_toggle_fullscreen");
