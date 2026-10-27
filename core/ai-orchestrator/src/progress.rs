use crate::{OrchestratorError, Result};
use serde::{Deserialize, Serialize};

/// Terminal state reported by ffmpeg's `progress=` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressStatus {
    /// Processing is still running.
    Continue,
    /// Processing has finished.
    End,
}

/// One structured update parsed from ffmpeg `-progress pipe:1` output.
///
/// Each output line updates one field, so unrelated fields remain `None`.
/// FFmpeg's historical `out_time_ms` name is preserved; applications should
/// follow ffmpeg's own unit convention when displaying it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FfmpegProgress {
    /// Current encoded frame number.
    pub frame: Option<u64>,
    /// Current encoding speed in frames per second.
    pub fps: Option<f64>,
    /// Raw value of ffmpeg's `out_time_ms` field.
    pub out_time_ms: Option<i64>,
    /// Continue/end marker, when this line contains one.
    pub progress: Option<ProgressStatus>,
}

impl FfmpegProgress {
    fn empty() -> Self {
        Self {
            frame: None,
            fps: None,
            out_time_ms: None,
            progress: None,
        }
    }
}

/// Parses one `key=value` line emitted by ffmpeg's progress writer.
///
/// Unknown keys and non-key/value lines return `Ok(None)`. Malformed values
/// for recognized keys return a structured error instead of being silently
/// accepted.
pub fn parse_progress_line(line: &str) -> Result<Option<FfmpegProgress>> {
    let Some((key, value)) = line.trim().split_once('=') else {
        return Ok(None);
    };
    let mut update = FfmpegProgress::empty();
    match key {
        "frame" => {
            update.frame = Some(value.parse::<u64>().map_err(|_| {
                OrchestratorError::InvalidProgress(format!("invalid frame value '{value}'"))
            })?);
        }
        "fps" => {
            let fps = value.parse::<f64>().map_err(|_| {
                OrchestratorError::InvalidProgress(format!("invalid fps value '{value}'"))
            })?;
            if !fps.is_finite() || fps < 0.0 {
                return Err(OrchestratorError::InvalidProgress(format!(
                    "fps must be finite and non-negative, got '{value}'"
                )));
            }
            update.fps = Some(fps);
        }
        "out_time_ms" => {
            update.out_time_ms = Some(value.parse::<i64>().map_err(|_| {
                OrchestratorError::InvalidProgress(format!("invalid out_time_ms value '{value}'"))
            })?);
        }
        "progress" => {
            update.progress = Some(match value {
                "continue" => ProgressStatus::Continue,
                "end" => ProgressStatus::End,
                _ => {
                    return Err(OrchestratorError::InvalidProgress(format!(
                        "unknown progress status '{value}'"
                    )))
                }
            });
        }
        _ => return Ok(None),
    }
    Ok(Some(update))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_progress_fields() {
        assert_eq!(
            parse_progress_line("frame=120").unwrap().unwrap().frame,
            Some(120)
        );
        assert_eq!(
            parse_progress_line("fps=29.97").unwrap().unwrap().fps,
            Some(29.97)
        );
        assert_eq!(
            parse_progress_line("out_time_ms=4000000")
                .unwrap()
                .unwrap()
                .out_time_ms,
            Some(4_000_000)
        );
    }

    #[test]
    fn parses_continue_and_end_markers() {
        assert_eq!(
            parse_progress_line("progress=continue")
                .unwrap()
                .unwrap()
                .progress,
            Some(ProgressStatus::Continue)
        );
        assert_eq!(
            parse_progress_line("progress=end")
                .unwrap()
                .unwrap()
                .progress,
            Some(ProgressStatus::End)
        );
    }

    #[test]
    fn ignores_other_ffmpeg_keys_and_plain_output() {
        assert_eq!(parse_progress_line("speed=1.2x").unwrap(), None);
        assert_eq!(parse_progress_line("ordinary log").unwrap(), None);
    }

    #[test]
    fn rejects_malformed_known_values() {
        assert!(parse_progress_line("frame=many").is_err());
        assert!(parse_progress_line("fps=NaN").is_err());
        assert!(parse_progress_line("progress=perhaps").is_err());
    }
}
