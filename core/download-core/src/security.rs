use crate::{DownloadError, DownloadResult};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Maximum accepted URL length in bytes.
pub const MAX_URL_LENGTH: usize = 2048;
/// Maximum accepted output name length in bytes.
pub const MAX_OUTPUT_NAME_LENGTH: usize = 512;

/// Validates that `url` is an HTTP(S) URL with a host and safe argv contents.
pub fn validate_url(url: &str) -> DownloadResult<()> {
    url_host(url).map(|_| ())
}

/// Normalizes the working directory and confines an output name within it.
///
/// The output name must be relative and must not contain any `..` component.
/// Existing path components are canonicalized as well, which prevents an
/// existing symbolic link from redirecting output outside the working tree.
pub fn resolve_output_path(work_dir: &Path, output_name: &str) -> DownloadResult<PathBuf> {
    if output_name.is_empty() {
        return Err(DownloadError::InvalidOutputName(
            "name must not be empty".into(),
        ));
    }
    if output_name.len() > MAX_OUTPUT_NAME_LENGTH {
        return Err(DownloadError::InvalidOutputName(format!(
            "name exceeds {MAX_OUTPUT_NAME_LENGTH} bytes"
        )));
    }
    if output_name.chars().any(char::is_control) {
        return Err(DownloadError::InvalidOutputName(
            "name contains a control character".into(),
        ));
    }

    let relative = Path::new(output_name);
    if relative.is_absolute() {
        return Err(DownloadError::InvalidOutputName(
            "name must be relative".into(),
        ));
    }
    let mut has_normal_component = false;
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                return Err(DownloadError::InvalidOutputName(
                    "name must not contain '..'".into(),
                ))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(DownloadError::InvalidOutputName(
                    "name must be relative".into(),
                ))
            }
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir => {}
        }
    }
    if !has_normal_component {
        return Err(DownloadError::InvalidOutputName(
            "name must identify a file".into(),
        ));
    }

    let root = normalize_from_filesystem(&absolute(work_dir)?)?;
    let candidate = normalize_from_filesystem(&root.join(relative))?;
    if !candidate.starts_with(&root) {
        return Err(DownloadError::OutputOutsideWorkDir {
            work_dir: root,
            output: candidate,
        });
    }
    Ok(candidate)
}

pub(crate) fn url_host(url: &str) -> DownloadResult<String> {
    if url.is_empty() {
        return Err(DownloadError::InvalidUrl("URL is empty".into()));
    }
    if url.len() > MAX_URL_LENGTH {
        return Err(DownloadError::InvalidUrl(format!(
            "URL exceeds {MAX_URL_LENGTH} bytes"
        )));
    }
    if url
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(DownloadError::InvalidUrl(
            "URL contains whitespace or a control character".into(),
        ));
    }

    let (scheme, remainder) = url
        .split_once("://")
        .ok_or_else(|| DownloadError::InvalidUrl("missing '://' separator".into()))?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(DownloadError::InvalidUrl(
            "only http and https are allowed".into(),
        ));
    }

    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if host_port.is_empty() {
        return Err(DownloadError::InvalidUrl("host is empty".into()));
    }

    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        let closing = bracketed
            .find(']')
            .ok_or_else(|| DownloadError::InvalidUrl("unclosed IPv6 host".into()))?;
        let host = &bracketed[..closing];
        let suffix = &bracketed[closing + 1..];
        if !suffix.is_empty() {
            let port = suffix
                .strip_prefix(':')
                .ok_or_else(|| DownloadError::InvalidUrl("invalid IPv6 authority".into()))?;
            validate_port(port)?;
        }
        host.to_owned()
    } else {
        let (host, port) = host_port
            .rsplit_once(':')
            .map_or((host_port, None), |(host, port)| (host, Some(port)));
        if host.contains(':') {
            return Err(DownloadError::InvalidUrl(
                "IPv6 hosts must use brackets".into(),
            ));
        }
        if let Some(port) = port {
            validate_port(port)?;
        }
        host.to_owned()
    };

    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(DownloadError::InvalidUrl("host is empty".into()));
    }
    Ok(host)
}

fn validate_port(port: &str) -> DownloadResult<()> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DownloadError::InvalidUrl("invalid port".into()));
    }
    if port.parse::<u16>().is_err() {
        return Err(DownloadError::InvalidUrl(
            "port is outside the valid range".into(),
        ));
    }
    Ok(())
}

fn absolute(path: &Path) -> DownloadResult<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(DownloadError::InvalidOutputName(
            "working directory is empty".into(),
        ));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|error| DownloadError::Io(error.to_string()))
    }
}

fn normalize_from_filesystem(path: &Path) -> DownloadResult<PathBuf> {
    let lexical = normalize_lexical(path);
    let mut ancestor = lexical.as_path();
    let mut suffix: Vec<OsString> = Vec::new();

    while !ancestor.exists() {
        let name = ancestor.file_name().ok_or_else(|| {
            DownloadError::Io(format!("cannot normalize '{}'", lexical.display()))
        })?;
        suffix.push(name.to_os_string());
        ancestor = ancestor.parent().ok_or_else(|| {
            DownloadError::Io(format!("cannot normalize '{}'", lexical.display()))
        })?;
    }

    let mut resolved = fs::canonicalize(ancestor).map_err(|error| {
        DownloadError::Io(format!(
            "cannot canonicalize '{}': {error}",
            ancestor.display()
        ))
    })?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "eventkyu-download-core-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn rejects_parent_traversal() {
        for name in ["../escape", "nested/../../escape", ".."] {
            assert!(matches!(
                resolve_output_path(Path::new("/tmp"), name),
                Err(DownloadError::InvalidOutputName(_))
            ));
        }
    }

    #[test]
    fn rejects_absolute_and_control_character_names() {
        assert!(resolve_output_path(Path::new("/tmp"), "/tmp/escape").is_err());
        assert!(resolve_output_path(Path::new("/tmp"), "bad\nname").is_err());
        assert!(resolve_output_path(Path::new("/tmp"), "").is_err());
    }

    #[test]
    fn confines_normalized_output_to_work_dir() {
        let root = temp_dir("normalized");
        let output = resolve_output_path(&root, "video.%(ext)s").unwrap();
        assert!(output.starts_with(fs::canonicalize(&root).unwrap()));
        assert!(output.ends_with("video.%(ext)s"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = temp_dir("symlink-root");
        let outside = temp_dir("symlink-outside");
        symlink(&outside, root.join("link")).unwrap();
        assert!(matches!(
            resolve_output_path(&root, "link/video.mp4"),
            Err(DownloadError::OutputOutsideWorkDir { .. })
        ));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn validates_urls_for_argv() {
        assert!(validate_url("https://example.com/a?b=c").is_ok());
        assert!(validate_url("https://example.com/a\n--exec=bad").is_err());
        assert!(validate_url("https://example.com:99999/a").is_err());
        assert!(validate_url("https://[::1]:443/a").is_ok());
    }
}
