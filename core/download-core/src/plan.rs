use crate::{validate_url, DownloadError, DownloadResult};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

/// Media format requested from yt-dlp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaFormat {
    /// Best video and audio, with video height capped at 1080 pixels.
    #[default]
    BestVideo,
    /// Best audio-only stream.
    AudioOnly,
}

impl MediaFormat {
    /// Returns the yt-dlp format selector for this request.
    pub fn as_yt_dlp_selector(self) -> &'static str {
        match self {
            Self::BestVideo => "bestvideo[height<=1080]+bestaudio/best[height<=1080]",
            Self::AudioOnly => "bestaudio/best",
        }
    }
}

/// Browsers from which yt-dlp may import cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookieBrowser {
    /// Google Chrome.
    Chrome,
    /// Mozilla Firefox.
    Firefox,
    /// Apple Safari.
    Safari,
    /// Microsoft Edge.
    Edge,
    /// Brave.
    Brave,
}

impl CookieBrowser {
    /// Returns the exact yt-dlp browser argument.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Firefox => "firefox",
            Self::Safari => "safari",
            Self::Edge => "edge",
            Self::Brave => "brave",
        }
    }

    /// Returns every supported browser.
    pub const fn all() -> &'static [Self] {
        &[
            Self::Chrome,
            Self::Firefox,
            Self::Safari,
            Self::Edge,
            Self::Brave,
        ]
    }
}

impl FromStr for CookieBrowser {
    type Err = DownloadError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chrome" => Ok(Self::Chrome),
            "firefox" => Ok(Self::Firefox),
            "safari" => Ok(Self::Safari),
            "edge" => Ok(Self::Edge),
            "brave" => Ok(Self::Brave),
            other => Err(DownloadError::InvalidBrowser(other.to_owned())),
        }
    }
}

/// An HTTP header required to play a direct stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpHeader {
    /// Header field name.
    pub name: String,
    /// Header field value.
    pub value: String,
}

/// A cookie required to play a direct stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCookie {
    /// Cookie name.
    pub name: String,
    /// Cookie value.
    pub value: String,
}

/// A directly playable media URL and its required request metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectStream {
    /// HTTP(S) media URL.
    pub url: String,
    /// Headers required by the origin server.
    pub headers: Vec<HttpHeader>,
    /// Cookies required by the origin server.
    pub cookies: Vec<StreamCookie>,
}

impl DirectStream {
    /// Creates a direct stream without additional headers or cookies.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: Vec::new(),
            cookies: Vec::new(),
        }
    }
}

/// An external tool invocation planned without shell interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPlan {
    /// Executable path or name.
    pub program: PathBuf,
    /// Arguments passed individually to [`Command::args`].
    pub args: Vec<String>,
    /// Expected output path.
    pub output_path: PathBuf,
    /// When true, callers must only inspect the plan and must not execute it.
    pub simulate: bool,
}

impl DownloadPlan {
    /// Builds a process command using an executable and an argv vector.
    ///
    /// No shell is involved, so shell metacharacters in arguments have no
    /// special meaning.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

/// Description of how an application can obtain media.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvePlan {
    /// Play or fetch this URL directly with the supplied request metadata.
    DirectStream(DirectStream),
    /// Run this tool argv plan to download the media.
    Download(DownloadPlan),
}

impl ResolvePlan {
    /// Validates URLs, request metadata, and process-plan invariants.
    pub fn validate(&self) -> DownloadResult<()> {
        match self {
            Self::DirectStream(stream) => {
                validate_url(&stream.url)?;
                for header in &stream.headers {
                    if !valid_header_name(&header.name) {
                        return Err(DownloadError::InvalidPlan(format!(
                            "invalid HTTP header name: {:?}",
                            header.name
                        )));
                    }
                    validate_metadata_value(&header.value, "HTTP header")?;
                }
                for cookie in &stream.cookies {
                    if !valid_header_name(&cookie.name) {
                        return Err(DownloadError::InvalidPlan(format!(
                            "invalid cookie name: {:?}",
                            cookie.name
                        )));
                    }
                    validate_metadata_value(&cookie.value, "cookie")?;
                }
                Ok(())
            }
            Self::Download(plan) => {
                if plan.program.as_os_str().is_empty() {
                    return Err(DownloadError::InvalidPlan(
                        "download program is empty".into(),
                    ));
                }
                if plan.args.iter().any(|argument| argument.contains('\0')) {
                    return Err(DownloadError::InvalidPlan(
                        "download argv contains a NUL character".into(),
                    ));
                }
                if plan.output_path.as_os_str().is_empty() {
                    return Err(DownloadError::InvalidPlan(
                        "download output path is empty".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn validate_metadata_value(value: &str, kind: &str) -> DownloadResult<()> {
    if value.len() > 16 * 1024 {
        return Err(DownloadError::InvalidPlan(format!(
            "{kind} value is too long"
        )));
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(DownloadError::InvalidPlan(format!(
            "{kind} value contains a forbidden control character"
        )));
    }
    Ok(())
}

/// Inputs shared by resolvers when constructing a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveContext {
    /// Directory in which downloaded output must remain.
    pub work_dir: PathBuf,
    /// Relative output name or yt-dlp output template.
    pub output_name: String,
    /// Requested media format.
    pub format: MediaFormat,
    /// Optional browser cookie source.
    pub cookies_from_browser: Option<CookieBrowser>,
    /// If true, construct argv only and skip process execution.
    pub simulate: bool,
}

impl ResolveContext {
    /// Creates a context for best-video output without browser cookies.
    pub fn new(work_dir: impl Into<PathBuf>, output_name: impl Into<String>) -> Self {
        Self {
            work_dir: work_dir.into(),
            output_name: output_name.into(),
            format: MediaFormat::BestVideo,
            cookies_from_browser: None,
            simulate: false,
        }
    }

    /// Sets the requested media format.
    pub fn with_format(mut self, format: MediaFormat) -> Self {
        self.format = format;
        self
    }

    /// Sets the browser from which yt-dlp should import cookies.
    pub fn with_cookies_from_browser(mut self, browser: CookieBrowser) -> Self {
        self.cookies_from_browser = Some(browser);
        self
    }

    /// Enables or disables argv-only simulation.
    pub fn with_simulation(mut self, simulate: bool) -> Self {
        self.simulate = simulate;
        self
    }

    /// Returns the validated, confined output path.
    pub fn output_path(&self) -> DownloadResult<PathBuf> {
        crate::resolve_output_path(Path::new(&self.work_dir), &self.output_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_names_are_a_closed_enum() {
        for browser in CookieBrowser::all() {
            assert_eq!(browser.as_str().parse::<CookieBrowser>().unwrap(), *browser);
        }
        assert!("chromium".parse::<CookieBrowser>().is_err());
        assert!("chrome --exec".parse::<CookieBrowser>().is_err());
    }

    #[test]
    fn direct_stream_rejects_header_injection() {
        let stream = DirectStream {
            url: "https://cdn.example.com/video.mp4".into(),
            headers: vec![HttpHeader {
                name: "Referer".into(),
                value: "ok\r\nX-Evil: yes".into(),
            }],
            cookies: Vec::new(),
        };
        assert!(ResolvePlan::DirectStream(stream).validate().is_err());
    }

    #[test]
    fn direct_stream_accepts_headers_and_cookies() {
        let stream = DirectStream {
            url: "https://cdn.example.com/video.mp4".into(),
            headers: vec![HttpHeader {
                name: "Referer".into(),
                value: "https://example.com/".into(),
            }],
            cookies: vec![StreamCookie {
                name: "session".into(),
                value: "abc".into(),
            }],
        };
        assert!(ResolvePlan::DirectStream(stream).validate().is_ok());
    }
}
