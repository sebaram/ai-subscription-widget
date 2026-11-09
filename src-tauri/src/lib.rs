pub mod config;
pub mod model;
pub mod providers;
pub mod scheduler;

use scheduler::SnapshotCache;
use std::sync::Arc;
use tauri::Manager;

#[tauri::command]
async fn refresh_now(
    cache: tauri::State<'_, Arc<SnapshotCache>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    scheduler::refresh_and_emit(&app, &cache.0).await;
    Ok(())
}

#[tauri::command]
fn open_popup(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn hide_me(webview: tauri::WebviewWindow) {
    let _ = webview.hide();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(SnapshotCache(Default::default())))
        .setup(|app| {
            let state: tauri::State<Arc<SnapshotCache>> = app.state();
            scheduler::start(app.handle().clone(), state.inner().clone());
            config::spawn_config_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![refresh_now, open_popup, hide_me])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
