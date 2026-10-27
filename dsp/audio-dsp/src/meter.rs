use crate::gain::linear_to_db;

/// Peak and RMS readings for one channel or an aggregate of channels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelSnapshot {
    /// Peak amplitude in dBFS.
    pub peak_dbfs: f32,
    /// Root-mean-square amplitude in dBFS.
    pub rms_dbfs: f32,
}

/// Per-channel peak-hold and sliding-window RMS meter.
///
/// Peaks hold for a configured number of frames and then fall at a configured
/// dB-per-second rate. RMS uses one preallocated ring of squared samples per
/// channel, so [`process`](Self::process) does not allocate.
pub struct LevelMeter {
    channels: usize,
    window_frames: usize,
    write_frame: usize,
    rms_squares: Vec<f32>,
    rms_sums: Vec<f32>,
    peaks: Vec<f32>,
    hold_remaining: Vec<usize>,
    hold_frames: usize,
    release_coefficient: f32,
}

impl LevelMeter {
    /// Creates a meter with a 300 ms RMS window, 1 s peak hold, and 20 dB/s
    /// peak release.
    pub fn new(channels: usize, sample_rate: u32) -> Self {
        Self::with_timing(channels, sample_rate, 300.0, 1_000.0, 20.0)
    }

    /// Creates a meter with explicit timing parameters.
    ///
    /// A zero peak hold or release time disables that stage. All storage is
    /// allocated here rather than in the audio callback.
    ///
    /// # Panics
    ///
    /// Panics on zero channels or sample rate, invalid timing values, or a
    /// sub-millisecond RMS window that rounds to zero frames.
    pub fn with_timing(
        channels: usize,
        sample_rate: u32,
        rms_window_ms: f32,
        peak_hold_ms: f32,
        peak_release_db_per_second: f32,
    ) -> Self {
        assert!(channels > 0, "channels must be non-zero");
        assert!(sample_rate > 0, "sample_rate must be non-zero");
        assert!(
            rms_window_ms.is_finite() && rms_window_ms > 0.0,
            "rms_window_ms must be finite and positive"
        );
        assert!(
            peak_hold_ms.is_finite() && peak_hold_ms >= 0.0,
            "peak_hold_ms must be finite and non-negative"
        );
        assert!(
            peak_release_db_per_second.is_finite() && peak_release_db_per_second >= 0.0,
            "peak release must be finite and non-negative"
        );

        let window_frames = milliseconds_to_frames(sample_rate, rms_window_ms);
        assert!(window_frames > 0, "RMS window must contain a frame");
        let hold_frames = milliseconds_to_frames(sample_rate, peak_hold_ms);
        let release_coefficient = if peak_release_db_per_second == 0.0 {
            0.0
        } else {
            10.0_f32.powf(-peak_release_db_per_second / (20.0 * sample_rate as f32))
        };

        Self {
            channels,
            window_frames,
            write_frame: 0,
            rms_squares: vec![0.0; channels * window_frames],
            rms_sums: vec![0.0; channels],
            peaks: vec![0.0; channels],
            hold_remaining: vec![0; channels],
            hold_frames,
            release_coefficient,
        }
    }

    /// Consumes an interleaved buffer and updates all channel measurements.
    ///
    /// # Panics
    ///
    /// Panics if the buffer does not contain a whole number of frames.
    pub fn process(&mut self, buffer: &[f32]) {
        assert_eq!(
            buffer.len() % self.channels,
            0,
            "buffer must contain whole frames"
        );

        for frame in buffer.chunks_exact(self.channels) {
            for (channel, sample) in frame.iter().enumerate() {
                let magnitude = sample.abs();
                let square = sample * sample;
                let slot = channel * self.window_frames + self.write_frame;
                self.rms_sums[channel] += square - self.rms_squares[slot];
                self.rms_squares[slot] = square;

                if magnitude >= self.peaks[channel] {
                    self.peaks[channel] = magnitude;
                    self.hold_remaining[channel] = self.hold_frames;
                } else if self.hold_remaining[channel] > 0 {
                    self.hold_remaining[channel] -= 1;
                } else {
                    self.peaks[channel] =
                        (self.peaks[channel] * self.release_coefficient).max(magnitude);
                }
            }
            self.write_frame = (self.write_frame + 1) % self.window_frames;
        }
    }

    /// Returns the current peak and RMS readings for one channel.
    pub fn channel_levels(&self, channel: usize) -> LevelSnapshot {
        assert!(channel < self.channels, "channel out of range");
        let rms = (self.rms_sums[channel].max(0.0) / self.window_frames as f32).sqrt();
        LevelSnapshot {
            peak_dbfs: linear_to_db(self.peaks[channel]),
            rms_dbfs: linear_to_db(rms),
        }
    }

    /// Returns aggregate levels: maximum channel peak and mean-power channel RMS.
    pub fn summary(&self) -> LevelSnapshot {
        let mut peak = 0.0_f32;
        let mut mean_power = 0.0_f32;
        for channel in 0..self.channels {
            peak = peak.max(self.peaks[channel]);
            mean_power += self.rms_sums[channel].max(0.0) / self.window_frames as f32;
        }
        mean_power /= self.channels as f32;
        LevelSnapshot {
            peak_dbfs: linear_to_db(peak),
            rms_dbfs: linear_to_db(mean_power.sqrt()),
        }
    }

    /// Clears all measurements and the sliding RMS history without allocating.
    pub fn reset(&mut self) {
        self.write_frame = 0;
        self.rms_squares.fill(0.0);
        self.rms_sums.fill(0.0);
        self.peaks.fill(0.0);
        self.hold_remaining.fill(0);
    }
}

fn milliseconds_to_frames(sample_rate: u32, milliseconds: f32) -> usize {
    (sample_rate as f32 * milliseconds / 1_000.0).round() as usize
}

#[cfg(test)]
mod tests {
    use super::LevelMeter;

    fn close(actual: f32, expected: f32, tolerance: f32) {
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn sine_wave_has_known_peak_and_rms() {
        let sample_rate = 1_000;
        let mut meter = LevelMeter::new(1, sample_rate);
        let buffer: Vec<f32> = (0..300)
            .map(|frame| (std::f32::consts::TAU * 5.0 * frame as f32 / 1_000.0).sin())
            .collect();
        meter.process(&buffer);

        let levels = meter.channel_levels(0);
        close(levels.peak_dbfs, 0.0, 1e-4);
        close(levels.rms_dbfs, -3.010_3, 1e-3);
    }

    #[test]
    fn dc_channels_have_known_rms_and_aggregate() {
        let mut meter = LevelMeter::new(2, 1_000);
        let buffer: Vec<f32> = (0..300).flat_map(|_| [0.5, 0.25]).collect();
        meter.process(&buffer);

        close(meter.channel_levels(0).rms_dbfs, -6.020_6, 1e-4);
        close(meter.channel_levels(1).rms_dbfs, -12.041_2, 1e-4);
        let summary = meter.summary();
        close(summary.peak_dbfs, -6.020_6, 1e-4);
        close(summary.rms_dbfs, 20.0 * 0.395_284_7_f32.log10(), 1e-4);
    }

    #[test]
    fn peak_holds_then_releases_slowly() {
        let mut meter = LevelMeter::with_timing(1, 1_000, 300.0, 10.0, 20.0);
        meter.process(&[1.0]);
        meter.process(&[0.0; 10]);
        close(meter.channel_levels(0).peak_dbfs, 0.0, 1e-6);

        meter.process(&[0.0; 1_000]);
        close(meter.channel_levels(0).peak_dbfs, -20.0, 0.01);
    }

    #[test]
    fn rms_window_forgets_old_signal() {
        let mut meter = LevelMeter::with_timing(1, 100, 10.0, 0.0, 0.0);
        meter.process(&[1.0]);
        meter.process(&[0.0]);
        assert_eq!(meter.channel_levels(0).rms_dbfs, f32::NEG_INFINITY);
    }
}
