use crate::{validate_execution_plan, ExecutionLog, ExecutionPlan, OrchestratorError, Result};
use serde::{Deserialize, Serialize};

/// Lifecycle of a proposed execution plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanState {
    /// Initial, not yet validated state.
    Draft,
    /// Allowlist and path validation succeeded.
    Validated,
    /// The plan is displayed and waiting for a user decision.
    AwaitingConfirmation,
    /// The user explicitly approved this plan.
    Approved,
    /// An approved plan is being executed.
    Executing,
    /// Every step completed successfully.
    Done,
    /// Execution or repair exhausted its allowed attempts.
    Failed,
    /// The user rejected or cancelled the plan.
    Cancelled,
}

/// Stateful confirmation gate around an [`ExecutionPlan`].
///
/// Only the execution engine can move an approved session into `Executing`;
/// there is no public transition that bypasses `Validated`,
/// `AwaitingConfirmation`, and `Approved`.
#[derive(Debug)]
pub struct PlanSession {
    plan: ExecutionPlan,
    state: PlanState,
    pub(crate) log: ExecutionLog,
    last_error: Option<String>,
}

impl PlanSession {
    /// Creates a draft session. No process may run until the full confirmation
    /// sequence has completed.
    pub fn new(plan: ExecutionPlan) -> Self {
        Self {
            plan,
            state: PlanState::Draft,
            log: ExecutionLog::default(),
            last_error: None,
        }
    }

    /// Returns the current plan, including the latest validated retry plan.
    pub fn plan(&self) -> &ExecutionPlan {
        &self.plan
    }

    /// Returns the current lifecycle state.
    pub fn state(&self) -> PlanState {
        self.state
    }

    /// Returns all captured execution logs and structured progress.
    pub fn log(&self) -> &ExecutionLog {
        &self.log
    }

    /// Returns the most recent terminal error, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Validates the draft and moves it to `Validated`.
    pub fn validate(&mut self) -> Result<()> {
        self.require(PlanState::Draft, "validate")?;
        validate_execution_plan(&self.plan)?;
        self.state = PlanState::Validated;
        Ok(())
    }

    /// Marks a validated plan as displayed to the user for confirmation.
    pub fn request_confirmation(&mut self) -> Result<()> {
        self.require(PlanState::Validated, "request confirmation")?;
        self.state = PlanState::AwaitingConfirmation;
        Ok(())
    }

    /// Approves the exact validated plan currently awaiting confirmation.
    pub fn approve(&mut self) -> Result<()> {
        self.require(PlanState::AwaitingConfirmation, "approve")?;
        self.state = PlanState::Approved;
        Ok(())
    }

    /// Rejects a plan awaiting confirmation and moves it to `Cancelled`.
    pub fn reject(&mut self) -> Result<()> {
        self.require(PlanState::AwaitingConfirmation, "reject")?;
        self.state = PlanState::Cancelled;
        Ok(())
    }

    /// Cancels a plan before execution starts.
    ///
    /// Injected runners own OS process handles, so cooperative termination of
    /// an already-running subprocess remains an application responsibility.
    pub fn cancel(&mut self) -> Result<()> {
        match self.state {
            PlanState::Draft
            | PlanState::Validated
            | PlanState::AwaitingConfirmation
            | PlanState::Approved => {
                self.state = PlanState::Cancelled;
                Ok(())
            }
            state => Err(OrchestratorError::InvalidState {
                state,
                action: "cancel",
            }),
        }
    }

    pub(crate) fn begin_execution(&mut self) -> Result<()> {
        self.require(PlanState::Approved, "execute")?;
        self.state = PlanState::Executing;
        self.last_error = None;
        Ok(())
    }

    pub(crate) fn finish(&mut self, plan: ExecutionPlan, log: ExecutionLog) {
        self.plan = plan;
        self.log = log;
        self.state = PlanState::Done;
        self.last_error = None;
    }

    pub(crate) fn fail(&mut self, plan: ExecutionPlan, log: ExecutionLog, error: &str) {
        self.plan = plan;
        self.log = log;
        self.state = PlanState::Failed;
        self.last_error = Some(error.to_owned());
    }

    fn require(&self, expected: PlanState, action: &'static str) -> Result<()> {
        if self.state == expected {
            Ok(())
        } else {
            Err(OrchestratorError::InvalidState {
                state: self.state,
                action,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolPlan;

    fn session() -> PlanSession {
        PlanSession::new(ExecutionPlan::single(ToolPlan::new(
            "ffprobe",
            vec!["-show_streams".into(), "input.mp4".into()],
            "Inspect input",
            "/work",
        )))
    }

    #[test]
    fn follows_confirmation_sequence() {
        let mut session = session();
        assert_eq!(session.state(), PlanState::Draft);
        session.approve().unwrap_err();
        session.validate().unwrap();
        assert_eq!(session.state(), PlanState::Validated);
        session.request_confirmation().unwrap();
        session.approve().unwrap();
        assert_eq!(session.state(), PlanState::Approved);
    }

    #[test]
    fn reject_moves_to_cancelled() {
        let mut session = session();
        session.validate().unwrap();
        session.request_confirmation().unwrap();
        session.reject().unwrap();
        assert_eq!(session.state(), PlanState::Cancelled);
    }
}
