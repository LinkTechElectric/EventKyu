use std::fmt;

/// Stable identifier of a cue within one engine.
pub type CueId = u64;

/// Automatic continuation behavior after this cue starts or finishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoContinue {
    /// Do not trigger another cue automatically.
    #[default]
    None,
    /// Trigger the next list cue after post-wait, counted from action start.
    ///
    /// The current cue's action may still be running when the next cue starts.
    Continue,
    /// Trigger the next list cue as soon as this cue's action completes.
    Follow,
}

/// The operation performed by a cue's action phase.
#[derive(Debug, Clone, PartialEq)]
pub enum CueKind {
    /// An audio source to be played by a media adapter.
    Audio {
        /// Adapter-specific source URI or file path.
        source: String,
    },
    /// A video source to be played by a media adapter.
    Video {
        /// Adapter-specific source URI or file path.
        source: String,
    },
    /// A timing-only cue.
    Wait,
    /// Changes a running target cue's volume and/or opacity.
    Fade {
        /// Cue whose parameters will be interpolated.
        target: CueId,
        /// Desired final volume; `None` leaves volume unchanged.
        volume: Option<f32>,
        /// Desired final opacity; `None` leaves opacity unchanged.
        opacity: Option<f32>,
        /// Interpolation curve applied to both parameters.
        curve: crate::FadeCurve,
    },
    /// Starts child cues one at a time, waiting for each to complete.
    Group {
        /// Children in trigger order.
        children: Vec<CueId>,
    },
}

impl CueKind {
    /// Creates an audio cue kind with an adapter-specific source.
    pub fn audio(source: impl Into<String>) -> Self {
        Self::Audio {
            source: source.into(),
        }
    }

    /// Creates a video cue kind with an adapter-specific source.
    pub fn video(source: impl Into<String>) -> Self {
        Self::Video {
            source: source.into(),
        }
    }

    /// Returns `true` for audio and video cues.
    pub const fn is_media(&self) -> bool {
        matches!(self, Self::Audio { .. } | Self::Video { .. })
    }
}

/// Public state of a cue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CueState {
    /// Not running and available to start.
    #[default]
    Stopped,
    /// Waiting before the action starts.
    PreWait,
    /// Executing the action.
    Running,
    /// Waiting after the action completes.
    PostWait,
    /// Timing and fades are frozen; resume restores the previous phase.
    Paused,
    /// Finished naturally, including its post-wait.
    Completed,
    /// A validation, target, media, or scheduling error occurred.
    Error,
}

/// Immutable cue definition and timing configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    id: CueId,
    name: String,
    kind: CueKind,
    pre_wait_ms: u64,
    duration_ms: Option<u64>,
    post_wait_ms: u64,
    auto_continue: AutoContinue,
    volume: f32,
    opacity: f32,
}

impl Cue {
    /// Creates a cue with zero waits, no duration, and no auto-continuation.
    ///
    /// Wait and fade cues require a duration before they can start. Audio and
    /// video cues with no duration wait for an external completion notification.
    pub fn new(id: CueId, name: impl Into<String>, kind: CueKind) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            pre_wait_ms: 0,
            duration_ms: None,
            post_wait_ms: 0,
            auto_continue: AutoContinue::None,
            volume: 1.0,
            opacity: 1.0,
        }
    }

    /// Sets the pre-wait duration in milliseconds.
    pub const fn with_pre_wait_ms(mut self, pre_wait_ms: u64) -> Self {
        self.pre_wait_ms = pre_wait_ms;
        self
    }

    /// Sets a finite action duration in milliseconds.
    pub const fn with_duration_ms(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Sets or clears the action duration.
    ///
    /// Clearing a media cue's duration makes it depend on external completion.
    pub const fn with_duration(mut self, duration_ms: Option<u64>) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// Sets the post-wait duration in milliseconds.
    pub const fn with_post_wait_ms(mut self, post_wait_ms: u64) -> Self {
        self.post_wait_ms = post_wait_ms;
        self
    }

    /// Sets automatic continuation behavior.
    pub const fn with_auto_continue(mut self, auto_continue: AutoContinue) -> Self {
        self.auto_continue = auto_continue;
        self
    }

    /// Sets the normal running volume, clamped to 0.0 through 1.0.
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = sanitize_parameter(volume);
        self
    }

    /// Sets the normal running opacity, clamped to 0.0 through 1.0.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = sanitize_parameter(opacity);
        self
    }

    /// Returns the stable cue identifier.
    pub const fn id(&self) -> CueId {
        self.id
    }

    /// Returns the display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the action kind.
    pub const fn kind(&self) -> &CueKind {
        &self.kind
    }

    /// Returns the pre-wait duration in milliseconds.
    pub const fn pre_wait_ms(&self) -> u64 {
        self.pre_wait_ms
    }

    /// Returns the configured action duration, if finite.
    pub const fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// Returns the post-wait duration in milliseconds.
    pub const fn post_wait_ms(&self) -> u64 {
        self.post_wait_ms
    }

    /// Returns the automatic continuation mode.
    pub const fn auto_continue(&self) -> AutoContinue {
        self.auto_continue
    }

    /// Returns the normal running volume.
    pub const fn volume(&self) -> f32 {
        self.volume
    }

    /// Returns the normal running opacity.
    pub const fn opacity(&self) -> f32 {
        self.opacity
    }
}

fn sanitize_parameter(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

impl fmt::Display for CueState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
