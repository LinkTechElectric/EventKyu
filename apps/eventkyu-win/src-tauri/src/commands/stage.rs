use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDescription {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageFullscreenState {
    pub fullscreen: bool,
    pub active_monitor: Option<MonitorDescription>,
    pub monitors: Vec<MonitorDescription>,
}

#[cfg(target_os = "windows")]
fn enumerate_monitors(window: &WebviewWindow) -> Result<Vec<MonitorDescription>, String> {
    // Windows 目标：使用 Tauri 经 wry/tao 暴露的 monitor API 枚举物理显示器，
    // 供后续“选择舞台输出屏”功能复用；当前命令只返回信息并切换全屏。
    window
        .available_monitors()
        .map_err(|error| error.to_string())
        .map(|monitors| {
            monitors
                .into_iter()
                .map(|monitor| MonitorDescription {
                    name: monitor.name().cloned(),
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                    scale_factor: monitor.scale_factor(),
                })
                .collect()
        })
}

#[cfg(not(target_os = "windows"))]
fn enumerate_monitors(_window: &WebviewWindow) -> Result<Vec<MonitorDescription>, String> {
    // 保持 macOS 主机可检查；正式显示器枚举只在 Windows 目标启用。
    Ok(Vec::new())
}

#[tauri::command]
pub fn stage_toggle_fullscreen(app: AppHandle) -> Result<StageFullscreenState, String> {
    let stage = app
        .get_webview_window("stage")
        .ok_or_else(|| "stage window is not available".to_owned())?;
    let next = !stage.is_fullscreen().map_err(|error| error.to_string())?;
    stage
        .set_fullscreen(next)
        .map_err(|error| error.to_string())?;
    let active_monitor = stage
        .current_monitor()
        .map_err(|error| error.to_string())?
        .map(|monitor| MonitorDescription {
            name: monitor.name().cloned(),
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
            scale_factor: monitor.scale_factor(),
        });
    let monitors = enumerate_monitors(&stage)?;

    Ok(StageFullscreenState {
        fullscreen: stage.is_fullscreen().map_err(|error| error.to_string())?,
        active_monitor,
        monitors,
    })
}
