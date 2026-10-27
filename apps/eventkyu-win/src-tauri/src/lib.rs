mod commands;
mod runtime_check;

use cue_core::Engine;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::<Engine>::default())
        .invoke_handler(tauri::generate_handler![
            commands::engine::engine_add_cue,
            commands::engine::engine_go,
            commands::engine::engine_tick,
            commands::engine::engine_status,
            commands::engine::engine_events,
            commands::download::download_resolve,
            runtime_check::runtime_selfcheck,
            commands::stage::stage_toggle_fullscreen,
        ])
        .run(tauri::generate_context!())
        .expect("error while running EventKyu");
}
