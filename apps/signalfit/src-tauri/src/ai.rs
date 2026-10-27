//! BYOK provider adapters for AI-assisted offline planning.

use crate::keystore::PlatformKeyStore;
use ai_orchestrator::{
    generate_plan, ExecutionPlan, KeyStore, LlmProvider, OrchestratorError, Result, ToolPlan,
};
use serde::Deserialize;
use std::path::PathBuf;

/// Provider selection accepted by the AI panel.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Deterministic local provider for demonstrations and development.
    Mock,
    /// OpenAI-compatible chat-completions endpoint (HTTP intentionally pending).
    Openai,
}

/// Provider settings and the confinement root used by generated plans.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderConfig {
    /// Provider implementation.
    pub kind: ProviderKind,
    /// OpenAI-compatible base URL, for example `https://api.openai.com/v1`.
    pub base_url: Option<String>,
    /// Remote model identifier.
    pub model: Option<String>,
    /// User-selected directory that all generated media paths must stay within.
    pub workdir: PathBuf,
}

/// Generates and validates a plan without sending media bytes anywhere.
pub fn generate(
    config: AiProviderConfig,
    ffprobe_metadata: &str,
    requirement: &str,
) -> Result<ExecutionPlan> {
    if requirement.trim().is_empty() {
        return Err(OrchestratorError::InvalidPlan("AI 需求描述不能为空".into()));
    }
    match config.kind {
        ProviderKind::Mock => {
            let mut provider = MockProvider::new(config.workdir);
            generate_plan(&mut provider, ffprobe_metadata, requirement)
        }
        ProviderKind::Openai => {
            let base_url = config.base_url.ok_or_else(|| {
                OrchestratorError::Llm("请填写 OpenAI 兼容接口的 Base URL".into())
            })?;
            let key = PlatformKeyStore::new()
                .get("openai")?
                .ok_or_else(|| OrchestratorError::Llm("请先在 Keychain 中保存 API Key".into()))?;
            let mut provider = OpenAiCompatibleProvider::new(
                base_url,
                config.model.unwrap_or_else(|| "gpt-4o-mini".into()),
                key,
            );
            generate_plan(&mut provider, ffprobe_metadata, requirement)
        }
    }
}

/// Local demo provider. It performs no network request and needs no API key.
pub struct MockProvider {
    workdir: PathBuf,
}

impl MockProvider {
    /// Creates a deterministic provider confined to one user workdir.
    pub fn new(workdir: PathBuf) -> Self {
        Self { workdir }
    }
}

impl LlmProvider for MockProvider {
    fn complete(&mut self, _system_prompt: &str, _user_payload: &str) -> Result<String> {
        let mut step = ToolPlan::new(
            "ffmpeg",
            vec![
                "-hide_banner".into(),
                "-nostdin".into(),
                "-y".into(),
                "-i".into(),
                "input.mp4".into(),
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                "18".into(),
                "-c:a".into(),
                "aac".into(),
                "-progress".into(),
                "pipe:1".into(),
                "output.mp4".into(),
            ],
            "Mock 本地演示：将 input.mp4 转为 H.264/AAC output.mp4",
            &self.workdir,
        );
        step.inputs.push(PathBuf::from("input.mp4"));
        step.outputs.push(PathBuf::from("output.mp4"));
        step.estimated_minutes = Some(5.0);
        serde_json::to_string(&ExecutionPlan::single(step))
            .map_err(|error| OrchestratorError::Serialization(error.to_string()))
    }
}

/// OpenAI-compatible provider boundary with secret material kept out of `Debug`.
///
/// HTTP implementation留白：在独立网络特性中接入 `reqwest`（或应用选定的 HTTP
/// 栈），向 `{base_url}/chat/completions` 发送 `model` 与 system/messages，
/// Authorization 仅写入请求头，响应只取 assistant text，再交给核心
/// `parse_plan_json` 做同一套白名单与路径校验。不得记录请求头、Key 或完整请求。
pub struct OpenAiCompatibleProvider {
    _base_url: String,
    _model: String,
    _api_key: String,
}

impl OpenAiCompatibleProvider {
    /// Stores request configuration in memory for the future HTTP adapter.
    pub fn new(base_url: String, model: String, api_key: String) -> Self {
        Self {
            _base_url: base_url,
            _model: model,
            _api_key: api_key,
        }
    }
}

impl LlmProvider for OpenAiCompatibleProvider {
    fn complete(&mut self, _system_prompt: &str, _user_payload: &str) -> Result<String> {
        Err(OrchestratorError::Llm(
            "OpenAI 兼容 HTTP provider 尚未接入；当前请使用 Mock 本地演示".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_provider_generates_a_valid_confined_plan_without_network() {
        let workdir = std::env::temp_dir();
        let plan = generate(
            AiProviderConfig {
                kind: ProviderKind::Mock,
                base_url: None,
                model: None,
                workdir,
            },
            "{\"streams\":[]}",
            "制作一个本地代理文件",
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].program, "ffmpeg");
        assert!(plan.steps[0].args.contains(&"pipe:1".into()));
    }

    #[test]
    fn empty_requirement_is_rejected_before_provider_use() {
        let error = generate(
            AiProviderConfig {
                kind: ProviderKind::Mock,
                base_url: None,
                model: None,
                workdir: std::env::temp_dir(),
            },
            "{}",
            "  ",
        )
        .unwrap_err();
        assert!(error.to_string().contains("需求描述不能为空"));
    }
}
