use crate::{
    parse_progress_line, repair_plan, ExecutionPlan, FfmpegProgress, LlmProvider,
    OrchestratorError, PlanSession, ProcessRunner, Result, RetryContext, ToolPlan,
};

/// Default total number of execution attempts, including the initial run.
pub const DEFAULT_MAX_ATTEMPTS: usize = 2;
/// Default number of stderr lines copied into retry diagnostics.
pub const DEFAULT_STDERR_TAIL_LINES: usize = 80;

/// Structured ffmpeg progress associated with a step and attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressEvent {
    /// One-based execution attempt.
    pub attempt: usize,
    /// Zero-based plan step index.
    pub step_index: usize,
    /// Parsed progress update.
    pub progress: FfmpegProgress,
}

/// Complete output and diagnostics for one process invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptLog {
    /// One-based execution attempt.
    pub attempt: usize,
    /// Zero-based plan step index.
    pub step_index: usize,
    /// Exact validated plan passed to the runner.
    pub plan: ToolPlan,
    /// Complete captured standard-output lines.
    pub stdout: Vec<String>,
    /// Complete captured standard-error lines.
    pub stderr: Vec<String>,
    /// Exit code, or `None` if the runner failed before returning one.
    pub exit_code: Option<i32>,
    /// Human-readable process failure, if any.
    pub error: Option<String>,
}

impl AttemptLog {
    /// Returns the last `maximum` stderr lines while retaining the full log.
    pub fn stderr_tail(&self, maximum: usize) -> Vec<String> {
        tail_lines(&self.stderr, maximum)
    }
}

/// All attempts and progress events for an execution session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExecutionLog {
    /// Per-process logs in execution order.
    pub attempts: Vec<AttemptLog>,
    /// Parsed ffmpeg progress in arrival order.
    pub progress: Vec<ProgressEvent>,
}

impl ExecutionLog {
    /// Exports complete stdout/stderr logs as human-readable plain text.
    ///
    /// The export includes every retained attempt, not only the stderr tail
    /// sent to the LLM during retry.
    pub fn export_text(&self) -> String {
        let mut output = String::new();
        for attempt in &self.attempts {
            output.push_str(&format!(
                "=== attempt {} step {}: {} ===\n",
                attempt.attempt, attempt.step_index, attempt.plan.program
            ));
            output.push_str(&format!("argv: {:?}\n", attempt.plan.args));
            output.push_str(&format!("exit_code: {:?}\n", attempt.exit_code));
            if let Some(error) = &attempt.error {
                output.push_str(&format!("error: {error}\n"));
            }
            output.push_str("--- stdout ---\n");
            for line in &attempt.stdout {
                output.push_str(line);
                output.push('\n');
            }
            output.push_str("--- stderr ---\n");
            for line in &attempt.stderr {
                output.push_str(line);
                output.push('\n');
            }
        }
        output
    }

    /// Returns whether no process output has been recorded.
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty() && self.progress.is_empty()
    }
}

/// Successful execution result with the final plan and complete logs.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionReport {
    /// Final plan, including any validated LLM repair that succeeded.
    pub plan: ExecutionPlan,
    /// Complete execution history.
    pub log: ExecutionLog,
}

/// Coordinates confirmed plans, injected processes, progress, and retries.
#[derive(Debug, Clone)]
pub struct ExecutionEngine {
    max_attempts: usize,
    stderr_tail_lines: usize,
}

impl ExecutionEngine {
    /// Creates an engine with a total attempt limit of at least one.
    pub fn new(max_attempts: usize) -> Result<Self> {
        if max_attempts == 0 {
            return Err(OrchestratorError::InvalidRetryLimit);
        }
        Ok(Self {
            max_attempts,
            stderr_tail_lines: DEFAULT_STDERR_TAIL_LINES,
        })
    }

    /// Sets how many trailing stderr lines are included in retry contexts.
    /// Full logs are always retained regardless of this setting.
    pub fn with_stderr_tail_lines(mut self, maximum: usize) -> Self {
        self.stderr_tail_lines = maximum;
        self
    }

    /// Executes an approved plan once without LLM repair.
    pub fn execute(
        &self,
        session: &mut PlanSession,
        runner: &mut dyn ProcessRunner,
    ) -> Result<ExecutionReport> {
        session.begin_execution()?;
        let plan = session.plan().clone();
        let mut log = ExecutionLog::default();
        match self.run_plan(&plan, 1, runner, &mut log) {
            Ok(()) => {
                session.finish(plan.clone(), log.clone());
                Ok(ExecutionReport { plan, log })
            }
            Err(failure) => {
                session.fail(plan, log, &failure.error.to_string());
                Err(failure.error)
            }
        }
    }

    /// Executes an approved plan with bounded LLM-assisted repair.
    ///
    /// `max_attempts` includes the initial run. Every repair response is parsed
    /// and subjected to the same allowlist and path validation as the original
    /// plan. The session remains in `Executing` between attempts and can never
    /// reach this method without prior user approval.
    pub fn execute_with_retry(
        &self,
        session: &mut PlanSession,
        runner: &mut dyn ProcessRunner,
        provider: &mut dyn LlmProvider,
    ) -> Result<ExecutionReport> {
        session.begin_execution()?;
        let mut plan = session.plan().clone();
        let mut log = ExecutionLog::default();
        let mut attempt = 1;

        loop {
            match self.run_plan(&plan, attempt, runner, &mut log) {
                Ok(()) => {
                    session.finish(plan.clone(), log.clone());
                    return Ok(ExecutionReport { plan, log });
                }
                Err(failure) if attempt >= self.max_attempts => {
                    session.fail(plan, log, &failure.error.to_string());
                    return Err(failure.error);
                }
                Err(failure) => {
                    let context = RetryContext {
                        plan: plan.clone(),
                        stderr_tail: failure.stderr_tail,
                        attempt,
                        failed_step_index: failure.failed_step_index,
                    };
                    match repair_plan(provider, &context) {
                        Ok(repaired) => {
                            plan = repaired;
                            attempt += 1;
                        }
                        Err(error) => {
                            session.fail(plan, log, &error.to_string());
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    fn run_plan(
        &self,
        plan: &ExecutionPlan,
        attempt: usize,
        runner: &mut dyn ProcessRunner,
        log: &mut ExecutionLog,
    ) -> std::result::Result<(), StepFailure> {
        for (step_index, step) in plan.steps.iter().enumerate() {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut step_progress = Vec::new();
            let result = {
                let mut on_stdout = |line: &str| {
                    stdout.push(line.to_owned());
                    if let Ok(Some(progress)) = parse_progress_line(line) {
                        step_progress.push(ProgressEvent {
                            attempt,
                            step_index,
                            progress,
                        });
                    }
                };
                let mut on_stderr = |line: &str| stderr.push(line.to_owned());
                runner.run(step, &mut on_stdout, &mut on_stderr)
            };

            let stderr_tail = tail_lines(&stderr, self.stderr_tail_lines);
            let (exit_code, failure) = match result {
                Ok(exit) if exit.success() => (Some(exit.code), None),
                Ok(exit) => (
                    Some(exit.code),
                    Some(OrchestratorError::ProcessFailed {
                        code: exit.code,
                        stderr_tail: stderr_tail.join("\n"),
                    }),
                ),
                Err(error) => (None, Some(error)),
            };
            let error_text = failure.as_ref().map(ToString::to_string);
            log.attempts.push(AttemptLog {
                attempt,
                step_index,
                plan: step.clone(),
                stdout,
                stderr,
                exit_code,
                error: error_text,
            });
            log.progress.extend(step_progress);

            if let Some(error) = failure {
                return Err(StepFailure {
                    error,
                    failed_step_index: step_index,
                    stderr_tail,
                });
            }
        }
        Ok(())
    }
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            stderr_tail_lines: DEFAULT_STDERR_TAIL_LINES,
        }
    }
}

#[derive(Debug)]
struct StepFailure {
    error: OrchestratorError,
    failed_step_index: usize,
    stderr_tail: Vec<String>,
}

fn tail_lines(lines: &[String], maximum: usize) -> Vec<String> {
    lines[lines.len().saturating_sub(maximum)..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockProvider;
    use crate::{ExecutionPlan, PlanState, ProcessExit};
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct MockRun {
        code: i32,
        stdout: Vec<String>,
        stderr: Vec<String>,
    }

    impl MockRun {
        fn new(code: i32, stdout: &[&str], stderr: &[&str]) -> Self {
            Self {
                code,
                stdout: stdout.iter().map(|line| (*line).to_owned()).collect(),
                stderr: stderr.iter().map(|line| (*line).to_owned()).collect(),
            }
        }
    }

    #[derive(Default)]
    struct MockRunner {
        runs: VecDeque<MockRun>,
        calls: Vec<ToolPlan>,
    }

    impl MockRunner {
        fn with_runs(runs: Vec<MockRun>) -> Self {
            Self {
                runs: runs.into_iter().collect(),
                calls: Vec::new(),
            }
        }
    }

    impl ProcessRunner for MockRunner {
        fn run(
            &mut self,
            plan: &ToolPlan,
            on_stdout: &mut dyn FnMut(&str),
            on_stderr: &mut dyn FnMut(&str),
        ) -> Result<ProcessExit> {
            self.calls.push(plan.clone());
            let run = self
                .runs
                .pop_front()
                .ok_or_else(|| OrchestratorError::Process("mock has no run configured".into()))?;
            for line in &run.stdout {
                on_stdout(line);
            }
            for line in &run.stderr {
                on_stderr(line);
            }
            Ok(ProcessExit { code: run.code })
        }
    }

    fn plan() -> ExecutionPlan {
        ExecutionPlan::single(ToolPlan::new(
            "ffmpeg",
            vec![
                "-i".into(),
                "input.mp4".into(),
                "-progress".into(),
                "pipe:1".into(),
                "output.mp4".into(),
            ],
            "Convert locally",
            "/work",
        ))
    }

    fn approved_session() -> PlanSession {
        let mut session = PlanSession::new(plan());
        session.validate().unwrap();
        session.request_confirmation().unwrap();
        session.approve().unwrap();
        session
    }

    #[test]
    fn unapproved_plan_cannot_execute() {
        let mut session = PlanSession::new(plan());
        let mut runner = MockRunner::default();
        let error = ExecutionEngine::default()
            .execute(&mut session, &mut runner)
            .unwrap_err();
        assert!(matches!(
            error,
            OrchestratorError::InvalidState {
                state: PlanState::Draft,
                ..
            }
        ));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn captures_structured_progress_and_completes() {
        let mut session = approved_session();
        let mut runner = MockRunner::with_runs(vec![MockRun::new(
            0,
            &[
                "frame=24",
                "fps=12.5",
                "out_time_ms=2000000",
                "progress=end",
            ],
            &[],
        )]);
        let report = ExecutionEngine::default()
            .execute(&mut session, &mut runner)
            .unwrap();
        assert_eq!(session.state(), PlanState::Done);
        assert_eq!(report.log.progress.len(), 4);
        assert_eq!(report.log.progress[0].progress.frame, Some(24));
        assert_eq!(report.log.attempts.len(), 1);
    }

    #[test]
    fn retry_limit_is_total_attempts_and_failed_logs_export() {
        let mut session = approved_session();
        let mut runner = MockRunner::with_runs(vec![
            MockRun::new(1, &[], &["first failure"]),
            MockRun::new(2, &[], &["second failure"]),
        ]);
        let repaired = serde_json::to_string(&plan()).unwrap();
        let mut provider = MockProvider::new(vec![Ok(repaired)]);
        let error = ExecutionEngine::new(2)
            .unwrap()
            .execute_with_retry(&mut session, &mut runner, &mut provider)
            .unwrap_err();

        assert!(matches!(
            error,
            OrchestratorError::ProcessFailed { code: 2, .. }
        ));
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(provider.calls.len(), 1);
        assert_eq!(session.state(), PlanState::Failed);
        assert_eq!(session.log().attempts.len(), 2);
        let exported = session.log().export_text();
        assert!(exported.contains("first failure"));
        assert!(exported.contains("second failure"));
    }

    #[test]
    fn repaired_plan_is_revalidated_and_can_succeed() {
        let mut session = approved_session();
        let mut runner = MockRunner::with_runs(vec![
            MockRun::new(1, &[], &["bad option"]),
            MockRun::new(0, &["progress=end"], &[]),
        ]);
        let repaired = serde_json::to_string(&plan()).unwrap();
        let mut provider = MockProvider::new(vec![Ok(repaired)]);
        let report = ExecutionEngine::default()
            .execute_with_retry(&mut session, &mut runner, &mut provider)
            .unwrap();
        assert_eq!(session.state(), PlanState::Done);
        assert_eq!(report.log.attempts.len(), 2);
        assert_eq!(provider.calls.len(), 1);
        let retry_payload = &provider.calls[0].1;
        assert!(retry_payload.contains("bad option"));
        assert!(retry_payload.contains("\"attempt\": 1"));
    }

    #[test]
    fn stderr_tail_is_bounded_but_full_log_is_retained() {
        let mut session = approved_session();
        let mut runner = MockRunner::with_runs(vec![MockRun::new(
            9,
            &[],
            &["line one", "line two", "line three"],
        )]);
        let error = ExecutionEngine::new(1)
            .unwrap()
            .with_stderr_tail_lines(2)
            .execute(&mut session, &mut runner)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("line one"));
        assert!(error.contains("line two"));
        assert!(error.contains("line three"));
        assert!(session.log().export_text().contains("line one"));
    }

    #[test]
    fn zero_retry_limit_is_rejected() {
        assert!(matches!(
            ExecutionEngine::new(0),
            Err(OrchestratorError::InvalidRetryLimit)
        ));
    }
}
