/// Interpolation curve used for volume and opacity fades.
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
    /// Evaluates the curve at `progress` and returns a value from 0.0 to 1.0.
    ///
    /// Values outside the range are clamped; NaN is treated as zero.
    pub fn evaluate(self, progress: f32) -> f32 {
        if progress.is_nan() {
            return 0.0;
        }
        let t = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
            Self::SCurve => t * t * (3.0 - 2.0 * t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FadeCurve;

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn linear_midpoint() {
        close(FadeCurve::Linear.evaluate(0.5), 0.5);
    }

    #[test]
    fn equal_power_midpoint() {
        close(FadeCurve::EqualPower.evaluate(0.5), 0.5_f32.sqrt());
    }

    #[test]
    fn s_curve_midpoint() {
        close(FadeCurve::SCurve.evaluate(0.5), 0.5);
    }

    #[test]
    fn curves_hit_exact_endpoints_and_clamp() {
        for curve in [FadeCurve::Linear, FadeCurve::EqualPower, FadeCurve::SCurve] {
            close(curve.evaluate(-1.0), 0.0);
            close(curve.evaluate(0.0), 0.0);
            close(curve.evaluate(1.0), 1.0);
            close(curve.evaluate(2.0), 1.0);
        }
    }
}
