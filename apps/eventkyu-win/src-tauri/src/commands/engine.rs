use cue_core::{Cue, CueId, CueKind, CueState, Engine, EngineEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::State;

pub type EngineState = Mutex<Engine>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddCueInput {
    pub name: String,
    pub kind: AddCueKind,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub pre_wait_ms: u64,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub post_wait_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddCueKind {
    Audio,
    Video,
    Wait,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CueSnapshot {
    pub id: CueId,
    pub name: String,
    pub kind: String,
    pub source: Option<String>,
    pub state: String,
    pub selected: bool,
    pub volume: f32,
    pub opacity: f32,
    pub phase_remaining_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineSnapshot {
    pub now_ms: u64,
    pub playhead_index: Option<usize>,
    pub cues: Vec<CueSnapshot>,
}

fn lock_error(error: impl std::fmt::Display) -> String {
    format!("cue engine lock failed: {error}")
}

fn next_cue_id(engine: &Engine) -> Result<CueId, String> {
    engine
        .list()
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| "cue id space exhausted".to_owned())
}

fn build_cue(id: CueId, input: AddCueInput) -> Result<Cue, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("cue name cannot be empty".to_owned());
    }

    let kind = match input.kind {
        AddCueKind::Audio => CueKind::audio(required_source(input.source)?),
        AddCueKind::Video => CueKind::video(required_source(input.source)?),
        AddCueKind::Wait => {
            if input.duration_ms.is_none() {
                return Err("wait cues require durationMs".to_owned());
            }
            CueKind::Wait
        }
    };

    let mut cue = Cue::new(id, name, kind)
        .with_pre_wait_ms(input.pre_wait_ms)
        .with_post_wait_ms(input.post_wait_ms);
    if let Some(duration_ms) = input.duration_ms {
        cue = cue.with_duration_ms(duration_ms);
    }
    Ok(cue)
}

fn required_source(source: Option<String>) -> Result<String, String> {
    source
        .map(|source| source.trim().to_owned())
        .filter(|source| !source.is_empty())
        .ok_or_else(|| "audio and video cues require a source".to_owned())
}

fn state_name(state: CueState) -> &'static str {
    match state {
        CueState::Stopped => "stopped",
        CueState::PreWait => "preWait",
        CueState::Running => "running",
        CueState::PostWait => "postWait",
        CueState::Paused => "paused",
        CueState::Completed => "completed",
        CueState::Error => "error",
    }
}

fn snapshot(engine: &Engine) -> Result<EngineSnapshot, String> {
    let playhead = engine.playhead_cue();
    let cues = engine
        .list()
        .iter()
        .map(|id| {
            let cue = engine
                .cue(*id)
                .ok_or_else(|| format!("cue definition missing for {id}"))?;
            let status = engine.status(*id).map_err(|error| error.to_string())?;
            let (kind, source) = match cue.kind() {
                CueKind::Audio { source } => ("audio", Some(source.clone())),
                CueKind::Video { source } => ("video", Some(source.clone())),
                CueKind::Wait => ("wait", None),
                CueKind::Fade { .. } => ("fade", None),
                CueKind::Group { .. } => ("group", None),
            };
            Ok(CueSnapshot {
                id: *id,
                name: cue.name().to_owned(),
                kind: kind.to_owned(),
                source,
                state: state_name(status.state).to_owned(),
                selected: playhead == Some(*id),
                volume: status.volume,
                opacity: status.opacity,
                phase_remaining_ms: status.phase_remaining_ms,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(EngineSnapshot {
        now_ms: engine.now_ms(),
        playhead_index: engine.playhead_index(),
        cues,
    })
}

fn event_json(event: EngineEvent) -> Value {
    match event {
        EngineEvent::CueStateChanged {
            cue_id,
            state,
            at_ms,
        } => json!({
            "type": "cueStateChanged",
            "cueId": cue_id,
            "state": state_name(state),
            "atMs": at_ms,
        }),
        EngineEvent::CueCompleted { cue_id, at_ms } => json!({
            "type": "cueCompleted",
            "cueId": cue_id,
            "atMs": at_ms,
        }),
        EngineEvent::FadeUpdated {
            cue_id,
            volume,
            opacity,
            at_ms,
        } => json!({
            "type": "fadeUpdated",
            "cueId": cue_id,
            "volume": volume,
            "opacity": opacity,
            "atMs": at_ms,
        }),
        EngineEvent::MediaActionRequested {
            cue_id,
            action,
            at_ms,
        } => json!({
            "type": "mediaActionRequested",
            "cueId": cue_id,
            "action": format!("{action:?}"),
            "atMs": at_ms,
        }),
        EngineEvent::Error {
            cue_id,
            error,
            at_ms,
        } => json!({
            "type": "error",
            "cueId": cue_id,
            "error": error.to_string(),
            "atMs": at_ms,
        }),
    }
}

#[tauri::command]
pub fn engine_add_cue(
    input: AddCueInput,
    state: State<'_, EngineState>,
) -> Result<EngineSnapshot, String> {
    let mut engine = state.lock().map_err(lock_error)?;
    let id = next_cue_id(&engine)?;
    let cue = build_cue(id, input)?;
    engine.add_cue(cue).map_err(|error| error.to_string())?;
    snapshot(&engine)
}

#[tauri::command]
pub fn engine_go(state: State<'_, EngineState>) -> Result<EngineSnapshot, String> {
    let mut engine = state.lock().map_err(lock_error)?;
    engine.go().map_err(|error| error.to_string())?;
    snapshot(&engine)
}

#[tauri::command]
pub fn engine_tick(state: State<'_, EngineState>) -> Result<EngineSnapshot, String> {
    let mut engine = state.lock().map_err(lock_error)?;
    engine.tick_now().map_err(|error| error.to_string())?;
    snapshot(&engine)
}

#[tauri::command]
pub fn engine_status(state: State<'_, EngineState>) -> Result<EngineSnapshot, String> {
    let engine = state.lock().map_err(lock_error)?;
    snapshot(&engine)
}

#[tauri::command]
pub fn engine_events(state: State<'_, EngineState>) -> Result<Vec<Value>, String> {
    let mut engine = state.lock().map_err(lock_error)?;
    Ok(engine.drain_events().into_iter().map(event_json).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cue_core::ManualClock;
    use serde_json::json;

    #[test]
    fn cue_status_snapshot_serializes_camel_case_state() {
        let clock = ManualClock::new(100);
        let mut engine = Engine::with_clock(clock);
        engine
            .add_cue(
                Cue::new(7, "Opening", CueKind::Wait)
                    .with_pre_wait_ms(250)
                    .with_duration_ms(1_500),
            )
            .unwrap();
        engine.go().unwrap();

        let value = serde_json::to_value(snapshot(&engine).unwrap()).unwrap();
        assert_eq!(value["nowMs"], 100);
        assert_eq!(value["cues"][0]["id"], 7);
        assert_eq!(value["cues"][0]["state"], "preWait");
        assert_eq!(value["cues"][0]["phaseRemainingMs"], 250);
        assert!(value["cues"][0].get("phase_remaining_ms").is_none());
    }

    #[test]
    fn engine_events_are_ui_ready_json() {
        let value = event_json(EngineEvent::CueStateChanged {
            cue_id: 3,
            state: CueState::Running,
            at_ms: 42,
        });
        assert_eq!(
            value,
            json!({"type":"cueStateChanged","cueId":3,"state":"running","atMs":42})
        );
    }

    #[test]
    fn wait_cue_requires_a_duration_before_mutating_engine() {
        let error = build_cue(
            1,
            AddCueInput {
                name: "Hold".into(),
                kind: AddCueKind::Wait,
                source: None,
                pre_wait_ms: 0,
                duration_ms: None,
                post_wait_ms: 0,
            },
        )
        .unwrap_err();
        assert!(error.contains("durationMs"));
    }
}
