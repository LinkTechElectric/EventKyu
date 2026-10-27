use crate::{DownloadError, DownloadResult};

/// Structured progress parsed from yt-dlp `--newline` output.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DownloadProgress {
    /// Completion percentage in the inclusive range 0 through 100.
    pub percent: f64,
    /// Total media size when reported by yt-dlp.
    pub total_bytes: Option<u64>,
    /// Download speed in bytes per second when reported.
    pub speed_bytes_per_second: Option<f64>,
    /// Estimated seconds remaining when reported.
    pub eta_seconds: Option<u64>,
}

impl DownloadProgress {
    /// Creates a percentage-only progress value.
    pub fn new(percent: f64) -> DownloadResult<Self> {
        let progress = Self {
            percent,
            ..Self::default()
        };
        progress.validate()?;
        Ok(progress)
    }

    /// Validates all reported progress values.
    pub fn validate(&self) -> DownloadResult<()> {
        if !self.percent.is_finite() || !(0.0..=100.0).contains(&self.percent) {
            return Err(DownloadError::InvalidProgress(
                "percentage must be finite and between 0 and 100".into(),
            ));
        }
        if self
            .speed_bytes_per_second
            .is_some_and(|speed| !speed.is_finite() || speed < 0.0)
        {
            return Err(DownloadError::InvalidProgress(
                "speed must be finite and non-negative".into(),
            ));
        }
        Ok(())
    }
}

/// Parses a yt-dlp `[download]` progress line.
///
/// Returns `None` for destinations, post-processing messages, malformed
/// values, and other non-progress output. Both binary (`MiB`) and decimal
/// (`MB`) size suffixes are accepted.
pub fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 2 || fields[0] != "[download]" {
        return None;
    }
    let percent = fields[1].strip_suffix('%')?.parse::<f64>().ok()?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return None;
    }

    let mut progress = DownloadProgress {
        percent,
        ..DownloadProgress::default()
    };
    if let Some(of_index) = fields.iter().position(|field| *field == "of") {
        let mut size_index = of_index + 1;
        if fields.get(size_index) == Some(&"~") {
            size_index += 1;
        }
        if let Some(size) = fields.get(size_index) {
            progress.total_bytes = parse_bytes(size.trim_start_matches('~')).and_then(to_u64);
        }
    }
    if let Some(at_index) = fields.iter().position(|field| *field == "at") {
        if let Some(rate) = fields.get(at_index + 1) {
            if let Some(size) = rate.strip_suffix("/s") {
                progress.speed_bytes_per_second = parse_bytes(size);
            }
        }
    }
    if let Some(eta_index) = fields.iter().position(|field| *field == "ETA") {
        if let Some(eta) = fields.get(eta_index + 1) {
            progress.eta_seconds = parse_eta(eta);
        }
    }
    Some(progress)
}

fn parse_bytes(value: &str) -> Option<f64> {
    let number_end = value
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    if number_end == 0 {
        return None;
    }
    let number = value[..number_end].parse::<f64>().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    let unit = value[number_end..].to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" => 1.0,
        "k" | "kb" => 1_000.0,
        "kib" => 1_024.0,
        "m" | "mb" => 1_000_000.0,
        "mib" => 1_048_576.0,
        "g" | "gb" => 1_000_000_000.0,
        "gib" => 1_073_741_824.0,
        "t" | "tb" => 1_000_000_000_000.0,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };
    let bytes = number * multiplier;
    bytes.is_finite().then_some(bytes)
}

fn to_u64(value: f64) -> Option<u64> {
    (value >= 0.0 && value <= u64::MAX as f64).then_some(value as u64)
}

fn parse_eta(value: &str) -> Option<u64> {
    let components: Vec<&str> = value.split(':').collect();
    if !(2..=3).contains(&components.len())
        || components
            .iter()
            .any(|component| component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let numbers: DownloadResult<Vec<u64>> = components
        .iter()
        .map(|component| {
            component
                .parse::<u64>()
                .map_err(|_| DownloadError::InvalidProgress("invalid ETA".into()))
        })
        .collect();
    let numbers = numbers.ok()?;
    let (hours, minutes, seconds) = if numbers.len() == 3 {
        (numbers[0], numbers[1], numbers[2])
    } else {
        (0, numbers[0], numbers[1])
    };
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3600)?
        .checked_add(minutes.checked_mul(60)?)?
        .checked_add(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_percent_speed_total_and_eta() {
        let progress =
            parse_progress_line("[download]  42.5% of   10.00MiB at    1.25MiB/s ETA 00:09")
                .unwrap();
        assert_eq!(progress.percent, 42.5);
        assert_eq!(progress.total_bytes, Some(10_485_760));
        assert_eq!(progress.speed_bytes_per_second, Some(1_310_720.0));
        assert_eq!(progress.eta_seconds, Some(9));
    }

    #[test]
    fn parses_decimal_units_and_long_eta() {
        let progress =
            parse_progress_line("[download] 1.0% of ~2.50GB at 500KB/s ETA 01:02:03").unwrap();
        assert_eq!(progress.total_bytes, Some(2_500_000_000));
        assert_eq!(progress.speed_bytes_per_second, Some(500_000.0));
        assert_eq!(progress.eta_seconds, Some(3723));
    }

    #[test]
    fn parses_complete_line_without_optional_values() {
        let progress = parse_progress_line("[download] 100%").unwrap();
        assert_eq!(progress.percent, 100.0);
        assert_eq!(progress.total_bytes, None);
        assert_eq!(progress.speed_bytes_per_second, None);
        assert_eq!(progress.eta_seconds, None);
    }

    #[test]
    fn tolerates_unknown_speed_and_eta() {
        let progress =
            parse_progress_line("[download] 3.0% of 1.00MiB at Unknown speed ETA Unknown").unwrap();
        assert_eq!(progress.percent, 3.0);
        assert_eq!(progress.speed_bytes_per_second, None);
        assert_eq!(progress.eta_seconds, None);
    }

    #[test]
    fn ignores_non_progress_and_malformed_lines() {
        for line in [
            "[download] Destination: video.mp4",
            "[info] 42.0% of 1MiB",
            "[download] NaN% of 1MiB",
            "[download] 101% of 1MiB",
            "ordinary output",
        ] {
            assert_eq!(parse_progress_line(line), None, "line: {line}");
        }
    }

    #[test]
    fn progress_validation_rejects_nan_and_invalid_speed() {
        assert!(DownloadProgress::new(f64::NAN).is_err());
        let progress = DownloadProgress {
            percent: 1.0,
            speed_bytes_per_second: Some(-1.0),
            ..DownloadProgress::default()
        };
        assert!(progress.validate().is_err());
    }
}
