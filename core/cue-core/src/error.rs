use crate::{CueId, CueState};
use std::error::Error;
use std::fmt;

/// Errors returned by cue operations and scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CueError {
    /// The referenced cue does not exist.
    UnknownCue(CueId),
    /// A cue with the same identifier has already been added.
    DuplicateCue(CueId),
    /// No cue is selected by the list playhead.
    NoCueAtPlayhead,
    /// A playhead index is outside the cue list.
    PlayheadOutOfBounds(usize),
    /// The requested cart slot has no binding.
    UnboundCartSlot(u32),
    /// The cart slot is not bound to an existing cue.
    UnknownCartCue(u32),
    /// The operation is incompatible with the cue's current state.
    InvalidState {
        /// Affected cue.
        cue_id: CueId,
        /// Current state.
        state: CueState,
    },
    /// This cue kind requires a finite action duration.
    DurationRequired(CueId),
    /// The external completion event is only valid for a running media cue.
    InvalidMediaCompletion(CueId),
    /// A fade target is not currently running.
    TargetNotRunning(CueId),
    /// A group contains itself or another cyclic trigger relationship.
    GroupCycle(CueId),
    /// A manually supplied timestamp predates the engine's current time.
    NonMonotonicClock {
        /// Previously observed time.
        previous_ms: u64,
        /// Rejected time.
        received_ms: u64,
    },
}

impl fmt::Display for CueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCue(id) => write!(formatter, "unknown cue {id}"),
            Self::DuplicateCue(id) => write!(formatter, "duplicate cue {id}"),
            Self::NoCueAtPlayhead => formatter.write_str("no cue at the playhead"),
            Self::PlayheadOutOfBounds(index) => {
                write!(formatter, "playhead index {index} is out of bounds")
            }
            Self::UnboundCartSlot(slot) => write!(formatter, "cart slot {slot} is unbound"),
            Self::UnknownCartCue(slot) => {
                write!(formatter, "cart slot {slot} references an unknown cue")
            }
            Self::InvalidState { cue_id, state } => {
                write!(
                    formatter,
                    "cue {cue_id} cannot perform this action while {state}"
                )
            }
            Self::DurationRequired(id) => write!(formatter, "cue {id} requires a duration"),
            Self::InvalidMediaCompletion(id) => {
                write!(formatter, "cue {id} cannot accept media completion now")
            }
            Self::TargetNotRunning(id) => write!(formatter, "fade target cue {id} is not running"),
            Self::GroupCycle(id) => write!(formatter, "group cycle detected at cue {id}"),
            Self::NonMonotonicClock {
                previous_ms,
                received_ms,
            } => write!(
                formatter,
                "clock moved backwards from {previous_ms} ms to {received_ms} ms"
            ),
        }
    }
}

impl Error for CueError {}
