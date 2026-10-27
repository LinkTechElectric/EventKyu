use crate::{validate_execution_plan, ExecutionPlan, OrchestratorError, Result, ToolPlan};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Pixel dimensions used by scaling and padding presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

impl Resolution {
    /// Creates a resolution after checking that both dimensions are nonzero.
    pub fn new(width: u32, height: u32) -> Result<Self> {
        let resolution = Self { width, height };
        resolution.validate()?;
        Ok(resolution)
    }

    fn validate(self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            Err(OrchestratorError::InvalidPreset(
                "resolution dimensions must be nonzero".into(),
            ))
        } else {
            Ok(())
        }
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

/// H.264 encoder backends supported by the general-purpose preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum H264Encoder {
    /// Software encoding with libx264 and CRF.
    LibX264,
    /// macOS VideoToolbox hardware encoding.
    H264Videotoolbox,
}

impl H264Encoder {
    fn as_str(self) -> &'static str {
        match self {
            Self::LibX264 => "libx264",
            Self::H264Videotoolbox => "h264_videotoolbox",
        }
    }
}

/// Parameters for the general H.264 transcode preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct H264Options {
    /// Target canvas; aspect ratio is preserved with scaling and padding.
    pub resolution: Resolution,
    /// libx264 constant rate factor. The default and recommended value is 18.
    pub crf: u8,
    /// Software or VideoToolbox encoder.
    pub encoder: H264Encoder,
}

impl Default for H264Options {
    fn default() -> Self {
        Self {
            resolution: Resolution::default(),
            crf: 18,
            encoder: H264Encoder::LibX264,
        }
    }
}

/// Creates a general-purpose H.264 transcode, defaulting to 1080p and CRF 18.
pub fn h264_1080p(
    input: &Path,
    output: &Path,
    workdir: &Path,
    options: H264Options,
) -> Result<ExecutionPlan> {
    options.resolution.validate()?;
    validate_crf(options.crf)?;
    let Resolution { width, height } = options.resolution;
    let scale_pad = format!(
        "scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2"
    );
    let mut args = strings(&[
        "-hide_banner",
        "-nostdin",
        "-y",
        "-i",
        &input.to_string_lossy(),
        "-vf",
        &scale_pad,
        "-c:v",
        options.encoder.as_str(),
    ]);
    match options.encoder {
        H264Encoder::LibX264 => {
            args.extend(strings(&[
                "-crf",
                &options.crf.to_string(),
                "-preset",
                "medium",
            ]));
        }
        H264Encoder::H264Videotoolbox => {
            args.extend(strings(&["-q:v", "65"]));
        }
    }
    args.extend(strings(&[
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-movflags",
        "+faststart",
        &output.to_string_lossy(),
    ]));
    single_step(
        "ffmpeg",
        args,
        "Transcode to H.264 with aspect-preserving scale and padding",
        workdir,
        vec![input.to_path_buf()],
        vec![output.to_path_buf()],
        Some(5.0),
    )
}

/// Creates a ProRes 422 preset using `prores_ks` profile 3.
pub fn prores_422(input: &Path, output: &Path, workdir: &Path) -> Result<ExecutionPlan> {
    let args = strings(&[
        "-hide_banner",
        "-nostdin",
        "-y",
        "-i",
        &input.to_string_lossy(),
        "-c:v",
        "prores_ks",
        "-profile:v",
        "3",
        "-pix_fmt",
        "yuv422p10le",
        "-c:a",
        "pcm_s24le",
        &output.to_string_lossy(),
    ]);
    single_step(
        "ffmpeg",
        args,
        "Transcode video to ProRes 422 (profile 3)",
        workdir,
        vec![input.to_path_buf()],
        vec![output.to_path_buf()],
        Some(5.0),
    )
}

/// Creates a horizontal canvas from vertical video using a blurred background.
///
/// The filter graph splits the source, enlarges and blurs one branch, scales
/// the foreground without cropping, and overlays it at the canvas center.
pub fn vertical_to_horizontal_blur(
    input: &Path,
    output: &Path,
    workdir: &Path,
    resolution: Resolution,
) -> Result<ExecutionPlan> {
    resolution.validate()?;
    let Resolution { width, height } = resolution;
    let filter = format!(
        "[0:v]split=2[bg][fg];[bg]scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},boxblur=20:10[blurred];[fg]scale={width}:{height}:force_original_aspect_ratio=decrease[scaled];[blurred][scaled]overlay=(W-w)/2:(H-h)/2"
    );
    let args = strings(&[
        "-hide_banner",
        "-nostdin",
        "-y",
        "-i",
        &input.to_string_lossy(),
        "-filter_complex",
        &filter,
        "-map",
        "0:a?",
        "-c:v",
        "libx264",
        "-crf",
        "18",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        &output.to_string_lossy(),
    ]);
    single_step(
        "ffmpeg",
        args,
        "Place vertical video over a cropped, blurred horizontal background",
        workdir,
        vec![input.to_path_buf()],
        vec![output.to_path_buf()],
        Some(6.0),
    )
}

/// First-pass measurements required by ffmpeg's linear loudnorm second pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoudnormMeasurement {
    /// Measured integrated loudness (`measured_I`).
    pub input_i: f64,
    /// Measured true peak (`measured_TP`).
    pub input_tp: f64,
    /// Measured loudness range (`measured_LRA`).
    pub input_lra: f64,
    /// Measured threshold (`measured_thresh`).
    pub input_thresh: f64,
    /// Target offset (`offset`) reported by the first pass.
    pub target_offset: f64,
}

impl LoudnormMeasurement {
    fn validate(self) -> Result<()> {
        let values = [
            self.input_i,
            self.input_tp,
            self.input_lra,
            self.input_thresh,
            self.target_offset,
        ];
        if values.iter().all(|value| value.is_finite()) {
            Ok(())
        } else {
            Err(OrchestratorError::InvalidPreset(
                "loudnorm measurements must all be finite".into(),
            ))
        }
    }
}

/// Creates ffmpeg loudnorm measurement and linear normalization passes.
///
/// The returned plan contains two steps. `measurement` comes from the JSON
/// printed by the first pass; the same first pass is included so the template
/// remains self-contained and can also be used to refresh those measurements.
pub fn loudnorm_two_pass(
    input: &Path,
    output: &Path,
    workdir: &Path,
    measurement: LoudnormMeasurement,
) -> Result<ExecutionPlan> {
    measurement.validate()?;
    let target = "I=-16:TP=-1.5:LRA=11";
    let first_filter = format!("loudnorm={target}:print_format=json");
    let first = ToolPlan {
        program: "ffmpeg".into(),
        args: strings(&[
            "-hide_banner",
            "-nostdin",
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-af",
            &first_filter,
            "-f",
            "null",
            "-",
        ]),
        explanation: "Measure integrated loudness, true peak, LRA, and threshold".into(),
        estimated_minutes: Some(3.0),
        workdir: workdir.to_path_buf(),
        inputs: vec![input.to_path_buf()],
        outputs: Vec::new(),
    };
    let second_filter = format!(
        "loudnorm={target}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=summary",
        measurement.input_i,
        measurement.input_tp,
        measurement.input_lra,
        measurement.input_thresh,
        measurement.target_offset
    );
    let second = ToolPlan {
        program: "ffmpeg".into(),
        args: strings(&[
            "-hide_banner",
            "-nostdin",
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-af",
            &second_filter,
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            &output.to_string_lossy(),
        ]),
        explanation: "Apply linear loudness normalization using first-pass measurements".into(),
        estimated_minutes: Some(3.0),
        workdir: workdir.to_path_buf(),
        inputs: vec![input.to_path_buf()],
        outputs: vec![output.to_path_buf()],
    };
    validated(ExecutionPlan::new(vec![first, second]))
}

/// Local ncnn-vulkan upscaling backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpscaleEngine {
    /// Real-ESRGAN ncnn Vulkan executable.
    RealEsrGan,
    /// waifu2x ncnn Vulkan executable.
    Waifu2x,
}

impl UpscaleEngine {
    fn program(self) -> &'static str {
        match self {
            Self::RealEsrGan => "realesrgan-ncnn-vulkan",
            Self::Waifu2x => "waifu2x-ncnn-vulkan",
        }
    }
}

/// Parameters for the extract/upscale/reassemble superresolution chain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UpscaleOptions {
    /// Local upscaling executable.
    pub engine: UpscaleEngine,
    /// Integer scale factor passed to the ncnn tool.
    pub scale: u8,
    /// Frame rate used when reassembling the image sequence.
    pub fps: f64,
    /// libx264 CRF used for the reassembled video.
    pub crf: u8,
}

impl Default for UpscaleOptions {
    fn default() -> Self {
        Self {
            engine: UpscaleEngine::RealEsrGan,
            scale: 2,
            fps: 30.0,
            crf: 18,
        }
    }
}

/// Creates a three-step superresolution chain: frames, upscale, and reassemble.
///
/// Intermediate image directories are confined beneath `workdir`; no Python,
/// shell script, network service, or additional executable is introduced.
pub fn superresolution_three_step(
    input: &Path,
    output: &Path,
    workdir: &Path,
    options: UpscaleOptions,
) -> Result<ExecutionPlan> {
    validate_crf(options.crf)?;
    if !matches!(options.scale, 2..=4) {
        return Err(OrchestratorError::InvalidPreset(
            "upscale factor must be 2, 3, or 4".into(),
        ));
    }
    if !options.fps.is_finite() || options.fps <= 0.0 {
        return Err(OrchestratorError::InvalidPreset(
            "upscale fps must be finite and positive".into(),
        ));
    }
    let frames = PathBuf::from("signalfit_frames");
    let upscaled = PathBuf::from("signalfit_upscaled");
    let frame_pattern = frames.join("frame_%08d.png");
    let upscaled_pattern = upscaled.join("frame_%08d.png");

    let extract = ToolPlan {
        program: "ffmpeg".into(),
        args: strings(&[
            "-hide_banner",
            "-nostdin",
            "-y",
            "-i",
            &input.to_string_lossy(),
            "-map",
            "0:v:0",
            "-vsync",
            "0",
            &frame_pattern.to_string_lossy(),
        ]),
        explanation: "Extract source frames to a confined working directory".into(),
        estimated_minutes: Some(3.0),
        workdir: workdir.to_path_buf(),
        inputs: vec![input.to_path_buf()],
        outputs: vec![frame_pattern],
    };
    let upscale = ToolPlan {
        program: options.engine.program().into(),
        args: strings(&[
            "-i",
            &frames.to_string_lossy(),
            "-o",
            &upscaled.to_string_lossy(),
            "-s",
            &options.scale.to_string(),
        ]),
        explanation: "Upscale every extracted frame with the selected local ncnn backend".into(),
        estimated_minutes: Some(20.0),
        workdir: workdir.to_path_buf(),
        inputs: vec![frames],
        outputs: vec![upscaled],
    };
    let reassemble = ToolPlan {
        program: "ffmpeg".into(),
        args: strings(&[
            "-hide_banner",
            "-nostdin",
            "-y",
            "-framerate",
            &options.fps.to_string(),
            "-i",
            &upscaled_pattern.to_string_lossy(),
            "-i",
            &input.to_string_lossy(),
            "-map",
            "0:v:0",
            "-map",
            "1:a?",
            "-c:v",
            "libx264",
            "-crf",
            &options.crf.to_string(),
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "copy",
            "-shortest",
            &output.to_string_lossy(),
        ]),
        explanation: "Reassemble upscaled frames and copy the original audio stream".into(),
        estimated_minutes: Some(5.0),
        workdir: workdir.to_path_buf(),
        inputs: vec![upscaled_pattern, input.to_path_buf()],
        outputs: vec![output.to_path_buf()],
    };
    validated(ExecutionPlan::new(vec![extract, upscale, reassemble]))
}

fn single_step(
    program: &str,
    args: Vec<String>,
    explanation: &str,
    workdir: &Path,
    inputs: Vec<PathBuf>,
    outputs: Vec<PathBuf>,
    estimated_minutes: Option<f64>,
) -> Result<ExecutionPlan> {
    validated(ExecutionPlan::single(ToolPlan {
        program: program.into(),
        args,
        explanation: explanation.into(),
        estimated_minutes,
        workdir: workdir.to_path_buf(),
        inputs,
        outputs,
    }))
}

fn validated(plan: ExecutionPlan) -> Result<ExecutionPlan> {
    validate_execution_plan(&plan)?;
    Ok(plan)
}

fn validate_crf(crf: u8) -> Result<()> {
    if crf <= 51 {
        Ok(())
    } else {
        Err(OrchestratorError::InvalidPreset(
            "CRF must be between 0 and 51".into(),
        ))
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_after<'a>(args: &'a [String], option: &str) -> &'a str {
        let index = args.iter().position(|arg| arg == option).unwrap();
        args[index + 1].as_str()
    }

    #[test]
    fn h264_preset_has_1080p_crf18_and_optional_videotoolbox() {
        let plan = h264_1080p(
            Path::new("input.mov"),
            Path::new("output.mp4"),
            Path::new("/work"),
            H264Options::default(),
        )
        .unwrap();
        let args = &plan.steps[0].args;
        assert_eq!(value_after(args, "-c:v"), "libx264");
        assert_eq!(value_after(args, "-crf"), "18");
        assert!(value_after(args, "-vf").contains("scale=1920:1080"));

        let hardware = h264_1080p(
            Path::new("input.mov"),
            Path::new("output.mp4"),
            Path::new("/work"),
            H264Options {
                encoder: H264Encoder::H264Videotoolbox,
                ..H264Options::default()
            },
        )
        .unwrap();
        assert_eq!(
            value_after(&hardware.steps[0].args, "-c:v"),
            "h264_videotoolbox"
        );
    }

    #[test]
    fn prores_preset_uses_prores_ks_profile_3() {
        let plan = prores_422(
            Path::new("input.mp4"),
            Path::new("output.mov"),
            Path::new("/work"),
        )
        .unwrap();
        let args = &plan.steps[0].args;
        assert_eq!(value_after(args, "-c:v"), "prores_ks");
        assert_eq!(value_after(args, "-profile:v"), "3");
        assert_eq!(value_after(args, "-pix_fmt"), "yuv422p10le");
    }

    #[test]
    fn vertical_preset_contains_split_boxblur_and_overlay_graph() {
        let plan = vertical_to_horizontal_blur(
            Path::new("vertical.mp4"),
            Path::new("horizontal.mp4"),
            Path::new("/work"),
            Resolution::default(),
        )
        .unwrap();
        let graph = value_after(&plan.steps[0].args, "-filter_complex");
        assert!(graph.contains("split=2"));
        assert!(graph.contains("boxblur=20:10"));
        assert!(graph.contains("overlay=(W-w)/2:(H-h)/2"));
    }

    #[test]
    fn loudnorm_preset_has_measurement_and_linear_second_pass() {
        let plan = loudnorm_two_pass(
            Path::new("input.mp4"),
            Path::new("normalized.mp4"),
            Path::new("/work"),
            LoudnormMeasurement {
                input_i: -20.0,
                input_tp: -3.0,
                input_lra: 7.0,
                input_thresh: -30.0,
                target_offset: 0.5,
            },
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 2);
        let first = value_after(&plan.steps[0].args, "-af");
        assert!(first.contains("loudnorm=I=-16:TP=-1.5:LRA=11"));
        assert!(first.contains("print_format=json"));
        let second = value_after(&plan.steps[1].args, "-af");
        assert!(second.contains("measured_I=-20"));
        assert!(second.contains("measured_TP=-3"));
        assert!(second.contains("linear=true"));
    }

    #[test]
    fn superresolution_preset_has_three_expected_programs_and_paths() {
        let plan = superresolution_three_step(
            Path::new("input.mp4"),
            Path::new("upscaled.mp4"),
            Path::new("/work"),
            UpscaleOptions::default(),
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].program, "ffmpeg");
        assert_eq!(plan.steps[1].program, "realesrgan-ncnn-vulkan");
        assert_eq!(plan.steps[2].program, "ffmpeg");
        assert!(plan.steps[0]
            .args
            .iter()
            .any(|arg| arg == "signalfit_frames/frame_%08d.png"));
        assert_eq!(value_after(&plan.steps[1].args, "-s"), "2");
        assert!(plan.steps[2]
            .args
            .iter()
            .any(|arg| arg == "signalfit_upscaled/frame_%08d.png"));
        assert_eq!(value_after(&plan.steps[2].args, "-crf"), "18");
    }
}
