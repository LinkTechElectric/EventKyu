use crate::{DownloadEvent, DownloadResult, Platform, ResolveContext, ResolvePlan};

/// A media resolver that can produce a direct stream or a download plan.
///
/// Implementations must be thread-safe. Plugin calls may run on a worker
/// thread so the registry can enforce a timeout.
pub trait MediaResolver: Send + Sync {
    /// Returns the stable identifier recorded in jobs and events.
    fn id(&self) -> &str;

    /// Returns whether this resolver supports `platform`.
    fn supports(&self, platform: Platform) -> bool;

    /// Resolves `url` without performing the planned download.
    fn resolve(&self, url: &str, ctx: &ResolveContext) -> DownloadResult<ResolvePlan>;
}

/// Successful resolution, including any plugin failure and fallback events.
#[derive(Debug)]
pub struct Resolution {
    /// Plan selected by the registry.
    pub plan: ResolvePlan,
    /// Identifier of the resolver that produced the plan.
    pub resolver_used: String,
    /// Plugin failures and fallback notifications generated along the way.
    pub events: Vec<DownloadEvent>,
}
