/// Mixes N equally sized input buffers into one output bus with per-input gain.
///
/// The output is overwritten, not added to existing contents. Accumulation is
/// `f32` in input order: different orders can produce different rounding, and
/// very large input counts may lose low bits. No clipping is performed here;
/// place the master-bus [`Limiter`](crate::Limiter) after mixing.
///
/// # Panics
///
/// Panics if input and gain counts differ or any input length differs from the
/// output length.
pub fn mix(inputs: &[&[f32]], gains: &[f32], output: &mut [f32]) {
    assert_eq!(inputs.len(), gains.len(), "one gain is required per input");
    assert!(
        inputs.iter().all(|input| input.len() == output.len()),
        "all inputs must match the output length"
    );

    output.fill(0.0);
    for (input, gain) in inputs.iter().zip(gains) {
        for (output_sample, input_sample) in output.iter_mut().zip(input.iter()) {
            *output_sample += *input_sample * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mix;

    #[test]
    fn mixer_accumulates_scaled_inputs() {
        let first = [1.0, 2.0, 3.0, 4.0];
        let second = [0.5, -0.5, 1.0, -1.0];
        let third = [2.0; 4];
        let mut output = [99.0; 4];
        mix(&[&first, &second, &third], &[0.5, 2.0, 0.25], &mut output);
        assert_eq!(output, [2.0, 0.5, 4.0, 0.5]);
    }

    #[test]
    fn mixer_accepts_no_inputs_as_silence() {
        let mut output = [1.0; 4];
        mix(&[], &[], &mut output);
        assert_eq!(output, [0.0; 4]);
    }
}
