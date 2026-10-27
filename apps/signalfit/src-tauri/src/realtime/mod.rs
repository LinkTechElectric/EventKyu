//! Real-time signal adapter contracts.
//!
//! This module deliberately contains contracts only. The offline toolbox is
//! functional; capture, GPU processing, and output backends are later work.

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Supported real-time capture families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputSource {
    /// macOS Syphon shared texture.
    Syphon,
    /// NDI network video source.
    Ndi,
    /// Blackmagic DeckLink capture device.
    DeckLink,
    /// USB Video Class capture device.
    Uvc,
}

/// Supported real-time output families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputTarget {
    /// macOS Syphon shared texture output.
    Syphon,
    /// NDI network video output.
    Ndi,
    /// Blackmagic DeckLink output device.
    DeckLink,
    /// Local preview window.
    Preview,
}

/// An explicit unsupported-backend error instead of a panic or `todo!()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealtimeError {
    message: &'static str,
}

impl RealtimeError {
    /// Creates an unsupported-operation error with a stable user-facing message.
    pub const fn unsupported(message: &'static str) -> Self {
        Self { message }
    }
}

impl Display for RealtimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for RealtimeError {}

/// Lifecycle contract for a real-time input → processing → output pipeline.
///
/// Integration points:
///
/// * Syphon/DeckLink/Uvc frames should be imported as `wgpu::Texture` handles;
///   NDI receives need a zero-copy import path where the SDK permits it.
/// * `wgpu` owns device/queue/surface management and synchronization.
/// * `libplacebo` is the color-management, scaling, debanding, and tone-mapping
///   insertion point, backed by the same GPU device where supported.
/// * Output backends consume GPU textures directly.
///
/// GPU discipline: once a frame enters the pipeline it must remain on the GPU
/// through conversion, scaling, color processing, and output. Do not introduce
/// CPU readback between stages; any unavoidable capture upload or SDK download
/// must be measured, documented, and isolated at that backend boundary.
pub trait RealtimePipeline {
    /// Opens the selected input and output and starts frame delivery.
    fn start(&mut self, input: InputSource, output: OutputTarget) -> Result<(), RealtimeError>;

    /// Stops frame delivery and releases backend resources.
    fn stop(&mut self) -> Result<(), RealtimeError>;
}

/// Placeholder used until platform backends are implemented.
#[derive(Debug, Default)]
pub struct UnsupportedRealtimePipeline;

impl RealtimePipeline for UnsupportedRealtimePipeline {
    fn start(&mut self, _input: InputSource, _output: OutputTarget) -> Result<(), RealtimeError> {
        Err(RealtimeError::unsupported(
            "实时信号适配链路仍在开发中，当前版本仅提供离线媒体工具箱",
        ))
    }

    fn stop(&mut self) -> Result<(), RealtimeError> {
        Err(RealtimeError::unsupported(
            "实时管线尚未启动，没有可停止的后端",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_unsupported_instead_of_panicking() {
        let mut pipeline = UnsupportedRealtimePipeline;
        let error = pipeline
            .start(InputSource::Syphon, OutputTarget::Preview)
            .unwrap_err();
        assert!(error.to_string().contains("开发中"));
        assert!(pipeline.stop().is_err());
    }
}
