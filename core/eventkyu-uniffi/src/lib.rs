//! Thread-safe UniFFI surface for EventKyu's media-agnostic cue engine.

#![forbid(unsafe_code)]

use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use cue_core::{
    AutoContinue as CoreAutoContinue, Cue, CueKind as CoreCueKind, CueState as CoreCueState,
    Engine, EngineEvent as CoreEngineEvent, FadeCurve, MediaAction as CoreMediaAction,
};

uniffi::setup_scaffolding!();

/// Cue kinds accepted by the M0 FFI surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CueKind {
    Audio,
    Video,
    Wait,
}

/// Automatic continuation behavior for a cue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Enum)]
pub enum AutoContinue {
    #[default]
    None,
    Continue,
    Follow,
}

/// Public runtime state of a cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CueState {
    Stopped,
    PreWait,
    Running,
    PostWait,
    Paused,
    Completed,
    Error,
}

/// Media operation requested by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MediaAction {
    Play,
    Pause,
    Resume,
    Stop,
}

/// Discriminator for an [`EngineEvent`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EngineEventKind {
    CueStateChanged,
    CueCompleted,
    FadeUpdated,
    MediaActionRequested,
    Error,
}

/// Snapshot of one cue, suitable for rendering a native cue list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CueStatus {
    pub cue_id: String,
    pub name: String,
    pub kind: String,
    pub state: CueState,
    pub phase_remaining_ms: Option<u64>,
    pub volume: f32,
    pub opacity: f32,
}

/// Snapshot of the complete engine and its current playhead.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EngineStatus {
    pub cues: Vec<CueStatus>,
    pub playhead_cue_id: Option<String>,
    pub now_ms: u64,
}

/// Flat event representation that keeps all event variants FFI-friendly.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EngineEvent {
    pub kind: EngineEventKind,
    pub cue_id: Option<String>,
    pub at_ms: u64,
    pub state: Option<CueState>,
    pub media_action: Option<MediaAction>,
    pub volume: Option<f32>,
    pub opacity: Option<f32>,
    pub message: Option<String>,
}

impl EngineEvent {
    fn new(kind: EngineEventKind, cue_id: Option<u64>, at_ms: u64) -> Self {
        Self {
            kind,
            cue_id: cue_id.map(|id| id.to_string()),
            at_ms,
            state: None,
            media_action: None,
            volume: None,
            opacity: None,
            message: None,
        }
    }
}

/// Errors crossing the FFI boundary as Swift errors.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
#[uniffi(flat_error)]
pub enum EventKyuError {
    InvalidCueId(String),
    Engine(String),
    LockPoisoned,
}

impl fmt::Display for EventKyuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCueId(id) => write!(formatter, "invalid cue id: {id}"),
            Self::Engine(message) => formatter.write_str(message),
            Self::LockPoisoned => formatter.write_str("cue engine lock is poisoned"),
        }
    }
}

impl std::error::Error for EventKyuError {}

impl From<cue_core::CueError> for EventKyuError {
    fn from(error: cue_core::CueError) -> Self {
        Self::Engine(error.to_string())
    }
}

struct HandleState {
    engine: Engine,
    next_cue_id: u64,
}

/// Send + Sync cue engine handle exported to Swift.
#[derive(uniffi::Object)]
pub struct CueEngineHandle {
    state: Mutex<HandleState>,
}

#[uniffi::export]
impl CueEngineHandle {
    /// Creates an empty engine backed by the system monotonic clock.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(HandleState {
                engine: Engine::new(),
                next_cue_id: 1,
            }),
        })
    }

    /// Adds an audio, video, or wait cue and returns its stable string ID.
    #[allow(clippy::too_many_arguments)] // UniFFI 边界：参数即跨语言契约，保持平铺
    pub fn add_cue(
        &self,
        kind: CueKind,
        name: String,
        source: String,
        pre_wait_ms: u64,
        duration_ms: Option<u64>,
        post_wait_ms: u64,
        auto_continue: Option<AutoContinue>,
    ) -> Result<String, EventKyuError> {
        let mut state = self.lock()?;
        let id = state.next_cue_id;
        let kind = match kind {
            CueKind::Audio => CoreCueKind::audio(source),
            CueKind::Video => CoreCueKind::video(source),
            CueKind::Wait => CoreCueKind::Wait,
        };
        let auto_continue = match auto_continue.unwrap_or_default() {
            AutoContinue::None => CoreAutoContinue::None,
            AutoContinue::Continue => CoreAutoContinue::Continue,
            AutoContinue::Follow => CoreAutoContinue::Follow,
        };
        let cue = Cue::new(id, name, kind)
            .with_pre_wait_ms(pre_wait_ms)
            .with_duration(duration_ms)
            .with_post_wait_ms(post_wait_ms)
            .with_auto_continue(auto_continue);
        state.engine.add_cue(cue)?;
        state.next_cue_id = state.next_cue_id.saturating_add(1);
        Ok(id.to_string())
    }

    /// Triggers the playhead cue and returns its ID.
    pub fn go(&self) -> Result<String, EventKyuError> {
        Ok(self.lock()?.engine.go()?.to_string())
    }

    /// Advances every cue using the system monotonic clock.
    pub fn tick_now(&self) -> Result<(), EventKyuError> {
        self.lock()?.engine.tick_now()?;
        Ok(())
    }

    /// Starts a cue, optionally with a linear fade-in.
    pub fn start(&self, cue_id: String, fade_ms: Option<u64>) -> Result<(), EventKyuError> {
        let id = parse_cue_id(&cue_id)?;
        let mut state = self.lock()?;
        match fade_ms {
            Some(duration_ms) => state
                .engine
                .start_faded(id, duration_ms, FadeCurve::Linear)?,
            None => state.engine.start(id)?,
        }
        Ok(())
    }

    /// Stops a cue, optionally with a linear fade-out.
    pub fn stop(&self, cue_id: String, fade_ms: Option<u64>) -> Result<(), EventKyuError> {
        let id = parse_cue_id(&cue_id)?;
        let mut state = self.lock()?;
        match fade_ms {
            Some(duration_ms) => state
                .engine
                .stop_faded(id, duration_ms, FadeCurve::Linear)?,
            None => state.engine.stop(id)?,
        }
        Ok(())
    }

    /// Pauses a cue, optionally fading out before timing freezes.
    pub fn pause(&self, cue_id: String, fade_ms: Option<u64>) -> Result<(), EventKyuError> {
        let id = parse_cue_id(&cue_id)?;
        let mut state = self.lock()?;
        match fade_ms {
            Some(duration_ms) => state
                .engine
                .pause_faded(id, duration_ms, FadeCurve::Linear)?,
            None => state.engine.pause(id)?,
        }
        Ok(())
    }

    /// Resumes a cue, optionally with a linear fade-in.
    pub fn resume(&self, cue_id: String, fade_ms: Option<u64>) -> Result<(), EventKyuError> {
        let id = parse_cue_id(&cue_id)?;
        let mut state = self.lock()?;
        match fade_ms {
            Some(duration_ms) => state
                .engine
                .resume_faded(id, duration_ms, FadeCurve::Linear)?,
            None => state.engine.resume(id)?,
        }
        Ok(())
    }

    /// Reports completion from an external media adapter.
    pub fn notify_media_completed(&self, cue_id: String) -> Result<(), EventKyuError> {
        self.lock()?
            .engine
            .notify_media_completed(parse_cue_id(&cue_id)?)?;
        Ok(())
    }

    /// Takes an atomic snapshot of all cues and the playhead.
    pub fn status(&self) -> Result<EngineStatus, EventKyuError> {
        let state = self.lock()?;
        let cues = state
            .engine
            .list()
            .iter()
            .map(|id| {
                let cue = state.engine.cue(*id).ok_or_else(|| {
                    EventKyuError::Engine(format!("cue {id} is missing from the engine"))
                })?;
                let status = state.engine.status(*id)?;
                Ok(CueStatus {
                    cue_id: id.to_string(),
                    name: cue.name().to_owned(),
                    kind: cue_kind_name(cue.kind()).to_owned(),
                    state: status.state.into(),
                    phase_remaining_ms: status.phase_remaining_ms,
                    volume: status.volume,
                    opacity: status.opacity,
                })
            })
            .collect::<Result<Vec<_>, EventKyuError>>()?;
        Ok(EngineStatus {
            cues,
            playhead_cue_id: state.engine.playhead_cue().map(|id| id.to_string()),
            now_ms: state.engine.now_ms(),
        })
    }

    /// Removes and returns all pending events in emission order.
    pub fn drain_events(&self) -> Result<Vec<EngineEvent>, EventKyuError> {
        Ok(self
            .lock()?
            .engine
            .drain_events()
            .into_iter()
            .map(EngineEvent::from)
            .collect())
    }
}

impl CueEngineHandle {
    fn lock(&self) -> Result<MutexGuard<'_, HandleState>, EventKyuError> {
        self.state.lock().map_err(|_| EventKyuError::LockPoisoned)
    }
}

/// Returns the EventKyu core package version.
#[uniffi::export]
pub fn eventkyu_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn parse_cue_id(cue_id: &str) -> Result<u64, EventKyuError> {
    cue_id
        .parse()
        .map_err(|_| EventKyuError::InvalidCueId(cue_id.to_owned()))
}

fn cue_kind_name(kind: &CoreCueKind) -> &'static str {
    match kind {
        CoreCueKind::Audio { .. } => "Audio",
        CoreCueKind::Video { .. } => "Video",
        CoreCueKind::Wait => "Wait",
        CoreCueKind::Fade { .. } => "Fade",
        CoreCueKind::Group { .. } => "Group",
    }
}

impl From<CoreCueState> for CueState {
    fn from(state: CoreCueState) -> Self {
        match state {
            CoreCueState::Stopped => Self::Stopped,
            CoreCueState::PreWait => Self::PreWait,
            CoreCueState::Running => Self::Running,
            CoreCueState::PostWait => Self::PostWait,
            CoreCueState::Paused => Self::Paused,
            CoreCueState::Completed => Self::Completed,
            CoreCueState::Error => Self::Error,
        }
    }
}

impl From<CoreMediaAction> for MediaAction {
    fn from(action: CoreMediaAction) -> Self {
        match action {
            CoreMediaAction::Play => Self::Play,
            CoreMediaAction::Pause => Self::Pause,
            CoreMediaAction::Resume => Self::Resume,
            CoreMediaAction::Stop => Self::Stop,
        }
    }
}

impl From<CoreEngineEvent> for EngineEvent {
    fn from(event: CoreEngineEvent) -> Self {
        match event {
            CoreEngineEvent::CueStateChanged {
                cue_id,
                state,
                at_ms,
            } => Self {
                state: Some(state.into()),
                ..Self::new(EngineEventKind::CueStateChanged, Some(cue_id), at_ms)
            },
            CoreEngineEvent::CueCompleted { cue_id, at_ms } => {
                Self::new(EngineEventKind::CueCompleted, Some(cue_id), at_ms)
            }
            CoreEngineEvent::FadeUpdated {
                cue_id,
                volume,
                opacity,
                at_ms,
            } => Self {
                volume: Some(volume),
                opacity: Some(opacity),
                ..Self::new(EngineEventKind::FadeUpdated, Some(cue_id), at_ms)
            },
            CoreEngineEvent::MediaActionRequested {
                cue_id,
                action,
                at_ms,
            } => Self {
                media_action: Some(action.into()),
                ..Self::new(EngineEventKind::MediaActionRequested, Some(cue_id), at_ms)
            },
            CoreEngineEvent::Error {
                cue_id,
                error,
                at_ms,
            } => Self {
                message: Some(error.to_string()),
                ..Self::new(EngineEventKind::Error, cue_id, at_ms)
            },
        }
    }
}

#[cfg(test)]
mod tests;
