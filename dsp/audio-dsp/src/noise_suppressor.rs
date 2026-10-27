use std::fmt;

/// Native RNNoise sample rate in Hz.
pub const RNNOISE_SAMPLE_RATE: u32 = 48_000;
/// Native RNNoise frame size: 10 ms at 48 kHz.
pub const RNNOISE_FRAME_SAMPLES: usize = 480;

/// Selectable noise-suppression backend.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoiseSuppressorBackend {
    /// No suppression; samples pass through unchanged.
    #[default]
    Passthrough,
    /// RNNoise backend.
    Rnnoise,
    /// Reserved for a future DeepFilterNet backend; not currently implemented.
    DeepFilterNet,
}

/// Errors reported by noise-suppressor input validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoiseSuppressorError {
    /// The interleaved channel count differs from the constructed backend.
    InvalidChannelCount {
        /// Channel count required by the backend.
        expected: usize,
        /// Channel count supplied to processing.
        actual: usize,
    },
    /// The processing sample rate differs from the constructed backend.
    InvalidSampleRate {
        /// Sample rate required by the backend.
        expected: u32,
        /// Sample rate supplied to processing.
        actual: u32,
    },
    /// The interleaved sample count is not exactly one backend frame.
    InvalidFrameLength {
        /// Required total sample count, including all channels.
        expected: usize,
        /// Supplied total sample count.
        actual: usize,
    },
    /// The backend cannot be constructed at this sample rate.
    UnsupportedSampleRate {
        /// Unsupported requested sample rate.
        sample_rate: u32,
    },
}

impl fmt::Display for NoiseSuppressorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChannelCount { expected, actual } => write!(
                formatter,
                "invalid channel count: expected {expected}, got {actual}"
            ),
            Self::InvalidSampleRate { expected, actual } => write!(
                formatter,
                "invalid sample rate: expected {expected}, got {actual}"
            ),
            Self::InvalidFrameLength { expected, actual } => write!(
                formatter,
                "invalid frame length: expected {expected} samples, got {actual}"
            ),
            Self::UnsupportedSampleRate { sample_rate } => write!(
                formatter,
                "RNNoise requires {RNNOISE_SAMPLE_RATE} Hz, got {sample_rate}"
            ),
        }
    }
}

impl std::error::Error for NoiseSuppressorError {}

/// Audio-callback interface for pluggable noise suppression.
///
/// Implementations must not allocate in [`process`](Self::process). Buffers are
/// interleaved `f32` frames.
pub trait NoiseSuppressor {
    /// Processes one buffer in place.
    fn process(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        sample_rate: u32,
    ) -> Result<(), NoiseSuppressorError>;
}

/// Default no-op noise suppressor.
#[derive(Debug, Default, Clone, Copy)]
pub struct PassthroughNs;

impl NoiseSuppressor for PassthroughNs {
    fn process(
        &mut self,
        _buffer: &mut [f32],
        _channels: usize,
        _sample_rate: u32,
    ) -> Result<(), NoiseSuppressorError> {
        Ok(())
    }
}

/// Validating RNNoise plugin-slot placeholder.
///
/// Construction fixes channel count and the native 48 kHz rate. Processing
/// accepts exactly one 10 ms frame (480 samples per channel). Until the RNNoise
/// C library is vendored behind a build opt-in, valid frames pass through
/// unchanged; no suppression is claimed. A future vendor adapter can replace
/// only this implementation while HAL code retains the same trait object.
pub struct RnnoiseNs {
    channels: usize,
    sample_rate: u32,
}

impl RnnoiseNs {
    /// Creates an RNNoise slot for a fixed channel count and sample rate.
    ///
    /// Currently only [`RNNOISE_SAMPLE_RATE`] is accepted; resampling belongs to
    /// the audio engine outside this slot.
    pub fn new(channels: usize, sample_rate: u32) -> Result<Self, NoiseSuppressorError> {
        if channels == 0 {
            return Err(NoiseSuppressorError::InvalidChannelCount {
                expected: 1,
                actual: 0,
            });
        }
        if sample_rate != RNNOISE_SAMPLE_RATE {
            return Err(NoiseSuppressorError::UnsupportedSampleRate { sample_rate });
        }
        Ok(Self {
            channels,
            sample_rate,
        })
    }

    /// Returns the required number of samples per channel: 480 at 48 kHz.
    pub fn frame_samples(&self) -> usize {
        RNNOISE_FRAME_SAMPLES
    }

    /// Returns the frame duration in milliseconds.
    pub fn frame_duration_ms(&self) -> u32 {
        10
    }

    /// Returns this slot's backend identifier.
    pub fn backend(&self) -> NoiseSuppressorBackend {
        NoiseSuppressorBackend::Rnnoise
    }
}

impl NoiseSuppressor for RnnoiseNs {
    fn process(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        sample_rate: u32,
    ) -> Result<(), NoiseSuppressorError> {
        if channels != self.channels {
            return Err(NoiseSuppressorError::InvalidChannelCount {
                expected: self.channels,
                actual: channels,
            });
        }
        if sample_rate != self.sample_rate {
            return Err(NoiseSuppressorError::InvalidSampleRate {
                expected: self.sample_rate,
                actual: sample_rate,
            });
        }
        let expected = RNNOISE_FRAME_SAMPLES * self.channels;
        if buffer.len() != expected {
            return Err(NoiseSuppressorError::InvalidFrameLength {
                expected,
                actual: buffer.len(),
            });
        }

        // The future vendored RNNoise adapter processes each channel's 480-sample
        // native frame here. Keeping the slot explicit avoids a hidden C build
        // dependency and makes the current pass-through behavior intentional.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NoiseSuppressor, NoiseSuppressorBackend, NoiseSuppressorError, PassthroughNs, RnnoiseNs,
        RNNOISE_FRAME_SAMPLES, RNNOISE_SAMPLE_RATE,
    };

    #[test]
    fn default_slot_passes_samples_through() {
        let mut suppressor = PassthroughNs;
        let mut buffer = [0.25, -0.5, 0.75, -1.0];
        let original = buffer;
        suppressor.process(&mut buffer, 2, 48_000).unwrap();
        assert_eq!(buffer, original);
    }

    #[test]
    fn rnnoise_slot_accepts_exact_ten_millisecond_frame() {
        let mut suppressor = RnnoiseNs::new(2, RNNOISE_SAMPLE_RATE).unwrap();
        assert_eq!(suppressor.frame_samples(), 480);
        assert_eq!(suppressor.frame_duration_ms(), 10);
        assert_eq!(suppressor.backend(), NoiseSuppressorBackend::Rnnoise);
        let mut buffer = vec![0.25; RNNOISE_FRAME_SAMPLES * 2];
        suppressor
            .process(&mut buffer, 2, RNNOISE_SAMPLE_RATE)
            .unwrap();
        assert!(buffer.iter().all(|sample| *sample == 0.25));
    }

    #[test]
    fn rnnoise_slot_rejects_wrong_frame_length() {
        let mut suppressor = RnnoiseNs::new(1, RNNOISE_SAMPLE_RATE).unwrap();
        let mut buffer = [0.0; 479];
        assert_eq!(
            suppressor.process(&mut buffer, 1, RNNOISE_SAMPLE_RATE),
            Err(NoiseSuppressorError::InvalidFrameLength {
                expected: 480,
                actual: 479,
            })
        );
    }

    #[test]
    fn rnnoise_slot_rejects_mismatched_stream_format() {
        let mut suppressor = RnnoiseNs::new(2, RNNOISE_SAMPLE_RATE).unwrap();
        let mut buffer = vec![0.0; RNNOISE_FRAME_SAMPLES * 2];
        assert!(matches!(
            suppressor.process(&mut buffer, 1, RNNOISE_SAMPLE_RATE),
            Err(NoiseSuppressorError::InvalidChannelCount { .. })
        ));
        assert!(matches!(
            suppressor.process(&mut buffer, 2, 44_100),
            Err(NoiseSuppressorError::InvalidSampleRate { .. })
        ));
        assert!(matches!(
            RnnoiseNs::new(1, 44_100),
            Err(NoiseSuppressorError::UnsupportedSampleRate { .. })
        ));
    }

    #[test]
    fn deep_filter_net_backend_is_reserved() {
        assert_ne!(
            NoiseSuppressorBackend::DeepFilterNet,
            NoiseSuppressorBackend::Passthrough
        );
    }
}
