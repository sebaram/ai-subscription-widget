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

#[derive(Debug, PartialEq)]
pub enum AutoStartCmd {
    Enable,
    Disable,
}

/// Only touch the OS autostart state when it differs from what config wants.
fn autostart_action(current_enabled: Option<bool>, want: bool) -> Option<AutoStartCmd> {
    match current_enabled {
        Some(true) if want => None,
        Some(false) if !want => None,
        Some(false) | None if want => Some(AutoStartCmd::Enable),
        _ => Some(AutoStartCmd::Disable),
    }
}

fn apply_autostart(app: &tauri::AppHandle, want: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let current = app.autolaunch().is_enabled().ok();
    match autostart_action(current, want) {
        Some(AutoStartCmd::Enable) => {
            let _ = app.autolaunch().enable();
        }
        Some(AutoStartCmd::Disable) => {
            let _ = app.autolaunch().disable();
        }
        None => {}
    }
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
        // logical size → physical px at the monitor's DPI (review I5)
        let scale = w.scale_factor().unwrap_or(1.0);
        let (x, y) = clamped_popup_position(cx, cy, 430.0 * scale, 600.0 * scale, size.width as f64, size.height as f64);
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

/// Float-bar click: toggle the details page (click to show, click again to
/// close). Positioned relative to the float bar so it
/// never drifts to the wrong monitor.
#[tauri::command]
fn toggle_popup(app: tauri::AppHandle) {
    let Some(popup) = app.get_webview_window("popup") else { return };
    if popup.is_visible().unwrap_or(false) {
        let _ = popup.hide();
        return;
    }
    let (cursor, w_size) = app
        .get_webview_window("floatbar")
        .and_then(|f| {
            let pos = f.outer_position().ok()?;
            let size = f.outer_size().ok()?;
            Some((pos, size))
        })
        .map(|(pos, size)| ((pos.x, pos.y), (size.width, size.height)))
        .unwrap_or(((154, 154), (352, 200)));
    let cx = (cursor.0 as i64 + w_size.0 as i64) as f64 + 8.0;
    let cy = cursor.1 as f64;
    if let Ok(Some(monitor)) = popup.current_monitor() {
        let size = monitor.size();
        let scale = popup.scale_factor().unwrap_or(1.0);
        let (x, y) = clamped_popup_position(cx, cy, 430.0 * scale, 600.0 * scale, size.width as f64, size.height as f64);
        let _ = popup.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x as i32, y as i32)));
    }
    let _ = popup.show();
    let _ = popup.set_focus();
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn save_config(cfg: crate::config::Config, app: tauri::AppHandle) -> Result<(), String> {
    crate::config::save(&cfg);
    use tauri::Emitter;
    let _ = app.emit("config-changed", &cfg);
    Ok(())
}

#[tauri::command]
fn set_click_through(on: bool, webview: tauri::WebviewWindow) {
    let _ = webview.set_ignore_cursor_events(on);
}

/// Persist the float bar's current position (backend reads the real position).
#[tauri::command]
fn save_float_position(webview: tauri::WebviewWindow) {
    let Ok(pos) = webview.outer_position() else { return };
    crate::config::update_config(|mut cfg| {
        cfg.float_bar.position = Some((pos.x as f64, pos.y as f64));
        cfg
    });
}

/// Frontend asks once at startup so Settings/thresholds work without waiting for a file change (review I1).
#[tauri::command]
fn get_config() -> crate::config::Config {
    crate::config::load()
}

#[tauri::command]
fn hide_me(webview: tauri::WebviewWindow) {
    let _ = webview.hide();
}

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub available_version: Option<String>,
    pub error: Option<String>,
}

fn friendly_update_error(raw: &str) -> String {
    if raw.contains("404") || raw.contains("release JSON") || raw.contains("Could not fetch") {
        "No signed update manifest on the latest GitHub release yet.".to_string()
    } else {
        raw.to_string()
    }
}

fn updating_flag() -> &'static std::sync::atomic::AtomicBool {
    static FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    &FLAG
}

#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> UpdateStatus {
    use tauri_plugin_updater::UpdaterExt;
    let current_version = app.package_info().version.to_string();
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return UpdateStatus {
                current_version,
                available_version: None,
                error: Some(friendly_update_error(&e.to_string())),
            };
        }
    };
    match updater.check().await {
        Ok(Some(update)) => UpdateStatus {
            current_version,
            available_version: Some(update.version),
            error: None,
        },
        Ok(None) => UpdateStatus {
            current_version,
            available_version: None,
            error: None,
        },
        Err(e) => UpdateStatus {
            current_version,
            available_version: None,
            error: Some(friendly_update_error(&e.to_string())),
        },
    }
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<bool, String> {
    use std::sync::atomic::Ordering;
    use tauri_plugin_updater::UpdaterExt;
    if updating_flag()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false);
    }
    struct ResetGuard;
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            updating_flag().store(false, Ordering::SeqCst);
        }
    }
    let _guard = ResetGuard;

    let updater = app.updater().map_err(|e| friendly_update_error(&e.to_string()))?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|e| friendly_update_error(&e.to_string()))?
    else {
        return Ok(false);
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| friendly_update_error(&e.to_string()))?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Arc::new(SnapshotCache(Default::default())))
        .setup(|app| {
            let state: tauri::State<Arc<SnapshotCache>> = app.state();
            let cache = state.inner().clone();
            scheduler::start(app.handle().clone(), Arc::clone(&cache));

            #[cfg(not(debug_assertions))]
            {
                let upd_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    loop {
                        if crate::config::load().auto_update {
                            let _ = install_update(upd_handle.clone()).await;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                    }
                });
            }

            let cfg0 = config::load();
            apply_autostart(app.handle(), cfg0.autostart);
            if let Some(float_window) = app.get_webview_window("floatbar") {
                // Restore position only if it still lands on a real monitor — saved
                // positions go stale when display layouts change (this exact bug).
                let mut positioned = false;
                if let Some((x, y)) = cfg0.float_bar.position {
                    let on_screen = float_window
                        .available_monitors()
                        .unwrap_or_default()
                        .iter()
                        .any(|m| {
                            let o = m.position();
                            let s = m.size();
                            x >= o.x as f64 && y >= o.y as f64
                                && (x + 20.0) <= (o.x as f64 + s.width as f64)
                                && (y + 20.0) <= (o.y as f64 + s.height as f64)
                        });
                    if on_screen {
                        let _ = float_window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x as i32, y as i32),
                        ));
                        positioned = true;
                    }
                }
                if !positioned && float_window.outer_position().map(|p| p.x < -3000 || p.y > 1600 || p.x > 3000).unwrap_or(false) {
                    // stale spot: reset to the primary monitor's top-left
                    let _ = float_window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(24, 24)));
                    let mut cfg1 = cfg0.clone();
                    cfg1.float_bar.position = Some((24.0, 24.0));
                    config::save(&cfg1);
                }
            }

            let handle = app.handle().clone();
            let watcher_handle = handle.clone();
            config::spawn_config_watcher(handle, move |cfg| {
                apply_autostart(&watcher_handle, cfg.autostart);
            });

            // native drag → persist the position once it settles
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    let mut last: Option<(i32, i32)> = None;
                    let mut changed_at: Option<std::time::Instant> = None;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        let Some(float_window) = app_handle.get_webview_window("floatbar") else { continue };
                        if !float_window.is_visible().unwrap_or(false) {
                            continue;
                        }
                        let Ok(pos) = float_window.outer_position() else { continue };
                        let current = (pos.x, pos.y);
                        if last != Some(current) {
                            last = Some(current);
                            changed_at = Some(std::time::Instant::now());
                            continue;
                        }
                        if let Some(t) = changed_at.take_if(|t| t.elapsed() >= std::time::Duration::from_millis(700)) {
                            let _ = t;
                            let next = Some((current.0 as f64, current.1 as f64));
                            crate::config::update_config(move |mut cfg| {
                                if cfg.float_bar.position != next {
                                    cfg.float_bar.position = next;
                                    cfg
                                } else {
                                    cfg
                                }
                            });
                        }
                    }
                });
            }

            let popup = MenuItem::with_id(app, "popup", "Open popup", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
            let update_item = MenuItem::with_id(app, "update", "Check for updates…", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&popup, &refresh, &update_item, &settings, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("app icon").clone())
                .tooltip("AI Usage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id.as_ref() {
                    "popup" => toggle_popup_at(app, 0.0, 0.0),
                    "refresh" => spawn_window_refresh(app.clone(), &cache),
                    "update" => {
                        let h = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if !matches!(install_update(h.clone()).await, Ok(true)) {
                                show_settings(&h);
                            }
                        });
                    }
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
            refresh_now, open_popup, open_settings, hide_me, save_config,
            save_float_position, set_click_through, get_config, toggle_popup,
            check_for_update, install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{autostart_action, AutoStartCmd, clamped_popup_position};

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

    #[test]
    fn autostart_flag_toggles_plugin() {
        // already as desired → no plugin work
        assert_eq!(autostart_action(Some(true), true), None);
        assert_eq!(autostart_action(Some(false), false), None);
        // wants on
        assert_eq!(autostart_action(Some(false), true), Some(AutoStartCmd::Enable));
        assert_eq!(autostart_action(None, true), Some(AutoStartCmd::Enable));
        // wants off
        assert_eq!(autostart_action(Some(true), false), Some(AutoStartCmd::Disable));
    }

    #[test]
    fn friendly_update_error_maps_missing_manifest() {
        assert!(super::friendly_update_error("status code 404 Not Found").contains("No signed update manifest"));
        assert_eq!(super::friendly_update_error("network timeout"), "network timeout");
    }
}
