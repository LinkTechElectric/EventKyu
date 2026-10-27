#![forbid(unsafe_code)]

mod ai;
mod commands;
mod keystore;
mod process;
pub mod realtime;

use commands::AppState;

/// Starts the SignalFit desktop application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_presets,
            commands::preset_plan,
            commands::plan_validate,
            commands::plan_approve,
            commands::plan_reject,
            commands::plan_cancel,
            commands::plan_execute,
            commands::export_log,
            commands::ai_generate,
            commands::key_set,
            commands::key_status,
            commands::key_delete,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SignalFit");
}
