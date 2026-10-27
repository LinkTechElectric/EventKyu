/// Interpolation curve shared semantically with EventKyu's cue engine.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FadeCurve {
    /// Constant-rate interpolation.
    #[default]
    Linear,
    /// Sinusoidal amplitude ramp, giving equal-power audio crossfades.
    EqualPower,
    /// Cubic smoothstep with zero slope at both ends.
    SCurve,
}

impl FadeCurve {
    /// Evaluates the fade-in curve at `progress` in the range 0.0 to 1.0.
    ///
    /// Values outside the range are clamped; NaN is treated as zero.
    pub fn evaluate(self, progress: f32) -> f32 {
        if progress.is_nan() {
            return 0.0;
        }
        let progress = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => progress,
            Self::EqualPower => (progress * std::f32::consts::FRAC_PI_2).sin(),
            Self::SCurve => progress * progress * (3.0 - 2.0 * progress),
        }
    }
}

/// Direction in which a gain ramp is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FadeDirection {
    /// Ramp from silence to full gain.
    #[default]
    FadeIn,
    /// Ramp from full gain to silence using the reversed curve.
    FadeOut,
}

/// Generates one gain per frame for a complete fade.
///
/// The first and last gains are exactly 0 and 1 for a fade-in. An empty request
/// returns an empty vector; a one-frame fade returns `[1.0]`, representing an
/// already completed transition. This utility allocates and is intended for
/// setup or offline use rather than an audio callback.
pub fn fade_gains(frames: usize, curve: FadeCurve) -> Vec<f32> {
    let mut gains = vec![0.0; frames];
    for (frame, gain) in gains.iter_mut().enumerate() {
        *gain = gain_at(frame, frames, curve, FadeDirection::FadeIn);
    }
    gains
}

/// Applies a fade to an interleaved buffer without allocating.
///
/// One gain is applied to every channel in each frame. Fade-out evaluates the
/// reversed curve, making equal-power fade-out cosine-shaped, consistent with
/// the cue engine's decreasing-parameter interpolation.
///
/// # Panics
///
/// Panics if `channels` is zero or the buffer does not contain whole frames.
pub fn apply_fade(buffer: &mut [f32], channels: usize, curve: FadeCurve, direction: FadeDirection) {
    assert!(channels > 0, "channels must be non-zero");
    assert_eq!(
        buffer.len() % channels,
        0,
        "buffer must contain whole frames"
    );
    let frames = buffer.len() / channels;

    for (index, frame) in buffer.chunks_exact_mut(channels).enumerate() {
        let gain = gain_at(index, frames, curve, direction);
        for sample in frame {
            *sample *= gain;
        }
    }
}

fn gain_at(frame: usize, frames: usize, curve: FadeCurve, direction: FadeDirection) -> f32 {
    if frames <= 1 {
        return 1.0;
    }
    let progress = frame as f32 / (frames - 1) as f32;
    match direction {
        FadeDirection::FadeIn => curve.evaluate(progress),
        FadeDirection::FadeOut => curve.evaluate(1.0 - progress),
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_fade, fade_gains, FadeCurve, FadeDirection};

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn linear_fade_has_half_gain_at_midpoint() {
        close(fade_gains(3, FadeCurve::Linear)[1], 0.5);
    }

    #[test]
    fn equal_power_fade_has_sqrt_half_gain_at_midpoint() {
        close(fade_gains(3, FadeCurve::EqualPower)[1], 0.5_f32.sqrt());
    }

    #[test]
    fn s_curve_fade_has_half_gain_at_midpoint() {
        close(fade_gains(3, FadeCurve::SCurve)[1], 0.5);
    }

    #[test]
    fn generated_fades_cover_exact_endpoints() {
        for curve in [FadeCurve::Linear, FadeCurve::EqualPower, FadeCurve::SCurve] {
            let gains = fade_gains(5, curve);
            assert_eq!(gains.first(), Some(&0.0));
            assert_eq!(gains.last(), Some(&1.0));
        }
        assert!(fade_gains(0, FadeCurve::Linear).is_empty());
        assert_eq!(fade_gains(1, FadeCurve::Linear), [1.0]);
    }

    #[test]
    fn fade_application_is_per_frame_and_supports_both_directions() {
        let mut fade_in = [1.0; 6];
        apply_fade(&mut fade_in, 2, FadeCurve::Linear, FadeDirection::FadeIn);
        assert_eq!(fade_in, [0.0, 0.0, 0.5, 0.5, 1.0, 1.0]);

        let mut fade_out = [1.0; 6];
        apply_fade(&mut fade_out, 2, FadeCurve::Linear, FadeDirection::FadeOut);
        assert_eq!(fade_out, [1.0, 1.0, 0.5, 0.5, 0.0, 0.0]);
    }
}
