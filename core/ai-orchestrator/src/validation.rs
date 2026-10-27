use crate::{ExecutionPlan, OrchestratorError, Result, ToolPlan};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Executables that may appear in a [`ToolPlan`].
pub const ALLOWED_PROGRAMS: [&str; 4] = [
    "ffmpeg",
    "ffprobe",
    "realesrgan-ncnn-vulkan",
    "waifu2x-ncnn-vulkan",
];

/// Validates that `program` is exactly one of [`ALLOWED_PROGRAMS`].
pub fn validate_program(program: &str) -> Result<()> {
    if ALLOWED_PROGRAMS.contains(&program) {
        Ok(())
    } else {
        Err(OrchestratorError::ProgramNotAllowed(program.to_owned()))
    }
}

/// Validates every step in an ordered execution plan.
pub fn validate_execution_plan(plan: &ExecutionPlan) -> Result<()> {
    if plan.steps.is_empty() {
        return Err(OrchestratorError::EmptyPlan);
    }
    for step in &plan.steps {
        validate_tool_plan(step)?;
    }
    Ok(())
}

/// Validates one tool invocation without executing it.
///
/// In addition to the executable allowlist, this checks declared inputs and
/// outputs, path-valued options (including script-file options), positional
/// media paths, and absolute paths. Every candidate is normalized through its
/// nearest existing ancestor so an existing symlink cannot redirect access
/// outside `workdir`. Parent-directory components are rejected before
/// normalization, including when they would remain inside the workdir.
pub fn validate_tool_plan(plan: &ToolPlan) -> Result<()> {
    validate_program(&plan.program)?;
    if plan.explanation.trim().is_empty() {
        return Err(OrchestratorError::InvalidPlan(
            "explanation must not be empty".into(),
        ));
    }
    if plan
        .estimated_minutes
        .is_some_and(|minutes| !minutes.is_finite() || minutes < 0.0)
    {
        return Err(OrchestratorError::InvalidPlan(
            "estimated_minutes must be finite and non-negative".into(),
        ));
    }
    if plan.args.iter().any(|arg| arg.contains('\0')) {
        return Err(OrchestratorError::InvalidPlan(
            "arguments must not contain NUL".into(),
        ));
    }

    let root = normalize_root(&plan.workdir)?;
    for input in &plan.inputs {
        normalize_confined_path_at(&root, input)?;
    }
    for output in &plan.outputs {
        normalize_confined_path_at(&root, output)?;
    }
    validate_argv_paths(&plan.program, &plan.args, &root)
}

/// Normalizes a path and verifies that it stays within `workdir`.
///
/// Relative candidates are resolved against `workdir`. This function performs
/// no writes and does not require the final candidate to exist.
pub fn normalize_confined_path(workdir: &Path, candidate: &Path) -> Result<PathBuf> {
    let root = normalize_root(workdir)?;
    normalize_confined_path_at(&root, candidate)
}

fn validate_argv_paths(program: &str, args: &[String], root: &Path) -> Result<()> {
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            for positional in &args[index + 1..] {
                validate_argv_path(root, positional)?;
            }
            return Ok(());
        }
        if argument == "-" || !argument.starts_with('-') {
            validate_argv_path(root, argument)?;
            index += 1;
            continue;
        }

        let (option, inline_value) = argument
            .split_once('=')
            .map_or((argument.as_str(), None), |(name, value)| {
                (name, Some(value))
            });

        if is_path_option(program, option) {
            let value = option_value(args, index, inline_value, option)?;
            validate_argv_path(root, value)?;
            index += if inline_value.is_some() { 1 } else { 2 };
            continue;
        }

        if is_no_value_option(option) {
            index += 1;
            continue;
        }

        if is_known_non_path_option(option) {
            index += if inline_value.is_some() { 1 } else { 2 };
            continue;
        }

        // Unknown options are handled conservatively: their value is treated as
        // a potential path. Harmless scalar values still normalize inside the
        // workdir, while hidden script or protocol paths are rejected.
        if let Some(value) = inline_value {
            validate_argv_path(root, value)?;
            index += 1;
        } else if let Some(value) = args.get(index + 1) {
            validate_argv_path(root, value)?;
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn option_value<'a>(
    args: &'a [String],
    index: usize,
    inline: Option<&'a str>,
    option: &str,
) -> Result<&'a str> {
    if let Some(value) = inline {
        return Ok(value);
    }
    args.get(index + 1).map(String::as_str).ok_or_else(|| {
        OrchestratorError::InvalidPlan(format!("option '{option}' requires a value"))
    })
}

fn is_path_option(program: &str, option: &str) -> bool {
    let base = option.split(':').next().unwrap_or(option);
    matches!(
        base,
        "-i" | "-o"
            | "-m"
            | "-filter_script"
            | "-filter_complex_script"
            | "-passlogfile"
            | "-vstats_file"
            | "-sdp_file"
            | "-attach"
            | "-dump_attachment"
            | "-progress"
    ) || (program.ends_with("ncnn-vulkan") && base == "-model-path")
}

fn is_no_value_option(option: &str) -> bool {
    matches!(
        option,
        "-y" | "-n"
            | "-nostdin"
            | "-hide_banner"
            | "-an"
            | "-vn"
            | "-sn"
            | "-dn"
            | "-shortest"
            | "-copyts"
            | "-start_at_zero"
            | "-noaccurate_seek"
            | "-ignore_unknown"
            | "-xerror"
            | "-stats"
            | "-nostats"
            | "-re"
            | "-noautorotate"
            | "-autorotate"
            | "-benchmark"
            | "-benchmark_all"
            | "-show_format"
            | "-show_streams"
            | "-show_programs"
            | "-show_chapters"
            | "-show_frames"
            | "-show_packets"
            | "-show_error"
            | "-show_data"
            | "-show_private_data"
            | "-count_frames"
            | "-count_packets"
            | "-pretty"
            | "-help"
            | "-version"
    )
}

fn is_known_non_path_option(option: &str) -> bool {
    if option.starts_with("-filter") || option.starts_with("-metadata") {
        return true;
    }
    let base = option.split(':').next().unwrap_or(option);
    matches!(
        base,
        "-f" | "-c"
            | "-codec"
            | "-crf"
            | "-preset"
            | "-profile"
            | "-pix_fmt"
            | "-b"
            | "-q"
            | "-vf"
            | "-af"
            | "-map"
            | "-map_metadata"
            | "-map_chapters"
            | "-disposition"
            | "-scale"
            | "-s"
            | "-r"
            | "-framerate"
            | "-fps_mode"
            | "-frames"
            | "-t"
            | "-to"
            | "-ss"
            | "-sseof"
            | "-stream_loop"
            | "-ar"
            | "-ac"
            | "-ab"
            | "-sample_fmt"
            | "-channel_layout"
            | "-movflags"
            | "-loglevel"
            | "-v"
            | "-of"
            | "-output_format"
            | "-show_entries"
            | "-select_streams"
            | "-read_intervals"
            | "-probesize"
            | "-analyzeduration"
            | "-threads"
            | "-filter_threads"
            | "-filter_complex_threads"
            | "-pass"
            | "-passlogfile-format"
            | "-g"
            | "-j"
            | "-format"
            | "-noise-level"
            | "-scale-ratio"
            | "-tta"
            | "-time"
            | "-timeout"
    )
}

fn validate_argv_path(root: &Path, value: &str) -> Result<()> {
    if value == "-" || is_pipe(value) {
        return Ok(());
    }
    normalize_confined_path_at(root, Path::new(value)).map(|_| ())
}

fn is_pipe(value: &str) -> bool {
    value
        .strip_prefix("pipe:")
        .is_some_and(|fd| !fd.is_empty() && fd.bytes().all(|byte| byte.is_ascii_digit()))
}

fn normalize_root(workdir: &Path) -> Result<PathBuf> {
    if workdir.as_os_str().is_empty() {
        return Err(OrchestratorError::InvalidPath {
            path: String::new(),
            reason: "working directory must not be empty".into(),
        });
    }
    normalize_from_filesystem(&absolute(workdir)?)
}

fn normalize_confined_path_at(root: &Path, candidate: &Path) -> Result<PathBuf> {
    let display = candidate.display().to_string();
    if candidate.as_os_str().is_empty() {
        return Err(OrchestratorError::InvalidPath {
            path: display,
            reason: "path must not be empty".into(),
        });
    }
    if display.chars().any(char::is_control) {
        return Err(OrchestratorError::InvalidPath {
            path: display,
            reason: "path contains a control character".into(),
        });
    }
    if has_uri_scheme(&display) {
        return Err(OrchestratorError::InvalidPath {
            path: display,
            reason: "URI and network-protocol paths are not allowed".into(),
        });
    }
    if candidate
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(OrchestratorError::InvalidPath {
            path: display,
            reason: "parent-directory components are not allowed".into(),
        });
    }

    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let normalized = normalize_from_filesystem(&joined)?;
    if !normalized.starts_with(root) {
        return Err(OrchestratorError::PathOutsideWorkdir {
            path: normalized,
            workdir: root.to_path_buf(),
        });
    }
    Ok(normalized)
}

fn has_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    // A single ASCII letter is left to platform-specific Windows drive
    // handling. Longer schemes (file, http, ftp, concat, and so on) are not
    // local path syntax and are always rejected.
    scheme.len() > 1
        && scheme.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_alphabetic()
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
            }
        })
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| OrchestratorError::Io(error.to_string()))
    }
}

fn normalize_from_filesystem(path: &Path) -> Result<PathBuf> {
    let lexical = normalize_lexical(path);
    let mut ancestor = lexical.as_path();
    let mut suffix: Vec<OsString> = Vec::new();

    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            OrchestratorError::Io(format!("cannot normalize '{}'", lexical.display()))
        })?;
        suffix.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            OrchestratorError::Io(format!("cannot normalize '{}'", lexical.display()))
        })?;
    }

    let mut resolved = fs::canonicalize(ancestor).map_err(|error| {
        OrchestratorError::Io(format!(
            "cannot canonicalize '{}': {error}",
            ancestor.display()
        ))
    })?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(program: &str, args: &[&str]) -> ToolPlan {
        ToolPlan::new(
            program,
            args.iter().map(|arg| (*arg).to_owned()).collect(),
            "test plan",
            "/work",
        )
    }

    #[test]
    fn rejects_non_allowlisted_program() {
        assert!(matches!(
            validate_tool_plan(&plan("rm", &["output.mp4"])),
            Err(OrchestratorError::ProgramNotAllowed(program)) if program == "rm"
        ));
        assert!(validate_tool_plan(&plan("curl", &["https://example.com"])).is_err());
    }

    #[test]
    fn rejects_parent_traversal_in_inputs_and_arguments() {
        let mut input_plan = plan("ffmpeg", &["-i", "input.mp4", "output.mp4"]);
        input_plan.inputs.push(PathBuf::from("nested/../input.mp4"));
        assert!(matches!(
            validate_tool_plan(&input_plan),
            Err(OrchestratorError::InvalidPath { .. })
        ));

        let script_plan = plan(
            "ffmpeg",
            &["-filter_script", "../outside.filter", "output.mp4"],
        );
        assert!(matches!(
            validate_tool_plan(&script_plan),
            Err(OrchestratorError::InvalidPath { .. })
        ));
    }

    #[test]
    fn rejects_output_and_protocol_outside_workdir() {
        let absolute = plan("ffmpeg", &["-i", "input.mp4", "/tmp/output.mp4"]);
        assert!(matches!(
            validate_tool_plan(&absolute),
            Err(OrchestratorError::PathOutsideWorkdir { .. })
        ));

        let protocol = plan("ffprobe", &["file:/etc/passwd"]);
        assert!(matches!(
            validate_tool_plan(&protocol),
            Err(OrchestratorError::InvalidPath { .. })
        ));
    }

    #[test]
    fn accepts_confined_paths_and_filter_graph_slashes() {
        let valid = plan(
            "ffmpeg",
            &[
                "-i",
                "input.mp4",
                "-filter_complex",
                "[0:v]split=2[a][b];[a][b]overlay=(W-w)/2:(H-h)/2",
                "-progress",
                "pipe:1",
                "nested/output.mp4",
            ],
        );
        validate_tool_plan(&valid).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let unique = format!("eventkyu-ai-validation-{}", std::process::id());
        let root = std::env::temp_dir().join(&unique).join("root");
        let outside = std::env::temp_dir().join(&unique).join("outside");
        let _ = fs::remove_dir_all(std::env::temp_dir().join(&unique));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, root.join("link")).unwrap();

        let mut escaping = ToolPlan::new(
            "ffmpeg",
            vec!["-i".into(), "input.mp4".into(), "link/out.mp4".into()],
            "test symlink confinement",
            &root,
        );
        escaping.outputs.push(PathBuf::from("link/out.mp4"));
        assert!(matches!(
            validate_tool_plan(&escaping),
            Err(OrchestratorError::PathOutsideWorkdir { .. })
        ));
        fs::remove_dir_all(std::env::temp_dir().join(unique)).unwrap();
    }
}
