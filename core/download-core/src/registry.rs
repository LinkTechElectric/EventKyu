use crate::{
    identify_platform, DownloadError, DownloadEvent, DownloadResult, MediaResolver, Resolution,
    ResolveContext, ResolvePlan, YtDlpResolver,
};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

struct PluginEntry {
    resolver: Arc<dyn MediaResolver>,
    enabled: bool,
}

/// Ordered third-party resolver registry with automatic default fallback.
///
/// Enabled plugins are tried in registration order before the default
/// resolver. Plugin errors, panics, invalid plans, and timeouts are reported as
/// events and do not prevent yt-dlp fallback.
pub struct ResolverRegistry {
    default: Arc<dyn MediaResolver>,
    plugins: Vec<PluginEntry>,
    plugin_timeout: Duration,
}

impl ResolverRegistry {
    /// Creates a registry with an application-selected default resolver.
    pub fn new(default: Arc<dyn MediaResolver>) -> Self {
        Self {
            default,
            plugins: Vec::new(),
            plugin_timeout: Duration::from_secs(10),
        }
    }

    /// Registers a typed third-party resolver.
    pub fn register<R>(&mut self, resolver: R) -> DownloadResult<()>
    where
        R: MediaResolver + 'static,
    {
        self.register_arc(Arc::new(resolver))
    }

    /// Registers a shared third-party resolver.
    pub fn register_arc(&mut self, resolver: Arc<dyn MediaResolver>) -> DownloadResult<()> {
        let id = resolver.id();
        if id.is_empty() || id.chars().any(char::is_control) {
            return Err(DownloadError::Resolver(
                "plugin id must be non-empty and contain no control characters".into(),
            ));
        }
        if id == self.default.id() || self.plugins.iter().any(|entry| entry.resolver.id() == id) {
            return Err(DownloadError::PluginAlreadyRegistered(id.to_owned()));
        }
        self.plugins.push(PluginEntry {
            resolver,
            enabled: true,
        });
        Ok(())
    }

    /// Enables or disables a registered plugin without removing it.
    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> DownloadResult<()> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|entry| entry.resolver.id() == plugin_id)
            .ok_or_else(|| DownloadError::PluginNotFound(plugin_id.to_owned()))?;
        plugin.enabled = enabled;
        Ok(())
    }

    /// Returns whether a registered plugin is currently enabled.
    pub fn is_enabled(&self, plugin_id: &str) -> DownloadResult<bool> {
        self.plugins
            .iter()
            .find(|entry| entry.resolver.id() == plugin_id)
            .map(|entry| entry.enabled)
            .ok_or_else(|| DownloadError::PluginNotFound(plugin_id.to_owned()))
    }

    /// Sets the maximum duration of each plugin resolve call.
    pub fn set_plugin_timeout(&mut self, timeout: Duration) {
        self.plugin_timeout = timeout;
    }

    /// Resolves a URL, trying enabled plugins before the default resolver.
    pub fn resolve(&self, url: &str, ctx: &ResolveContext) -> DownloadResult<Resolution> {
        let platform = identify_platform(url)?;
        let mut events = Vec::new();
        let mut failed_plugins = Vec::new();

        for plugin in &self.plugins {
            if !plugin.enabled || !plugin.resolver.supports(platform) {
                continue;
            }
            let plugin_id = plugin.resolver.id().to_owned();
            let result = self.invoke_plugin(plugin, url, ctx).and_then(|plan| {
                plan.validate()?;
                Ok(plan)
            });
            match result {
                Ok(plan) => {
                    return Ok(Resolution {
                        plan,
                        resolver_used: plugin_id,
                        events,
                    });
                }
                Err(error) => {
                    events.push(DownloadEvent::PluginFailed {
                        plugin_id: plugin_id.clone(),
                        error: error.to_string(),
                    });
                    failed_plugins.push(plugin_id);
                }
            }
        }

        if !self.default.supports(platform) {
            return Err(DownloadError::Resolver(format!(
                "default resolver '{}' does not support {platform:?}",
                self.default.id()
            )));
        }
        for plugin_id in failed_plugins {
            events.push(DownloadEvent::FallbackTriggered {
                plugin_id,
                fallback_id: self.default.id().to_owned(),
            });
        }
        let plan = self.default.resolve(url, ctx)?;
        plan.validate()?;
        Ok(Resolution {
            plan,
            resolver_used: self.default.id().to_owned(),
            events,
        })
    }

    fn invoke_plugin(
        &self,
        plugin: &PluginEntry,
        url: &str,
        ctx: &ResolveContext,
    ) -> DownloadResult<ResolvePlan> {
        let resolver = Arc::clone(&plugin.resolver);
        let plugin_id = plugin.resolver.id().to_owned();
        let url = url.to_owned();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name(format!("eventkyu-resolver-{plugin_id}"))
            .spawn(move || {
                let result = resolver.resolve(&url, &ctx);
                let _ = sender.send(result);
            })
            .map_err(|error| {
                DownloadError::Resolver(format!("cannot start plugin '{plugin_id}': {error}"))
            })?;

        match receiver.recv_timeout(self.plugin_timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(DownloadError::PluginTimeout {
                plugin_id,
                timeout: self.plugin_timeout,
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(DownloadError::Resolver(format!(
                "plugin '{plugin_id}' terminated without a result"
            ))),
        }
    }
}

impl Default for ResolverRegistry {
    fn default() -> Self {
        Self::new(Arc::new(YtDlpResolver::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DirectStream, Platform};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    enum Behavior {
        Direct,
        Fail,
        Delay(Duration),
        InvalidDirect,
    }

    struct TestResolver {
        id: &'static str,
        supported: Platform,
        behavior: Behavior,
        calls: AtomicUsize,
    }

    impl TestResolver {
        fn new(id: &'static str, supported: Platform, behavior: Behavior) -> Self {
            Self {
                id,
                supported,
                behavior,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl MediaResolver for TestResolver {
        fn id(&self) -> &str {
            self.id
        }

        fn supports(&self, platform: Platform) -> bool {
            self.supported == platform
        }

        fn resolve(&self, _url: &str, _ctx: &ResolveContext) -> DownloadResult<ResolvePlan> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.behavior {
                Behavior::Direct => Ok(ResolvePlan::DirectStream(DirectStream::new(
                    "https://cdn.example.com/video.mp4",
                ))),
                Behavior::Fail => Err(DownloadError::Resolver("plugin is broken".into())),
                Behavior::Delay(duration) => {
                    thread::sleep(duration);
                    Ok(ResolvePlan::DirectStream(DirectStream::new(
                        "https://cdn.example.com/late.mp4",
                    )))
                }
                Behavior::InvalidDirect => Ok(ResolvePlan::DirectStream(DirectStream::new(
                    "file:///tmp/not-http.mp4",
                ))),
            }
        }
    }

    fn context() -> ResolveContext {
        ResolveContext::new("/tmp", "video.%(ext)s")
    }

    #[test]
    fn plugin_takes_priority_over_yt_dlp() {
        let mut registry = ResolverRegistry::default();
        registry
            .register(TestResolver::new(
                "bilibili-plugin",
                Platform::Bilibili,
                Behavior::Direct,
            ))
            .unwrap();
        let resolution = registry
            .resolve("https://www.bilibili.com/video/BV1", &context())
            .unwrap();
        assert_eq!(resolution.resolver_used, "bilibili-plugin");
        assert!(matches!(resolution.plan, ResolvePlan::DirectStream(_)));
        assert!(resolution.events.is_empty());
    }

    #[test]
    fn plugin_failure_records_events_and_falls_back() {
        let mut registry = ResolverRegistry::default();
        registry
            .register(TestResolver::new(
                "broken-plugin",
                Platform::Bilibili,
                Behavior::Fail,
            ))
            .unwrap();
        let resolution = registry.resolve("https://b23.tv/abc", &context()).unwrap();
        assert_eq!(resolution.resolver_used, YtDlpResolver::ID);
        assert!(matches!(resolution.plan, ResolvePlan::Download(_)));
        assert!(resolution.events.iter().any(|event| matches!(
            event,
            DownloadEvent::PluginFailed { plugin_id, .. } if plugin_id == "broken-plugin"
        )));
        assert!(resolution.events.iter().any(|event| matches!(
            event,
            DownloadEvent::FallbackTriggered { plugin_id, fallback_id }
                if plugin_id == "broken-plugin" && fallback_id == YtDlpResolver::ID
        )));
    }

    #[test]
    fn plugin_timeout_falls_back() {
        let mut registry = ResolverRegistry::default();
        registry.set_plugin_timeout(Duration::from_millis(5));
        registry
            .register(TestResolver::new(
                "slow-plugin",
                Platform::YouTube,
                Behavior::Delay(Duration::from_millis(50)),
            ))
            .unwrap();
        let resolution = registry
            .resolve("https://youtu.be/abc", &context())
            .unwrap();
        assert_eq!(resolution.resolver_used, YtDlpResolver::ID);
        assert!(resolution.events.iter().any(|event| matches!(
            event,
            DownloadEvent::PluginFailed { error, .. } if error.contains("timed out")
        )));
    }

    #[test]
    fn disabled_plugin_is_not_called() {
        let mut registry = ResolverRegistry::default();
        let plugin = Arc::new(TestResolver::new(
            "disabled-plugin",
            Platform::Douyin,
            Behavior::Fail,
        ));
        registry.register_arc(plugin.clone()).unwrap();
        registry.set_enabled("disabled-plugin", false).unwrap();
        assert!(!registry.is_enabled("disabled-plugin").unwrap());

        let resolution = registry
            .resolve("https://v.douyin.com/abc", &context())
            .unwrap();
        assert_eq!(resolution.resolver_used, YtDlpResolver::ID);
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);
        assert!(resolution.events.is_empty());
    }

    #[test]
    fn unsupported_plugin_is_skipped() {
        let mut registry = ResolverRegistry::default();
        let plugin = Arc::new(TestResolver::new(
            "youtube-only",
            Platform::YouTube,
            Behavior::Direct,
        ));
        registry.register_arc(plugin.clone()).unwrap();
        let resolution = registry
            .resolve("https://www.bilibili.com/video/BV1", &context())
            .unwrap();
        assert_eq!(resolution.resolver_used, YtDlpResolver::ID);
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn invalid_plugin_plan_falls_back() {
        let mut registry = ResolverRegistry::default();
        registry
            .register(TestResolver::new(
                "invalid-plan",
                Platform::Xiaohongshu,
                Behavior::InvalidDirect,
            ))
            .unwrap();
        let resolution = registry
            .resolve("https://xhslink.com/a/abc", &context())
            .unwrap();
        assert_eq!(resolution.resolver_used, YtDlpResolver::ID);
        assert!(resolution
            .events
            .iter()
            .any(|event| matches!(event, DownloadEvent::PluginFailed { .. })));
    }

    #[test]
    fn rejects_duplicate_and_unknown_plugin_operations() {
        let mut registry = ResolverRegistry::default();
        registry
            .register(TestResolver::new(
                "duplicate",
                Platform::Generic,
                Behavior::Direct,
            ))
            .unwrap();
        assert!(matches!(
            registry.register(TestResolver::new(
                "duplicate",
                Platform::Generic,
                Behavior::Direct
            )),
            Err(DownloadError::PluginAlreadyRegistered(_))
        ));
        assert!(matches!(
            registry.set_enabled("missing", true),
            Err(DownloadError::PluginNotFound(_))
        ));
    }
}
