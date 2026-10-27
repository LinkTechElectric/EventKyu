use crate::{
    validate_url, DownloadError, DownloadPlan, DownloadResult, MediaFormat, MediaResolver,
    Platform, ResolveContext, ResolvePlan,
};
use std::path::{Path, PathBuf};

/// Default resolver that constructs yt-dlp argv plans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtDlpResolver {
    program: PathBuf,
}

impl YtDlpResolver {
    /// Stable resolver identifier used by jobs and fallback events.
    pub const ID: &'static str = "yt-dlp";

    /// Creates a resolver that invokes `yt-dlp` from the process path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses a specific yt-dlp executable path.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// Returns the configured yt-dlp executable.
    pub fn program(&self) -> &Path {
        &self.program
    }
}

impl Default for YtDlpResolver {
    fn default() -> Self {
        Self {
            program: PathBuf::from("yt-dlp"),
        }
    }
}

impl MediaResolver for YtDlpResolver {
    fn id(&self) -> &str {
        Self::ID
    }

    fn supports(&self, _platform: Platform) -> bool {
        true
    }

    fn resolve(&self, url: &str, ctx: &ResolveContext) -> DownloadResult<ResolvePlan> {
        validate_url(url)?;
        let output_path = ctx.output_path()?;
        let output = output_path
            .to_str()
            .ok_or_else(|| DownloadError::InvalidOutputName("path is not valid UTF-8".into()))?
            .to_owned();

        let mut args = vec![
            "--newline".into(),
            "--format".into(),
            ctx.format.as_yt_dlp_selector().into(),
        ];
        if ctx.format == MediaFormat::AudioOnly {
            args.push("--extract-audio".into());
        }
        args.push("--output".into());
        args.push(output);
        if let Some(browser) = ctx.cookies_from_browser {
            args.push("--cookies-from-browser".into());
            args.push(browser.as_str().into());
        }
        if ctx.simulate {
            args.push("--simulate".into());
        }
        // End option parsing before the URL. No shell interprets these values.
        args.push("--".into());
        args.push(url.into());

        let plan = DownloadPlan {
            program: self.program.clone(),
            args,
            output_path,
            simulate: ctx.simulate,
        };
        Ok(ResolvePlan::Download(plan))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CookieBrowser;

    fn download_plan(ctx: &ResolveContext) -> DownloadPlan {
        let resolver = YtDlpResolver::new();
        match resolver
            .resolve("https://www.bilibili.com/video/BV1", ctx)
            .unwrap()
        {
            ResolvePlan::Download(plan) => plan,
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        }
    }

    #[test]
    fn constructs_best_video_argv_with_1080p_cap() {
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        let plan = download_plan(&ctx);
        assert_eq!(plan.program, PathBuf::from("yt-dlp"));
        assert_eq!(plan.args[0], "--newline");
        let format_index = plan.args.iter().position(|arg| arg == "--format").unwrap();
        assert_eq!(
            plan.args[format_index + 1],
            "bestvideo[height<=1080]+bestaudio/best[height<=1080]"
        );
        assert_eq!(
            plan.args.last().unwrap(),
            "https://www.bilibili.com/video/BV1"
        );
        assert_eq!(plan.args[plan.args.len() - 2], "--");
    }

    #[test]
    fn constructs_audio_only_argv() {
        let ctx = ResolveContext::new("/tmp", "audio.%(ext)s").with_format(MediaFormat::AudioOnly);
        let plan = download_plan(&ctx);
        let format_index = plan.args.iter().position(|arg| arg == "--format").unwrap();
        assert_eq!(plan.args[format_index + 1], "bestaudio/best");
        assert!(plan.args.iter().any(|arg| arg == "--extract-audio"));
    }

    #[test]
    fn injects_cookie_browser_as_two_arguments() {
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s")
            .with_cookies_from_browser(CookieBrowser::Firefox);
        let plan = download_plan(&ctx);
        let index = plan
            .args
            .iter()
            .position(|arg| arg == "--cookies-from-browser")
            .unwrap();
        assert_eq!(plan.args[index + 1], "firefox");
    }

    #[test]
    fn confines_output_template_to_work_dir() {
        let ctx = ResolveContext::new("/tmp", "show/video.%(ext)s");
        let plan = download_plan(&ctx);
        let index = plan.args.iter().position(|arg| arg == "--output").unwrap();
        let output = PathBuf::from(&plan.args[index + 1]);
        assert!(output.starts_with(std::fs::canonicalize("/tmp").unwrap()));
        assert!(output.ends_with("show/video.%(ext)s"));
    }

    #[test]
    fn rejects_output_traversal() {
        let ctx = ResolveContext::new("/tmp", "../escape.%(ext)s");
        let resolver = YtDlpResolver::new();
        assert!(matches!(
            resolver.resolve("https://youtu.be/abc", &ctx),
            Err(DownloadError::InvalidOutputName(_))
        ));
    }

    #[test]
    fn rejects_invalid_url_before_constructing_argv() {
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        let resolver = YtDlpResolver::new();
        for url in ["file:///tmp/video", "https://example.com/a\n--exec"] {
            assert!(matches!(
                resolver.resolve(url, &ctx),
                Err(DownloadError::InvalidUrl(_))
            ));
        }
    }

    #[test]
    fn simulation_marks_plan_and_adds_simulate_flag() {
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s").with_simulation(true);
        let plan = download_plan(&ctx);
        assert!(plan.simulate);
        assert!(plan.args.iter().any(|arg| arg == "--simulate"));
    }

    #[test]
    fn custom_program_is_preserved() {
        let resolver = YtDlpResolver::with_program("/opt/eventkyu/yt-dlp");
        let ctx = ResolveContext::new("/tmp", "video.%(ext)s");
        match resolver.resolve("https://example.com/v", &ctx).unwrap() {
            ResolvePlan::Download(plan) => {
                assert_eq!(plan.program, PathBuf::from("/opt/eventkyu/yt-dlp"));
            }
            ResolvePlan::DirectStream(_) => panic!("expected download plan"),
        }
    }
}
