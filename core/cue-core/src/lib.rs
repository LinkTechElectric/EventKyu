//! EventKyu's deterministic, media-agnostic cue scheduling engine.
//!
//! The crate owns cue timing, continuation chains, fades, list/cart triggering,
//! and state events. It deliberately performs no audio or video I/O: media
//! adapters consume [`EngineEvent`] and report completion through
//! [`Engine::notify_media_completed`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod clock;
mod cue;
mod curve;
mod engine;
mod error;
mod event;

#[cfg(test)]
mod tests;

pub use clock::{Clock, ManualClock, SystemClock};
pub use cue::{AutoContinue, Cue, CueId, CueKind, CueState};
pub use curve::FadeCurve;
pub use engine::{CueStatus, Engine};
pub use error::CueError;
pub use event::{EngineEvent, MediaAction};
