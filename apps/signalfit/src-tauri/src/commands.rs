//! Tauri command boundary and the user-confirmation workflow.

use crate::ai::{self, AiProviderConfig};
use crate::keystore::PlatformKeyStore;
use crate::process::{LocalProcessRunner, OutputStream, ProcessOutput};
use ai_orchestrator::{
    h264_1080p, loudnorm_two_pass, parse_progress_line, prores_422, superresolution_three_step,
    vertical_to_horizontal_blur, AttemptLog, ExecutionEngine, ExecutionLog, ExecutionPlan,
    ExecutionReport, FfmpegProgress, H264Encoder, H264Options, KeyStore, LoudnormMeasurement,
    PlanSession, PlanState, Resolution, UpscaleEngine, UpscaleOptions,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, State};

static NEXT_PLAN_ID: AtomicU64 = AtomicU64::new(1);

/// Serializable preset metadata used by the toolbox cards.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetDescriptor {
    /// Stable command identifier.
    pub id: &'static str,
    /// Short user-facing name.
    pub name: &'static str,
    /// Capability summary.
    pub description: &'static str,
    /// Required external tools.
    pub tools: &'static [&'static str],
}

/// Common and preset-specific parameters supplied by the parameter form.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetParams {
    /// Input path, absolute or relative to `workdir`.
    pub input: String,
    /// Output path, absolute or relative to `workdir`.
    pub output: String,
    /// User-selected confinement and process working directory.
    pub workdir: PathBuf,
    /// Optional output width; defaults to 1920.
    pub width: Option<u32>,
    /// Optional output height; defaults to 1080.
    pub height: Option<u32>,
    /// H.264/upscale CRF; defaults to 18.
    pub crf: Option<u8>,
    /// `libx264` or `h264_videotoolbox`.
    pub encoder: Option<String>,
    /// `realesrgan` or `waifu2x`.
    pub upscale_engine: Option<String>,
    /// Superresolution scale factor (2–4).
    pub scale: Option<u8>,
    /// Frame rate for superresolution reassembly.
    pub fps: Option<f64>,
    /// First-pass loudnorm values used by the second pass.
    pub loudnorm: Option<LoudnormMeasurement>,
}

/// A validated plan awaiting explicit user confirmation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedPlan {
    /// Opaque session identifier used by approval and execution commands.
    pub plan_id: String,
    /// Exact plan that will be approved.
    pub plan: ExecutionPlan,
}

/// One process attempt in an execution report.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptView {
    /// One-based attempt number.
    pub attempt: usize,
    /// Zero-based plan step index.
    pub step_index: usize,
    /// Allowlisted executable.
    pub program: String,
    /// Exit status, absent if startup/communication failed.
    pub exit_code: Option<i32>,
    /// Complete standard-output lines.
    pub stdout: Vec<String>,
    /// Complete standard-error lines.
    pub stderr: Vec<String>,
    /// Process error, if any.
    pub error: Option<String>,
}

/// Successful execution response returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReportView {
    /// Final executed plan.
    pub plan: ExecutionPlan,
    /// Complete process history.
    pub attempts: Vec<AttemptView>,
    /// Plain-text export of the same complete history.
    pub log_text: String,
}

#[derive(Default)]
struct StateInner {
    sessions: HashMap<String, PlanSession>,
    cancellations: HashMap<String, Arc<AtomicBool>>,
    last_log: ExecutionLog,
}

/// Managed application state. No credentials are stored here.
#[derive(Default)]
pub struct AppState {
    inner: Mutex<StateInner>,
}

impl AppState {
    fn lock(&self) -> Result<MutexGuard<'_, StateInner>, String> {
        self.inner
            .lock()
            .map_err(|_| "SignalFit 内部状态锁不可用".to_owned())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputEvent {
    plan_id: String,
    stream: String,
    line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    plan_id: String,
    progress: FfmpegProgress,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StateEvent {
    plan_id: String,
    state: String,
    message: Option<String>,
}

/// Lists the five offline media presets.
#[tauri::command]
pub fn list_presets() -> Vec<PresetDescriptor> {
    preset_descriptors()
}

/// Builds a preset plan without starting any process.
#[tauri::command]
pub fn preset_plan(preset_id: String, params: PresetParams) -> Result<ExecutionPlan, String> {
    build_preset_plan(&preset_id, &params).map_err(|error| error.to_string())
}

/// Validates a plan and moves it to `AwaitingConfirmation`, returning its ID.
#[tauri::command]
pub fn plan_validate(
    plan: ExecutionPlan,
    state: State<'_, AppState>,
) -> Result<ValidatedPlan, String> {
    let session = prepare_session(plan.clone()).map_err(|error| error.to_string())?;
    let plan_id = format!("plan-{}", NEXT_PLAN_ID.fetch_add(1, Ordering::Relaxed));
    let mut inner = state.lock()?;
    inner.sessions.insert(plan_id.clone(), session);
    inner
        .cancellations
        .insert(plan_id.clone(), Arc::new(AtomicBool::new(false)));
    Ok(ValidatedPlan { plan_id, plan })
}

/// Approves the exact plan currently awaiting confirmation.
#[tauri::command]
pub fn plan_approve(plan_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut inner = state.lock()?;
    inner
        .sessions
        .get_mut(&plan_id)
        .ok_or_else(|| "计划不存在或已结束".to_owned())?
        .approve()
        .map_err(|error| error.to_string())
}

/// Rejects a plan before execution.
#[tauri::command]
pub fn plan_reject(plan_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut inner = state.lock()?;
    inner
        .sessions
        .get_mut(&plan_id)
        .ok_or_else(|| "计划不存在或已结束".to_owned())?
        .reject()
        .map_err(|error| error.to_string())?;
    inner.cancellations.remove(&plan_id);
    Ok(())
}

/// Cooperatively cancels a running process or cancels a pre-execution session.
#[tauri::command]
pub fn plan_cancel(plan_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut inner = state.lock()?;
    if let Some(cancelled) = inner.cancellations.get(&plan_id) {
        cancelled.store(true, Ordering::SeqCst);
    }
    if let Some(session) = inner.sessions.get_mut(&plan_id) {
        session.cancel().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Executes an approved plan and emits output/progress events while it runs.
#[tauri::command(async)]
pub fn plan_execute(
    plan_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ExecutionReportView, String> {
    let (mut session, cancelled) = {
        let mut inner = state.lock()?;
        let session = inner
            .sessions
            .remove(&plan_id)
            .ok_or_else(|| "计划不存在、正在执行或已结束".to_owned())?;
        let cancelled = inner
            .cancellations
            .get(&plan_id)
            .cloned()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        (session, cancelled)
    };
    cancelled.store(false, Ordering::SeqCst);

    let allowed_root = session
        .plan()
        .steps
        .first()
        .map(|step| step.workdir.clone())
        .ok_or_else(|| "执行计划为空".to_owned())?;
    emit_state(&app, &plan_id, "executing", None);

    let observer_app = app.clone();
    let observer_plan_id = plan_id.clone();
    let observer = Arc::new(move |output: ProcessOutput| {
        let stream = match output.stream {
            OutputStream::Stdout => "stdout",
            OutputStream::Stderr => "stderr",
        };
        let _ = observer_app.emit(
            "signalfit-output",
            OutputEvent {
                plan_id: observer_plan_id.clone(),
                stream: stream.to_owned(),
                line: output.line.clone(),
            },
        );
        if output.stream == OutputStream::Stdout {
            if let Ok(Some(progress)) = parse_progress_line(&output.line) {
                let _ = observer_app.emit(
                    "signalfit-progress",
                    ProgressEvent {
                        plan_id: observer_plan_id.clone(),
                        progress,
                    },
                );
            }
        }
    });

    let mut runner = LocalProcessRunner::new(allowed_root)
        .with_cancellation(cancelled)
        .with_observer(observer);
    let result = ExecutionEngine::default().execute(&mut session, &mut runner);
    let final_state = session.state();
    let last_log = session.log().clone();
    let last_error = session.last_error().map(str::to_owned);

    {
        let mut inner = state.lock()?;
        inner.last_log = last_log;
        inner.sessions.insert(plan_id.clone(), session);
    }

    match result {
        Ok(report) => {
            emit_state(&app, &plan_id, "done", None);
            Ok(report_view(report))
        }
        Err(error) => {
            let message = last_error.unwrap_or_else(|| error.to_string());
            emit_state(
                &app,
                &plan_id,
                if final_state == PlanState::Cancelled {
                    "cancelled"
                } else {
                    "failed"
                },
                Some(message.clone()),
            );
            Err(message)
        }
    }
}

/// Exports the most recent complete execution log, including failed attempts.
#[tauri::command]
pub fn export_log(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.lock()?.last_log.export_text())
}

/// Generates a validated AI plan from metadata and a natural-language request.
#[tauri::command(async)]
pub fn ai_generate(
    config: AiProviderConfig,
    ffprobe_metadata: String,
    requirement: String,
) -> Result<ExecutionPlan, String> {
    ai::generate(config, &ffprobe_metadata, &requirement).map_err(|error| error.to_string())
}

/// Stores or replaces a provider key in operating-system protected storage.
#[tauri::command(async)]
pub fn key_set(provider: String, key: String) -> Result<(), String> {
    PlatformKeyStore::new()
        .set(&provider, &key)
        .map_err(|error| error.to_string())
}

/// Reports whether a key exists without returning its value.
#[tauri::command(async)]
pub fn key_status(provider: String) -> Result<bool, String> {
    PlatformKeyStore::new()
        .get(&provider)
        .map(|key| key.is_some())
        .map_err(|error| error.to_string())
}

/// Deletes a provider key from protected storage.
#[tauri::command(async)]
pub fn key_delete(provider: String) -> Result<bool, String> {
    PlatformKeyStore::new()
        .delete(&provider)
        .map_err(|error| error.to_string())
}

fn preset_descriptors() -> Vec<PresetDescriptor> {
    vec![
        PresetDescriptor {
            id: "h264-1080p",
            name: "1080P H.264",
            description: "保持比例缩放并填充，输出通用 H.264/AAC 文件",
            tools: &["ffmpeg"],
        },
        PresetDescriptor {
            id: "prores-422",
            name: "ProRes 422",
            description: "prores_ks profile 3，10-bit 4:2:2 与 24-bit PCM 音频",
            tools: &["ffmpeg"],
        },
        PresetDescriptor {
            id: "vertical-blur",
            name: "竖屏模糊填充",
            description: "竖屏视频居中，放大裁切并模糊的背景填满横屏",
            tools: &["ffmpeg"],
        },
        PresetDescriptor {
            id: "loudnorm-two-pass",
            name: "响度归一化（两遍）",
            description: "测量 EBU R128 指标，再按第一遍数据做线性 loudnorm",
            tools: ["ffmpeg"].as_slice(),
        },
        PresetDescriptor {
            id: "superresolution",
            name: "离线超分三步链",
            description: "抽帧 → ncnn Vulkan 超分 → 重组视频并保留原音频",
            tools: &["ffmpeg", "realesrgan-ncnn-vulkan / waifu2x-ncnn-vulkan"],
        },
    ]
}

fn build_preset_plan(
    preset_id: &str,
    params: &PresetParams,
) -> ai_orchestrator::Result<ExecutionPlan> {
    let input = PathBuf::from(&params.input);
    let output = PathBuf::from(&params.output);
    let resolution = Resolution::new(params.width.unwrap_or(1920), params.height.unwrap_or(1080))?;
    match preset_id {
        "h264-1080p" => h264_1080p(
            &input,
            &output,
            &params.workdir,
            H264Options {
                resolution,
                crf: params.crf.unwrap_or(18),
                encoder: match params.encoder.as_deref().unwrap_or("libx264") {
                    "libx264" => H264Encoder::LibX264,
                    "h264_videotoolbox" => H264Encoder::H264Videotoolbox,
                    other => {
                        return Err(ai_orchestrator::OrchestratorError::InvalidPreset(format!(
                            "未知 H.264 编码器：{other}"
                        )))
                    }
                },
            },
        ),
        "prores-422" => prores_422(&input, &output, &params.workdir),
        "vertical-blur" => {
            vertical_to_horizontal_blur(&input, &output, &params.workdir, resolution)
        }
        "loudnorm-two-pass" => {
            let measurement = params.loudnorm.ok_or_else(|| {
                ai_orchestrator::OrchestratorError::InvalidPreset(
                    "响度归一化需要第一遍测量值".into(),
                )
            })?;
            loudnorm_two_pass(&input, &output, &params.workdir, measurement)
        }
        "superresolution" => superresolution_three_step(
            &input,
            &output,
            &params.workdir,
            UpscaleOptions {
                engine: match params.upscale_engine.as_deref().unwrap_or("realesrgan") {
                    "realesrgan" => UpscaleEngine::RealEsrGan,
                    "waifu2x" => UpscaleEngine::Waifu2x,
                    other => {
                        return Err(ai_orchestrator::OrchestratorError::InvalidPreset(format!(
                            "未知超分引擎：{other}"
                        )))
                    }
                },
                scale: params.scale.unwrap_or(2),
                fps: params.fps.unwrap_or(30.0),
                crf: params.crf.unwrap_or(18),
            },
        ),
        other => Err(ai_orchestrator::OrchestratorError::InvalidPreset(format!(
            "未知预设：{other}"
        ))),
    }
}

fn prepare_session(plan: ExecutionPlan) -> ai_orchestrator::Result<PlanSession> {
    let mut session = PlanSession::new(plan);
    session.validate()?;
    session.request_confirmation()?;
    Ok(session)
}

fn report_view(report: ExecutionReport) -> ExecutionReportView {
    let attempts = report.log.attempts.iter().map(attempt_view).collect();
    let log_text = report.log.export_text();
    ExecutionReportView {
        plan: report.plan,
        attempts,
        log_text,
    }
}

fn attempt_view(attempt: &AttemptLog) -> AttemptView {
    AttemptView {
        attempt: attempt.attempt,
        step_index: attempt.step_index,
        program: attempt.plan.program.clone(),
        exit_code: attempt.exit_code,
        stdout: attempt.stdout.clone(),
        stderr: attempt.stderr.clone(),
        error: attempt.error.clone(),
    }
}

fn emit_state(app: &AppHandle, plan_id: &str, state: &str, message: Option<String>) {
    let _ = app.emit(
        "signalfit-state",
        StateEvent {
            plan_id: plan_id.to_owned(),
            state: state.to_owned(),
            message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_orchestrator::{ExecutionEngine, ProcessExit, ProcessRunner, ToolPlan};

    fn params(preset_id: &str) -> PresetParams {
        PresetParams {
            input: "input.mov".into(),
            output: if preset_id == "prores-422" {
                "output.mov".into()
            } else {
                "output.mp4".into()
            },
            workdir: std::env::temp_dir(),
            width: None,
            height: None,
            crf: None,
            encoder: None,
            upscale_engine: None,
            scale: None,
            fps: None,
            loudnorm: Some(LoudnormMeasurement {
                input_i: -20.0,
                input_tp: -2.0,
                input_lra: 7.0,
                input_thresh: -30.0,
                target_offset: 0.5,
            }),
        }
    }

    #[test]
    fn preset_catalog_has_five_unique_ids() {
        let presets = preset_descriptors();
        assert_eq!(presets.len(), 5);
        let mut ids: Vec<_> = presets.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn every_preset_builds_a_validated_plan() {
        for preset in preset_descriptors() {
            let plan = build_preset_plan(preset.id, &params(preset.id)).unwrap();
            ai_orchestrator::validate_execution_plan(&plan).unwrap();
            assert!(!plan.steps.is_empty());
            assert!(plan
                .steps
                .iter()
                .all(|step| step.estimated_minutes.is_some()));
        }
    }

    #[test]
    fn unknown_preset_and_encoder_are_rejected() {
        assert!(build_preset_plan("not-a-preset", &params("h264-1080p")).is_err());
        let mut custom = params("h264-1080p");
        custom.encoder = Some("shell".into());
        assert!(build_preset_plan("h264-1080p", &custom).is_err());
    }

    #[derive(Default)]
    struct NoRunRunner;

    impl ProcessRunner for NoRunRunner {
        fn run(
            &mut self,
            _plan: &ToolPlan,
            _on_stdout: &mut dyn FnMut(&str),
            _on_stderr: &mut dyn FnMut(&str),
        ) -> ai_orchestrator::Result<ProcessExit> {
            panic!("an unapproved plan must not reach ProcessRunner")
        }
    }

    #[test]
    fn command_session_cannot_bypass_validation_and_approval() {
        let plan = build_preset_plan("h264-1080p", &params("h264-1080p")).unwrap();
        let mut unapproved = PlanSession::new(plan.clone());
        assert!(ExecutionEngine::default()
            .execute(&mut unapproved, &mut NoRunRunner)
            .is_err());

        let mut session = prepare_session(plan).unwrap();
        assert_eq!(session.state(), PlanState::AwaitingConfirmation);
        session.approve().unwrap();
        assert_eq!(session.state(), PlanState::Approved);
    }

    #[test]
    fn rejection_is_terminal_for_the_confirmation_flow() {
        let plan = build_preset_plan("prores-422", &params("prores-422")).unwrap();
        let mut session = prepare_session(plan).unwrap();
        session.reject().unwrap();
        assert_eq!(session.state(), PlanState::Cancelled);
        assert!(session.approve().is_err());
    }
}
