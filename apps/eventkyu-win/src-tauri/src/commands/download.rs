use download_core::{Platform, ResolveContext, ResolvePlan, ResolverRegistry};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolveDescription {
    pub platform: String,
    pub resolver_used: String,
    pub execution: String,
    pub plan: PlanDescription,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "detail", rename_all = "camelCase")]
pub enum PlanDescription {
    Download(DownloadDescription),
    DirectStream(DirectStreamDescription),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DownloadDescription {
    pub program: String,
    pub args: Vec<String>,
    pub output_path: String,
    pub simulate: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectStreamDescription {
    pub url: String,
    pub headers: Vec<MetadataDescription>,
    pub cookies: Vec<MetadataDescription>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MetadataDescription {
    pub name: String,
    pub value: String,
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Bilibili => "bilibili",
        Platform::Douyin => "douyin",
        Platform::Xiaohongshu => "xiaohongshu",
        Platform::NetEaseMusic => "netEaseMusic",
        Platform::YouTube => "youtube",
        Platform::Generic => "generic",
    }
}

fn resolve_description(url: &str, work_dir: &Path) -> Result<ResolveDescription, String> {
    let platform = Platform::from_url(url).map_err(|error| error.to_string())?;
    let registry = ResolverRegistry::default();
    let context = ResolveContext::new(work_dir, "eventkyu-%(title)s.%(ext)s").with_simulation(true);
    let resolution = registry
        .resolve(url, &context)
        .map_err(|error| error.to_string())?;

    // 真实下载接入点：保留 DownloadPlan，将其与 DownloadJob 传给
    // DownloadEngine::download，并注入 Windows ProcessRunner。
    // 此壳命令只返回 argv，绝不启动 yt-dlp，也不访问网络。
    let plan = match resolution.plan {
        ResolvePlan::Download(plan) => PlanDescription::Download(DownloadDescription {
            program: plan.program.to_string_lossy().into_owned(),
            args: plan.args,
            output_path: plan.output_path.to_string_lossy().into_owned(),
            simulate: plan.simulate,
        }),
        ResolvePlan::DirectStream(stream) => {
            PlanDescription::DirectStream(DirectStreamDescription {
                url: stream.url,
                headers: stream
                    .headers
                    .into_iter()
                    .map(|header| MetadataDescription {
                        name: header.name,
                        value: header.value,
                    })
                    .collect(),
                cookies: stream
                    .cookies
                    .into_iter()
                    .map(|cookie| MetadataDescription {
                        name: cookie.name,
                        value: cookie.value,
                    })
                    .collect(),
            })
        }
    };

    Ok(ResolveDescription {
        platform: platform_name(platform).to_owned(),
        resolver_used: resolution.resolver_used,
        execution: "planOnly".to_owned(),
        plan,
    })
}

#[tauri::command]
pub fn download_resolve(url: String) -> Result<ResolveDescription, String> {
    let work_dir = std::env::temp_dir().join("eventkyu-win").join("downloads");
    resolve_description(&url, &work_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_identification_passes_through_core() {
        let platform = Platform::from_url("https://b23.tv/abc123").unwrap();
        assert_eq!(platform_name(platform), "bilibili");
        assert!(Platform::from_url("file:///tmp/video.mp4").is_err());
    }

    #[test]
    fn default_download_is_a_simulated_argv_plan() {
        let description = resolve_description("https://youtu.be/abc", Path::new("/tmp")).unwrap();
        assert_eq!(description.platform, "youtube");
        assert_eq!(description.resolver_used, "yt-dlp");
        assert_eq!(description.execution, "planOnly");
        let PlanDescription::Download(plan) = description.plan else {
            panic!("default resolver must produce a download plan");
        };
        assert_eq!(plan.program, "yt-dlp");
        assert!(plan.simulate);
        assert_eq!(plan.args[plan.args.len() - 2], "--");
        assert_eq!(plan.args.last().unwrap(), "https://youtu.be/abc");
    }
}
