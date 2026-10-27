use crate::{CueError, CueId, CueState};

/// Media operation requested from an external audio/video adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    /// Begin playback from the start.
    Play,
    /// Pause playback.
    Pause,
    /// Resume paused playback.
    Resume,
    /// Stop playback and release playback resources.
    Stop,
}

/// Notifications emitted by the engine for UI, FFI, and media subscribers.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// A cue changed public state.
    CueStateChanged {
        /// Affected cue.
        cue_id: CueId,
        /// New state.
        state: CueState,
        /// Event timestamp in milliseconds.
        at_ms: u64,
    },
    /// A cue completed naturally after its post-wait.
    CueCompleted {
        /// Completed cue.
        cue_id: CueId,
        /// Event timestamp in milliseconds.
        at_ms: u64,
    },
    /// A running cue's interpolated media parameters changed.
    FadeUpdated {
        /// Cue whose parameters changed (the target, not the fade cue).
        cue_id: CueId,
        /// Current volume in the range 0.0 through 1.0.
        volume: f32,
        /// Current opacity in the range 0.0 through 1.0.
        opacity: f32,
        /// Event timestamp in milliseconds.
        at_ms: u64,
    },
    /// An external media adapter should perform an operation.
    MediaActionRequested {
        /// Media cue to operate on.
        cue_id: CueId,
        /// Requested operation.
        action: MediaAction,
        /// Event timestamp in milliseconds.
        at_ms: u64,
    },
    /// An operation or automatic transition failed.
    Error {
        /// Related cue, when one exists.
        cue_id: Option<CueId>,
        /// Error details.
        error: CueError,
        /// Event timestamp in milliseconds.
        at_ms: u64,
    },
}
