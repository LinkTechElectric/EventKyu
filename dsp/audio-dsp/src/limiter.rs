/// Configuration for [`Limiter`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimiterConfig {
    /// Audio lookahead delay in milliseconds.
    pub lookahead_ms: f32,
    /// Full-scale linear gain-reduction ramp time in milliseconds.
    pub attack_ms: f32,
    /// Full-scale linear gain-recovery ramp time in milliseconds.
    pub release_ms: f32,
    /// Maximum permitted output sample amplitude in dBFS.
    pub ceiling_db: f32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            lookahead_ms: 5.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            ceiling_db: -1.0,
        }
    }
}

/// Lookahead master-bus peak limiter with linked channel gain.
///
/// The detector window covers the delayed frame and its lookahead future. Gain
/// reduction therefore starts before a transient reaches the output. Attack and
/// release are linear full-scale ramps; a per-frame safety gain bounds the peak
/// even when attack is longer than lookahead. All delay and detector storage is
/// preallocated.
pub struct Limiter {
    channels: usize,
    lookahead_frames: usize,
    delay: Vec<f32>,
    delayed_frame: Vec<f32>,
    peak_history: Vec<f32>,
    delay_write: usize,
    history_write: usize,
    attack_step: f32,
    release_step: f32,
    ceiling: f32,
    envelope: f32,
}

impl Limiter {
    /// Creates a limiter with 5 ms lookahead and attack, 100 ms release, and a
    /// -1 dBFS ceiling.
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        Self::with_config(channels, sample_rate, LimiterConfig::default())
    }

    /// Creates a limiter with explicit configuration.
    ///
    /// # Panics
    ///
    /// Panics on zero channels or sample rate, invalid timing values, or a
    /// non-finite ceiling.
    pub fn with_config(channels: usize, sample_rate: u32, config: LimiterConfig) -> Self {
        assert!(channels > 0, "channels must be non-zero");
        assert!(sample_rate > 0, "sample_rate must be non-zero");
        assert!(
            config.lookahead_ms.is_finite() && config.lookahead_ms >= 0.0,
            "lookahead_ms must be finite and non-negative"
        );
        assert!(
            config.attack_ms.is_finite() && config.attack_ms >= 0.0,
            "attack_ms must be finite and non-negative"
        );
        assert!(
            config.release_ms.is_finite() && config.release_ms >= 0.0,
            "release_ms must be finite and non-negative"
        );
        assert!(config.ceiling_db.is_finite(), "ceiling_db must be finite");

        let lookahead_frames = milliseconds_to_frames(sample_rate, config.lookahead_ms);
        let attack_frames = milliseconds_to_frames(sample_rate, config.attack_ms);
        let release_frames = milliseconds_to_frames(sample_rate, config.release_ms);
        let attack_step = ramp_step(attack_frames);
        let release_step = ramp_step(release_frames);

        Self {
            channels,
            lookahead_frames,
            delay: vec![0.0; channels * lookahead_frames],
            delayed_frame: vec![0.0; channels],
            peak_history: vec![0.0; lookahead_frames + 1],
            delay_write: 0,
            history_write: 0,
            attack_step,
            release_step,
            ceiling: 10.0_f32.powf(config.ceiling_db / 20.0),
            envelope: 1.0,
        }
    }

    /// Returns the introduced latency in frames.
    pub fn latency_frames(&self) -> usize {
        self.lookahead_frames
    }

    /// Returns the current linear gain envelope before the safety bound.
    pub fn envelope(&self) -> f32 {
        self.envelope
    }

    /// Processes an interleaved buffer in place and delays it by the lookahead.
    ///
    /// A single gain is applied to all channels in each frame to preserve the
    /// stereo or multichannel image.
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
            let mut input_peak = 0.0_f32;
            let mut delayed_peak = 0.0_f32;

            for (channel, sample) in frame.iter_mut().enumerate() {
                let input = *sample;
                input_peak = input_peak.max(input.abs());
                self.delayed_frame[channel] = if self.lookahead_frames == 0 {
                    input
                } else {
                    let slot = channel * self.lookahead_frames + self.delay_write;
                    let delayed = self.delay[slot];
                    self.delay[slot] = input;
                    delayed
                };
                delayed_peak = delayed_peak.max(self.delayed_frame[channel].abs());
            }

            self.peak_history[self.history_write] = input_peak;
            self.history_write = (self.history_write + 1) % self.peak_history.len();
            let window_peak = self
                .peak_history
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(*sample));
            let target = required_gain(window_peak, self.ceiling);
            self.update_envelope(target);

            let gain = self.envelope.min(required_gain(delayed_peak, self.ceiling));
            for (sample, delayed) in frame.iter_mut().zip(&self.delayed_frame) {
                *sample = *delayed * gain;
            }

            if self.lookahead_frames > 0 {
                self.delay_write = (self.delay_write + 1) % self.lookahead_frames;
            }
        }
    }

    /// Clears delayed audio and detector history and restores unity gain.
    pub fn reset(&mut self) {
        self.delay.fill(0.0);
        self.delayed_frame.fill(0.0);
        self.peak_history.fill(0.0);
        self.delay_write = 0;
        self.history_write = 0;
        self.envelope = 1.0;
    }

    fn update_envelope(&mut self, target: f32) {
        if target < self.envelope {
            self.envelope = (self.envelope - self.attack_step).max(target);
        } else {
            self.envelope = (self.envelope + self.release_step).min(target);
        }
    }
}

fn required_gain(peak: f32, ceiling: f32) -> f32 {
    if peak > ceiling {
        ceiling / peak
    } else {
        1.0
    }
}

fn ramp_step(frames: usize) -> f32 {
    if frames == 0 {
        1.0
    } else {
        1.0 / frames as f32
    }
}

fn milliseconds_to_frames(sample_rate: u32, milliseconds: f32) -> usize {
    (sample_rate as f32 * milliseconds / 1_000.0).round() as usize
}

#[cfg(test)]
mod tests {
    use super::{Limiter, LimiterConfig};

    #[test]
    fn output_never_exceeds_ceiling_during_abrupt_transients() {
        let ceiling_db = -6.0;
        let ceiling = 10.0_f32.powf(ceiling_db / 20.0);
        let mut limiter = Limiter::with_config(
            2,
            1_000,
            LimiterConfig {
                ceiling_db,
                ..LimiterConfig::default()
            },
        );
        let mut buffer: Vec<f32> = (0..200)
            .flat_map(|frame| {
                if (50..55).contains(&frame) {
                    [2.0, -1.5]
                } else {
                    [0.1, -0.1]
                }
            })
            .collect();
        limiter.process(&mut buffer);

        let peak = buffer
            .iter()
            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak <= ceiling + 1e-6, "peak {peak} exceeded {ceiling}");
    }

    #[test]
    fn small_signals_pass_transparently_after_lookahead() {
        let mut limiter = Limiter::new(1, 48_000);
        let input: Vec<f32> = (0..480)
            .map(|frame| 0.1 * (std::f32::consts::TAU * frame as f32 / 100.0).sin())
            .collect();
        let mut output = input.clone();
        limiter.process(&mut output);

        let latency = limiter.latency_frames();
        assert!(output[..latency].iter().all(|sample| *sample == 0.0));
        for (actual, expected) in output[latency..]
            .iter()
            .zip(&input[..input.len() - latency])
        {
            assert_eq!(actual, expected);
        }
        assert_eq!(limiter.envelope(), 1.0);
    }

    #[test]
    fn safety_bound_holds_even_without_lookahead_or_attack_time() {
        let mut limiter = Limiter::with_config(
            1,
            48_000,
            LimiterConfig {
                lookahead_ms: 0.0,
                attack_ms: 100.0,
                release_ms: 100.0,
                ceiling_db: -12.0,
            },
        );
        let ceiling = 10.0_f32.powf(-12.0 / 20.0);
        let mut buffer = [0.0, 4.0, -4.0, 0.1];
        limiter.process(&mut buffer);

        assert!(buffer.iter().all(|sample| sample.abs() <= ceiling + 1e-6));
        assert!(buffer[1].abs() <= ceiling + 1e-6);
    }
}
