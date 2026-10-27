use crate::{identify_platform, DownloadError, DownloadProgress, DownloadResult, Platform};

/// Lifecycle states of a media download job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobState {
    /// Created but not yet resolving.
    Queued,
    /// Selecting a resolver and constructing a plan.
    Resolving,
    /// Executing the selected download plan.
    Downloading,
    /// Download finished successfully.
    Completed,
    /// Resolution or download failed.
    Failed,
    /// Job was cancelled before completion.
    Cancelled,
}

/// Events emitted by jobs and resolver orchestration.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    /// A job changed lifecycle state.
    StateChanged {
        /// Identifier of the changed job.
        job_id: String,
        /// Previous state.
        from: JobState,
        /// New state.
        to: JobState,
    },
    /// A job received structured download progress.
    Progress {
        /// Identifier of the job making progress.
        job_id: String,
        /// Parsed progress value.
        progress: DownloadProgress,
    },
    /// A plugin resolver returned an error, timed out, or produced bad data.
    PluginFailed {
        /// Identifier of the failed plugin.
        plugin_id: String,
        /// Human-readable failure reason.
        error: String,
    },
    /// The registry fell back from a failed plugin to its default resolver.
    FallbackTriggered {
        /// Identifier of the plugin that caused fallback.
        plugin_id: String,
        /// Identifier of the fallback resolver.
        fallback_id: String,
    },
}

/// A download job and its observable event history.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadJob {
    /// Caller-provided stable job identifier.
    pub id: String,
    /// Validated source URL.
    pub url: String,
    /// Platform identified from the source URL.
    pub platform: Platform,
    /// Resolver that produced the current plan, once resolution succeeds.
    pub resolver_used: Option<String>,
    /// Current lifecycle state.
    pub state: JobState,
    /// Latest structured progress.
    pub progress: DownloadProgress,
    /// Failure message for a failed job.
    pub error: Option<String>,
    events: Vec<DownloadEvent>,
}

impl DownloadJob {
    /// Creates a queued job after validating the URL and identifying its host.
    pub fn new(id: impl Into<String>, url: impl Into<String>) -> DownloadResult<Self> {
        let id = id.into();
        let url = url.into();
        if id.is_empty() || id.chars().any(char::is_control) {
            return Err(DownloadError::Resolver(
                "job id must be non-empty and contain no control characters".into(),
            ));
        }
        let platform = identify_platform(&url)?;
        Ok(Self {
            id,
            url,
            platform,
            resolver_used: None,
            state: JobState::Queued,
            progress: DownloadProgress::default(),
            error: None,
            events: Vec::new(),
        })
    }

    /// Moves a queued job into resolution.
    pub fn begin_resolving(&mut self) -> DownloadResult<()> {
        self.require_state(JobState::Queued, "begin resolving")?;
        self.transition(JobState::Resolving);
        Ok(())
    }

    /// Records the selected resolver after successful resolution.
    pub fn set_resolver_used(&mut self, resolver_id: impl Into<String>) -> DownloadResult<()> {
        self.require_state(JobState::Resolving, "set resolver")?;
        let resolver_id = resolver_id.into();
        if resolver_id.is_empty() {
            return Err(DownloadError::Resolver("resolver id is empty".into()));
        }
        self.resolver_used = Some(resolver_id);
        Ok(())
    }

    /// Moves a resolved job into process execution.
    pub fn begin_downloading(&mut self) -> DownloadResult<()> {
        self.require_state(JobState::Resolving, "begin downloading")?;
        if self.resolver_used.is_none() {
            return Err(DownloadError::InvalidState {
                state: self.state,
                action: "begin downloading before selecting a resolver",
            });
        }
        self.transition(JobState::Downloading);
        Ok(())
    }

    /// Updates progress and emits a [`DownloadEvent::Progress`] event.
    pub fn update_progress(&mut self, progress: DownloadProgress) -> DownloadResult<()> {
        self.require_state(JobState::Downloading, "update progress")?;
        progress.validate()?;
        self.progress = progress.clone();
        self.events.push(DownloadEvent::Progress {
            job_id: self.id.clone(),
            progress,
        });
        Ok(())
    }

    /// Marks a downloading job as completed and sets progress to 100 percent.
    pub fn complete(&mut self) -> DownloadResult<()> {
        self.require_state(JobState::Downloading, "complete")?;
        self.progress.percent = 100.0;
        self.progress.eta_seconds = Some(0);
        self.transition(JobState::Completed);
        Ok(())
    }

    /// Marks a resolving or downloading job as failed.
    pub fn fail(&mut self, error: impl Into<String>) -> DownloadResult<()> {
        if !matches!(self.state, JobState::Resolving | JobState::Downloading) {
            return Err(DownloadError::InvalidState {
                state: self.state,
                action: "fail",
            });
        }
        self.error = Some(error.into());
        self.transition(JobState::Failed);
        Ok(())
    }

    /// Cancels a queued, resolving, or downloading job.
    pub fn cancel(&mut self) -> DownloadResult<()> {
        if matches!(
            self.state,
            JobState::Completed | JobState::Failed | JobState::Cancelled
        ) {
            return Err(DownloadError::InvalidState {
                state: self.state,
                action: "cancel",
            });
        }
        self.transition(JobState::Cancelled);
        Ok(())
    }

    /// Returns all events emitted so far.
    pub fn events(&self) -> &[DownloadEvent] {
        &self.events
    }

    /// Drains and returns all pending events.
    pub fn take_events(&mut self) -> Vec<DownloadEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn record_events(&mut self, events: Vec<DownloadEvent>) {
        self.events.extend(events);
    }

    fn require_state(&self, expected: JobState, action: &'static str) -> DownloadResult<()> {
        if self.state != expected {
            return Err(DownloadError::InvalidState {
                state: self.state,
                action,
            });
        }
        Ok(())
    }

    fn transition(&mut self, to: JobState) {
        let from = self.state;
        self.state = to;
        self.events.push(DownloadEvent::StateChanged {
            job_id: self.id.clone(),
            from,
            to,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_through_the_happy_path_state_machine() {
        let mut job = DownloadJob::new("job-1", "https://youtu.be/abc").unwrap();
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.platform, Platform::YouTube);

        job.begin_resolving().unwrap();
        job.set_resolver_used("yt-dlp").unwrap();
        job.begin_downloading().unwrap();
        job.update_progress(DownloadProgress::new(45.0).unwrap())
            .unwrap();
        job.complete().unwrap();

        assert_eq!(job.state, JobState::Completed);
        assert_eq!(job.progress.percent, 100.0);
        assert_eq!(job.resolver_used.as_deref(), Some("yt-dlp"));
        assert!(job.events().iter().any(|event| matches!(
            event,
            DownloadEvent::Progress { progress, .. } if progress.percent == 45.0
        )));
        assert_eq!(
            job.events()
                .iter()
                .filter(|event| matches!(event, DownloadEvent::StateChanged { .. }))
                .count(),
            3
        );
    }

    #[test]
    fn rejects_invalid_state_transitions() {
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        assert!(job.begin_downloading().is_err());
        job.begin_resolving().unwrap();
        assert!(job.begin_resolving().is_err());
        assert!(job.begin_downloading().is_err());
        job.set_resolver_used("yt-dlp").unwrap();
        job.begin_downloading().unwrap();
        job.complete().unwrap();
        assert!(job.complete().is_err());
        assert!(job.cancel().is_err());
    }

    #[test]
    fn cancellation_is_allowed_from_active_states() {
        for target in [JobState::Queued, JobState::Resolving, JobState::Downloading] {
            let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
            if target != JobState::Queued {
                job.begin_resolving().unwrap();
            }
            if target == JobState::Downloading {
                job.set_resolver_used("yt-dlp").unwrap();
                job.begin_downloading().unwrap();
            }
            job.cancel().unwrap();
            assert_eq!(job.state, JobState::Cancelled);
        }
    }

    #[test]
    fn failure_is_recorded() {
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        job.begin_resolving().unwrap();
        job.fail("network unavailable").unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert_eq!(job.error.as_deref(), Some("network unavailable"));
    }

    #[test]
    fn take_events_drains_the_event_stream() {
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        job.begin_resolving().unwrap();
        assert_eq!(job.take_events().len(), 1);
        assert!(job.events().is_empty());
    }
}
