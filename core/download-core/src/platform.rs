use crate::security::url_host;
use crate::DownloadResult;
#[cfg(any(test, doc))]
use crate::MAX_URL_LENGTH;

/// Media platforms recognized by EventKyu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Bilibili, including `b23.tv` short links.
    Bilibili,
    /// Douyin.
    Douyin,
    /// Xiaohongshu, including `xhslink.com` short links.
    Xiaohongshu,
    /// NetEase Cloud Music.
    NetEaseMusic,
    /// YouTube, including `youtu.be` short links.
    YouTube,
    /// Any other valid HTTP or HTTPS URL.
    Generic,
}

impl Platform {
    /// Identifies a platform from a validated HTTP or HTTPS URL.
    ///
    /// Host matching is exact or on a dot boundary; strings in paths,
    /// user-info, or lookalike domains do not cause false positives.
    pub fn from_url(url: &str) -> DownloadResult<Self> {
        identify_platform(url)
    }
}

/// Identifies the media platform for `url`.
///
/// URLs longer than [`MAX_URL_LENGTH`] bytes or using a non-HTTP(S) scheme are
/// rejected instead of being classified as generic.
pub fn identify_platform(url: &str) -> DownloadResult<Platform> {
    let host = url_host(url)?;
    let platform = if host_matches(&host, "bilibili.com") || host_matches(&host, "b23.tv") {
        Platform::Bilibili
    } else if host_matches(&host, "douyin.com") {
        Platform::Douyin
    } else if host_matches(&host, "xiaohongshu.com") || host_matches(&host, "xhslink.com") {
        Platform::Xiaohongshu
    } else if host_matches(&host, "music.163.com") {
        Platform::NetEaseMusic
    } else if host_matches(&host, "youtube.com") || host_matches(&host, "youtu.be") {
        Platform::YouTube
    } else {
        Platform::Generic
    };
    Ok(platform)
}

fn host_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_bilibili_and_short_links() {
        for url in [
            "https://www.bilibili.com/video/BV1xx411c7XX",
            "https://m.bilibili.com/video/BV1xx411c7XX",
            "https://b23.tv/abc123",
        ] {
            assert_eq!(identify_platform(url).unwrap(), Platform::Bilibili);
        }
    }

    #[test]
    fn identifies_douyin() {
        for url in [
            "https://www.douyin.com/video/123",
            "https://v.douyin.com/abc/",
        ] {
            assert_eq!(identify_platform(url).unwrap(), Platform::Douyin);
        }
    }

    #[test]
    fn identifies_xiaohongshu_and_short_links() {
        for url in [
            "https://www.xiaohongshu.com/explore/abc",
            "https://xhslink.com/a/abc",
        ] {
            assert_eq!(identify_platform(url).unwrap(), Platform::Xiaohongshu);
        }
    }

    #[test]
    fn identifies_netease_music() {
        assert_eq!(
            identify_platform("https://music.163.com/#/song?id=1").unwrap(),
            Platform::NetEaseMusic
        );
    }

    #[test]
    fn identifies_youtube_and_short_links() {
        for url in [
            "https://www.youtube.com/watch?v=abc",
            "https://m.youtube.com/watch?v=abc",
            "https://youtu.be/abc?t=3",
        ] {
            assert_eq!(identify_platform(url).unwrap(), Platform::YouTube);
        }
    }

    #[test]
    fn identifies_other_hosts_as_generic() {
        for url in [
            "https://example.com/bilibili.com/video",
            "https://bilibili.com.evil.example/video",
            "https://notyoutube.com/watch",
            "https://163.com/song",
        ] {
            assert_eq!(identify_platform(url).unwrap(), Platform::Generic);
        }
    }

    #[test]
    fn handles_case_ports_and_trailing_dots() {
        assert_eq!(
            identify_platform("HTTPS://WWW.YOUTUBE.COM.:443/watch?v=x").unwrap(),
            Platform::YouTube
        );
        assert_eq!(
            identify_platform("https://B23.TV./abc").unwrap(),
            Platform::Bilibili
        );
    }

    #[test]
    fn host_matching_uses_authority_not_user_info() {
        assert_eq!(
            identify_platform("https://bilibili.com@evil.example/x").unwrap(),
            Platform::Generic
        );
        assert_eq!(
            identify_platform("https://user@bilibili.com/x").unwrap(),
            Platform::Bilibili
        );
    }

    #[test]
    fn rejects_non_http_urls() {
        for url in [
            "file:///tmp/a",
            "javascript:alert(1)",
            "ftp://example.com/a",
        ] {
            assert!(matches!(
                identify_platform(url),
                Err(crate::DownloadError::InvalidUrl(_))
            ));
        }
    }

    #[test]
    fn rejects_missing_host_and_overlong_urls() {
        assert!(identify_platform("https:///missing").is_err());
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LENGTH));
        assert!(identify_platform(&long).is_err());
    }
}
