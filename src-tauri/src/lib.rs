pub mod config;
pub mod model;
pub mod providers;
pub mod scheduler;

use scheduler::SnapshotCache;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

/// Where to place a w×h popup so it stays on screen: under the cursor, clamped inside.
fn clamped_popup_position(cx: f64, cy: f64, w: f64, h: f64, screen_w: f64, screen_h: f64) -> (f64, f64) {
    let x = (cx - w / 2.0).clamp(4.0, (screen_w - w - 4.0).max(4.0));
    let y = (cy + 12.0).clamp(4.0, (screen_h - h - 4.0).max(4.0));
    (x, y)
}

fn spawn_window_refresh(app: tauri::AppHandle, cache: &Arc<SnapshotCache>) {
    let cache = Arc::clone(cache);
    tauri::async_runtime::spawn(async move {
        scheduler::refresh_and_emit(&app, &cache.0).await;
    });
}

fn toggle_popup_at(app: &tauri::AppHandle, cx: f64, cy: f64) {
    let Some(w) = app.get_webview_window("popup") else { return };
    if w.is_visible().unwrap_or(false) {
        let _ = w.hide();
        return;
    }
    if let Ok(Some(monitor)) = w.current_monitor() {
        let size = monitor.size();
        let (x, y) = clamped_popup_position(cx, cy, 430.0, 600.0, size.width as f64, size.height as f64);
        let _ = w.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x as i32, y as i32)));
    }
    let _ = w.show();
    let _ = w.set_focus();
}

fn show_settings(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

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
    let Some(w) = app.get_webview_window("popup") else { return };
    let (x, _y) = match w.outer_position() {
        Ok(p) => (p.x as f64, p.y as f64),
        Err(_) => (0.0, 0.0),
    };
    toggle_popup_at(&app, x, 200.0);
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn hide_me(webview: tauri::WebviewWindow) {
    let _ = webview.hide();
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(SnapshotCache(Default::default())))
        .setup(|app| {
            let state: tauri::State<Arc<SnapshotCache>> = app.state();
            let cache = state.inner().clone();
            scheduler::start(app.handle().clone(), Arc::clone(&cache));
            config::spawn_config_watcher(app.handle().clone());

            let popup = MenuItem::with_id(app, "popup", "Open popup", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&popup, &refresh, &settings, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("app icon").clone())
                .tooltip("AI Usage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id.as_ref() {
                    "popup" => toggle_popup_at(app, 0.0, 0.0),
                    "refresh" => spawn_window_refresh(app.clone(), &cache),
                    "settings" => show_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        ..
                    } = event
                    {
                        // event position is already physical/global
                        toggle_popup_at(tray.app_handle(), position.x, position.y);
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet, refresh_now, open_popup, open_settings, hide_me
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::clamped_popup_position;

    #[test]
    fn popup_position_command_clamps_to_screen() {
        // near right/bottom edge: popup shifted fully inside
        let (x, y) = clamped_popup_position(3000.0, 1700.0, 400.0, 500.0, 3072.0, 1728.0);
        assert_eq!(x, 3072.0 - 400.0 - 4.0);
        assert_eq!(y, 1728.0 - 500.0 - 4.0);
        // mid-screen: roughly at cursor, nudged below
        let (x2, y2) = clamped_popup_position(1536.0, 864.0, 400.0, 500.0, 3072.0, 1728.0);
        assert_eq!(x2, 1536.0 - 200.0);
        assert_eq!(y2, 864.0 + 12.0);
        // tiny screen: never negative
        let (x3, y3) = clamped_popup_position(0.0, 0.0, 800.0, 600.0, 300.0, 200.0);
        assert_eq!(x3, 4.0);
        assert_eq!(y3, 4.0);
    }
}
