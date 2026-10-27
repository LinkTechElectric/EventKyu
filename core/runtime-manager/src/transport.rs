use std::collections::BTreeMap;

use crate::RuntimeError;

/// A byte range beginning at `start` and continuing to the end of the resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeRequest {
    /// Zero-based offset of the first requested byte.
    pub start: u64,
}

impl RangeRequest {
    /// Creates a suffix range request starting at `start`.
    pub const fn from(start: u64) -> Self {
        Self { start }
    }

    /// Returns the corresponding HTTP `Range` header value.
    pub fn header_value(&self) -> String {
        format!("bytes={}-", self.start)
    }
}

/// A fully buffered HTTP response returned by an injected [`Transport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers. Header lookup is transport-defined and case-insensitive
    /// implementations should normalize names to lowercase.
    pub headers: BTreeMap<String, String>,
    /// Response body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Creates a response with no headers.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Network abstraction used by the downloader.
///
/// The application layer supplies the real HTTP implementation. Implementations
/// should follow redirects, apply `RangeRequest` as an HTTP `Range` header, and
/// return `206 Partial Content` when a range is honored. No concrete HTTP client
/// is bundled in this crate.
pub trait Transport: Send + Sync {
    /// Performs a GET, optionally requesting bytes from `range.start` onward.
    fn get(&self, url: &str, range: Option<RangeRequest>) -> Result<HttpResponse, RuntimeError>;

    /// Performs a HEAD request for metadata or connectivity checks.
    fn head(&self, url: &str) -> Result<HttpResponse, RuntimeError>;
}
