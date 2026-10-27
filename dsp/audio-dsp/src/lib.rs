//! Real-time-safe audio DSP primitives for EventKyu's Core Audio and WASAPI
//! halves.
//!
//! Buffers use interleaved `f32` samples. All audio-processing methods perform
//! no heap allocation; storage is allocated by constructors. The crate performs
//! no device I/O and contains no platform-specific code.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod fade;
mod gain;
mod limiter;
mod meter;
mod mixer;
mod noise_suppressor;
mod ring_buffer;

pub use fade::{apply_fade, fade_gains, FadeCurve, FadeDirection};
pub use gain::{apply_gain, db_to_linear, linear_to_db, SmoothedGain, SILENCE_DB};
pub use limiter::{Limiter, LimiterConfig};
pub use meter::{LevelMeter, LevelSnapshot};
pub use mixer::mix;
pub use noise_suppressor::{
    NoiseSuppressor, NoiseSuppressorBackend, NoiseSuppressorError, PassthroughNs, RnnoiseNs,
    RNNOISE_FRAME_SAMPLES, RNNOISE_SAMPLE_RATE,
};
pub use ring_buffer::InterleavedRingBuffer;

/// Default sample rate used by EventKyu's audio engine.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
