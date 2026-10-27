use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One declarative invocation of an allowlisted local media tool.
///
/// `program` and each element of `args` are passed as separate argv values by
/// the application-provided runner. They are never joined into a shell command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolPlan {
    /// Executable name; must be one of [`crate::ALLOWED_PROGRAMS`].
    pub program: String,
    /// Argument vector excluding `argv[0]`.
    pub args: Vec<String>,
    /// Natural-language explanation shown to the user before approval.
    pub explanation: String,
    /// Optional user-facing duration estimate.
    pub estimated_minutes: Option<f64>,
    /// Directory that confines every input, output, and path-like argument.
    pub workdir: PathBuf,
    /// Declared local inputs, used in addition to argv inspection.
    #[serde(default)]
    pub inputs: Vec<PathBuf>,
    /// Declared local outputs, used in addition to argv inspection.
    #[serde(default)]
    pub outputs: Vec<PathBuf>,
}

impl ToolPlan {
    /// Creates a single-step plan with no declared inputs or outputs.
    ///
    /// Validation is deliberately separate so drafts can be constructed before
    /// all application-specific information is available.
    pub fn new(
        program: impl Into<String>,
        args: Vec<String>,
        explanation: impl Into<String>,
        workdir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            program: program.into(),
            args,
            explanation: explanation.into(),
            estimated_minutes: None,
            workdir: workdir.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }
}

/// An ordered collection of local tool invocations.
///
/// Multi-step operations such as frame extraction, per-frame upscaling, and
/// reassembly are represented as separate [`ToolPlan`] values in `steps`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Steps executed sequentially in array order.
    pub steps: Vec<ToolPlan>,
}

impl ExecutionPlan {
    /// Creates an ordered plan. Call [`crate::validate_execution_plan`] before
    /// presenting it for approval or executing it.
    pub fn new(steps: Vec<ToolPlan>) -> Self {
        Self { steps }
    }

    /// Creates a plan containing one tool invocation.
    pub fn single(step: ToolPlan) -> Self {
        Self { steps: vec![step] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_plan_round_trips_through_serde() {
        let plan = ExecutionPlan::single(ToolPlan::new(
            "ffprobe",
            vec!["-show_streams".into(), "input.mp4".into()],
            "Inspect the input",
            "/work",
        ));
        let json = serde_json::to_string(&plan).unwrap();
        let decoded: ExecutionPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, plan);
        assert!(decoded.steps[0].inputs.is_empty());
        assert!(decoded.steps[0].outputs.is_empty());
    }
}
