use crate::{
    parse_progress_line, DownloadError, DownloadJob, DownloadPlan, DownloadResult, ProcessRunner,
    ResolveContext, ResolvePlan, ResolverRegistry,
};

/// Coordinates job state, resolver selection, and injected process execution.
pub struct DownloadEngine {
    registry: ResolverRegistry,
}

impl DownloadEngine {
    /// Creates an engine using `registry` for resolution.
    pub fn new(registry: ResolverRegistry) -> Self {
        Self { registry }
    }

    /// Returns the underlying resolver registry.
    pub fn registry(&self) -> &ResolverRegistry {
        &self.registry
    }

    /// Returns the mutable resolver registry for plugin administration.
    pub fn registry_mut(&mut self) -> &mut ResolverRegistry {
        &mut self.registry
    }

    /// Resolves a queued job and records its selected resolver and events.
    pub fn resolve(
        &self,
        job: &mut DownloadJob,
        ctx: &ResolveContext,
    ) -> DownloadResult<ResolvePlan> {
        job.begin_resolving()?;
        match self.registry.resolve(&job.url, ctx) {
            Ok(resolution) => {
                job.set_resolver_used(resolution.resolver_used)?;
                job.record_events(resolution.events);
                Ok(resolution.plan)
            }
            Err(error) => {
                job.fail(error.to_string())?;
                Err(error)
            }
        }
    }

    /// Executes a download plan through an application-provided runner.
    ///
    /// Simulation plans never invoke the runner. Runner output is scanned for
    /// yt-dlp progress lines and appended to the job's event stream.
    pub fn download(
        &self,
        job: &mut DownloadJob,
        plan: &DownloadPlan,
        runner: &mut dyn ProcessRunner,
    ) -> DownloadResult<()> {
        job.begin_downloading()?;
        if plan.simulate {
            return job.complete();
        }

        let exit = {
            let mut on_line = |line: &str| {
                if let Some(progress) = parse_progress_line(line) {
                    // The job is downloading for the entire runner call.
                    let _ = job.update_progress(progress);
                }
            };
            runner.run(plan, &mut on_line)
        };

        match exit {
            Ok(exit) if exit.success() => job.complete(),
            Ok(exit) => {
                let error = DownloadError::ProcessFailed(exit.code);
                job.fail(error.to_string())?;
                Err(error)
            }
            Err(error) => {
                job.fail(error.to_string())?;
                Err(error)
            }
        }
    }
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new(ResolverRegistry::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DownloadEvent, JobState, ProcessExit};

    struct MockProcessRunner {
        lines: Vec<String>,
        exit: DownloadResult<ProcessExit>,
        calls: usize,
        last_plan: Option<DownloadPlan>,
    }

    impl MockProcessRunner {
        fn succeeding(lines: &[&str]) -> Self {
            Self {
                lines: lines.iter().map(|line| (*line).to_owned()).collect(),
                exit: Ok(ProcessExit { code: 0 }),
                calls: 0,
                last_plan: None,
            }
        }
    }

    impl ProcessRunner for MockProcessRunner {
        fn run(
            &mut self,
            plan: &DownloadPlan,
            on_line: &mut dyn FnMut(&str),
        ) -> DownloadResult<ProcessExit> {
            self.calls += 1;
            self.last_plan = Some(plan.clone());
            for line in &self.lines {
                on_line(line);
            }
            self.exit.clone()
        }
    }

    #[test]
    fn resolves_downloads_and_emits_progress() {
        let engine = DownloadEngine::default();
        let mut job = DownloadJob::new("job-1", "https://youtu.be/abc").unwrap();
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        let plan = match engine.resolve(&mut job, &ctx).unwrap() {
            ResolvePlan::Download(plan) => plan,
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        };
        let mut runner = MockProcessRunner::succeeding(&[
            "[download] 10.0% of 2.00MiB at 1.00MiB/s ETA 00:02",
            "[download] 100%",
        ]);

        engine.download(&mut job, &plan, &mut runner).unwrap();
        assert_eq!(job.state, JobState::Completed);
        assert_eq!(runner.calls, 1);
        assert_eq!(runner.last_plan, Some(plan));
        assert!(job.events().iter().any(|event| matches!(
            event,
            DownloadEvent::Progress { progress, .. } if progress.percent == 10.0
        )));
    }

    #[test]
    fn simulation_never_calls_process_runner() {
        let engine = DownloadEngine::default();
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s").with_simulation(true);
        let plan = match engine.resolve(&mut job, &ctx).unwrap() {
            ResolvePlan::Download(plan) => plan,
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        };
        let mut runner = MockProcessRunner::succeeding(&[]);
        engine.download(&mut job, &plan, &mut runner).unwrap();
        assert_eq!(runner.calls, 0);
        assert_eq!(job.state, JobState::Completed);
    }

    #[test]
    fn nonzero_exit_marks_job_failed() {
        let engine = DownloadEngine::default();
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        let plan = match engine.resolve(&mut job, &ctx).unwrap() {
            ResolvePlan::Download(plan) => plan,
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        };
        let mut runner = MockProcessRunner {
            lines: Vec::new(),
            exit: Ok(ProcessExit { code: 7 }),
            calls: 0,
            last_plan: None,
        };
        assert!(matches!(
            engine.download(&mut job, &plan, &mut runner),
            Err(DownloadError::ProcessFailed(7))
        ));
        assert_eq!(job.state, JobState::Failed);
    }

    #[test]
    fn runner_error_marks_job_failed() {
        let engine = DownloadEngine::default();
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        let plan = match engine.resolve(&mut job, &ctx).unwrap() {
            ResolvePlan::Download(plan) => plan,
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        };
        let mut runner = MockProcessRunner {
            lines: Vec::new(),
            exit: Err(DownloadError::Process("spawn failed".into())),
            calls: 0,
            last_plan: None,
        };
        assert!(engine.download(&mut job, &plan, &mut runner).is_err());
        assert_eq!(job.state, JobState::Failed);
        assert!(job.error.as_deref().unwrap().contains("spawn failed"));
    }

    #[test]
    fn resolution_error_marks_job_failed() {
        let engine = DownloadEngine::default();
        let mut job = DownloadJob::new("job-1", "https://example.com/v").unwrap();
        let ctx = ResolveContext::new("/tmp", "../escape");
        assert!(engine.resolve(&mut job, &ctx).is_err());
        assert_eq!(job.state, JobState::Failed);
    }
}
