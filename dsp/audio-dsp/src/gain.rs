/// Gain values at or below this dB threshold are treated as mute.
pub const SILENCE_DB: f32 = -60.0;

/// Converts a dB gain value to a linear amplitude multiplier.
///
/// Values at or below [`SILENCE_DB`] map to exact zero. NaN also maps to zero;
/// positive infinity remains an infinite gain.
pub fn db_to_linear(db: f32) -> f32 {
    if db.is_nan() || db <= SILENCE_DB {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

/// Converts a linear amplitude multiplier to dB.
///
/// The magnitude is used, zero and NaN map to negative infinity, and finite
/// non-zero values are not clamped to the mute threshold. This preserves useful
/// low-level readings for meters.
pub fn linear_to_db(linear: f32) -> f32 {
    let magnitude = linear.abs();
    if magnitude == 0.0 || magnitude.is_nan() {
        f32::NEG_INFINITY
    } else {
        20.0 * magnitude.log10()
    }
}

/// Applies one fixed linear gain per channel to an interleaved buffer.
///
/// # Panics
///
/// Panics if `channels` is zero, `gains` does not contain one value per channel,
/// or the buffer does not contain a whole number of frames.
pub fn apply_gain(buffer: &mut [f32], channels: usize, gains: &[f32]) {
    assert!(channels > 0, "channels must be non-zero");
    assert_eq!(gains.len(), channels, "one gain is required per channel");
    assert_eq!(
        buffer.len() % channels,
        0,
        "buffer must contain whole frames"
    );

    for frame in buffer.chunks_exact_mut(channels) {
        for (sample, gain) in frame.iter_mut().zip(gains) {
            *sample *= gain;
        }
    }
}

/// Per-channel one-pole smoothed linear gain.
///
/// Each sample moves the current gain toward its target by a coefficient derived
/// from the configured time constant. After one time constant, approximately
/// 63.2% of a step has been traversed. The target vectors are preallocated.
pub struct SmoothedGain {
    channels: usize,
    coefficient: f32,
    current: Vec<f32>,
    target: Vec<f32>,
}

impl SmoothedGain {
    /// Creates a smoother with unity current and target gains on every channel.
    ///
    /// A zero `time_constant_ms` makes gain changes immediate.
    ///
    /// # Panics
    ///
    /// Panics on zero channels, a zero sample rate, or a negative, NaN, or
    /// infinite time constant.
    pub fn new(channels: usize, sample_rate: u32, time_constant_ms: f32) -> Self {
        assert!(channels > 0, "channels must be non-zero");
        assert!(sample_rate > 0, "sample_rate must be non-zero");
        assert!(
            time_constant_ms.is_finite() && time_constant_ms >= 0.0,
            "time_constant_ms must be finite and non-negative"
        );

        let coefficient = if time_constant_ms == 0.0 {
            1.0
        } else {
            let time_constant_samples = sample_rate as f32 * time_constant_ms / 1_000.0;
            1.0 - (-1.0 / time_constant_samples).exp()
        };

        Self {
            channels,
            coefficient,
            current: vec![1.0; channels],
            target: vec![1.0; channels],
        }
    }

    /// Sets one channel's target from a linear amplitude multiplier.
    ///
    /// # Panics
    ///
    /// Panics if the channel is out of range or the gain is negative, NaN, or
    /// infinite.
    pub fn set_target_linear(&mut self, channel: usize, gain: f32) {
        assert!(channel < self.channels, "channel out of range");
        assert!(
            gain.is_finite() && gain >= 0.0,
            "gain must be finite and non-negative"
        );
        self.target[channel] = gain;
    }

    /// Sets one channel's target from a dB value using [`db_to_linear`].
    ///
    /// # Panics
    ///
    /// Panics if the channel is out of range.
    pub fn set_target_db(&mut self, channel: usize, db: f32) {
        self.set_target_linear(channel, db_to_linear(db));
    }

    /// Sets every channel to the same linear target gain.
    pub fn set_all_targets_linear(&mut self, gain: f32) {
        assert!(
            gain.is_finite() && gain >= 0.0,
            "gain must be finite and non-negative"
        );
        self.target.fill(gain);
    }

    /// Returns the current linear gain for a channel.
    pub fn current_linear(&self, channel: usize) -> f32 {
        self.current[channel]
    }

    /// Returns the target linear gain for a channel.
    pub fn target_linear(&self, channel: usize) -> f32 {
        self.target[channel]
    }

    /// Immediately snaps all current gains to their targets without processing.
    pub fn reset_to_targets(&mut self) {
        self.current.copy_from_slice(&self.target);
    }

    /// Applies smoothed gains to an interleaved buffer in place.
    ///
    /// # Panics
    ///
    /// Panics if the buffer does not contain a whole number of frames.
    pub fn process(&mut self, buffer: &mut [f32]) {
        assert_eq!(
            buffer.len() % self.channels,
            0,
            "buffer must contain whole frames"
        );

        for frame in buffer.chunks_exact_mut(self.channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let current = self.current[channel]
                    + (self.target[channel] - self.current[channel]) * self.coefficient;
                self.current[channel] = current;
                *sample *= current;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_gain, db_to_linear, linear_to_db, SmoothedGain, SILENCE_DB};

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }

    #[test]
    fn db_conversions_handle_unity_and_half_amplitude() {
        close(db_to_linear(0.0), 1.0);
        close(linear_to_db(1.0), 0.0);
        close(db_to_linear(-6.020_6), 0.5);
        close(linear_to_db(0.5), -6.020_6);
    }

    #[test]
    fn db_conversion_enforces_silence_threshold() {
        assert_eq!(db_to_linear(SILENCE_DB), 0.0);
        assert_eq!(db_to_linear(-120.0), 0.0);
        assert!(db_to_linear(-59.0) > 0.0);
        assert_eq!(linear_to_db(0.0), f32::NEG_INFINITY);
        close(linear_to_db(0.001), SILENCE_DB);
    }

    #[test]
    fn smoothed_gain_approaches_target_with_configured_time_constant() {
        let mut gain = SmoothedGain::new(1, 1_000, 20.0);
        gain.set_target_linear(0, 0.0);
        let mut buffer = vec![1.0; 20];
        gain.process(&mut buffer);

        close(gain.current_linear(0), (-1.0_f32).exp());
        assert!(buffer.windows(2).all(|pair| pair[1] < pair[0]));
    }

    #[test]
    fn smoothed_gain_tracks_channels_independently() {
        let mut gain = SmoothedGain::new(2, 48_000, 0.0);
        gain.set_target_linear(0, 0.0);
        gain.set_target_linear(1, 0.5);
        let mut buffer = [1.0, 1.0, 1.0, 1.0];
        gain.process(&mut buffer);

        assert_eq!(buffer, [0.0, 0.5, 0.0, 0.5]);
    }

    #[test]
    fn fixed_gain_is_applied_per_channel() {
        let mut buffer = [1.0, -1.0, 0.5, -0.5];
        apply_gain(&mut buffer, 2, &[0.5, 2.0]);
        assert_eq!(buffer, [0.5, -2.0, 0.25, -1.0]);
    }
}
