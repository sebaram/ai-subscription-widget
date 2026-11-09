use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub providers: Vec<ProviderCfg>,
    pub thresholds: Thresholds,
    pub poll_seconds: u64,
    pub per_provider_poll: HashMap<String, u64>,
    pub float_bar: FloatBarCfg,
    pub theme: String,
    pub ollama_api_key: String,
    pub autostart: bool,
    pub antigravity_oauth: bool,
    pub bar_thickness: u32,
    pub font_size: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderCfg {
    pub id: String,
    pub enabled: bool,
    pub float_windows: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Thresholds {
    pub amber: f64,
    pub red: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FloatBarCfg {
    pub opacity: f64,
    pub click_through: bool,
    pub position: Option<(f64, f64)>,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self { amber: 50.0, red: 80.0 }
    }
}

impl Default for FloatBarCfg {
    fn default() -> Self {
        Self { opacity: 0.95, click_through: false, position: None }
    }
}

impl Default for ProviderCfg {
    fn default() -> Self {
        Self { id: String::new(), enabled: true, float_windows: None }
    }
}

impl Default for Config {
    fn default() -> Self {
        defaults()
    }
}

pub fn defaults() -> Config {
    let provider = |id: &str| ProviderCfg { id: id.to_string(), ..Default::default() };
    Config {
        providers: vec![provider("claude"), provider("codex"), provider("antigravity"), provider("ollama")],
        thresholds: Thresholds::default(),
        poll_seconds: 300,
        per_provider_poll: HashMap::new(),
        float_bar: FloatBarCfg::default(),
        theme: "dark".to_string(),
        ollama_api_key: String::new(),
        autostart: true,
        antigravity_oauth: false,
        bar_thickness: 5,
        font_size: 10,
    }
}

fn save_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// Load-mutate-save under one lock — the only sanctioned way to persist a change.
pub fn update_config_at<F: FnOnce(Config) -> Config>(path: &std::path::Path, f: F) {
    let _lock = save_lock().lock().unwrap_or_else(|e| e.into_inner());
    let next = f(load_at(path));
    save_at(path, &next);
}

pub fn update_config<F: FnOnce(Config) -> Config>(f: F) {
    update_config_at(&config_path(), f);
}

pub fn config_path() -> PathBuf {
    // %APPDATA%\ai-usage\config.json
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("ai-usage").join("config.json")
}

pub fn load_at(path: &std::path::Path) -> Config {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        // missing file: seed defaults once
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let d = defaults();
            save_at(path, &d);
            return d;
        }
        // transient read failure (lock, AV scan) — keep the user's file untouched (review I6)
        Err(_) => return defaults(),
    };
    match serde_json::from_slice::<Config>(&bytes) {
        Ok(cfg) => cfg,
        Err(_) => {
            // malformed: preserve the original before rewriting (review I6)
            let _ = std::fs::copy(path, path.with_extension("json.bak"));
            let d = defaults();
            save_at(path, &d);
            d
        }
    }
}

pub fn load() -> Config {
    load_at(&config_path())
}

pub fn save_at(path: &std::path::Path, cfg: &Config) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_vec_pretty(cfg).expect("config serializes");
    let tmp = path.with_extension("json.tmp");
    match std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, path)) {
        Ok(_) => {}
        // Windows rename can race an open file handle from our own watcher: retry once, then plain write
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_millis(120));
            if std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, path)).is_err() {
                let _ = std::fs::write(path, &json);
            }
        }
    }
}

/// Settings' whole-config save — routed through the lock.
pub fn save(cfg: &Config) {
    let owned = cfg.clone();
    update_config(move |_| owned);
}

pub fn spawn_config_watcher<F: Fn(&Config) + Send + 'static>(app: tauri::AppHandle, on_change: F) {
    use notify::Watcher;
    use tauri::Emitter;

    std::thread::spawn(move || {
        let path = config_path();
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        let (tx, rx) = std::sync::mpsc::channel();
        let Ok(mut watcher) = notify::recommended_watcher(tx) else { return };
        if watcher.watch(&dir, notify::RecursiveMode::NonRecursive).is_err() { return; }
        loop {
            // block until an event arrives, then absorb the tail of the write burst
            if rx.recv().is_err() { break; }
            while rx.recv_timeout(std::time::Duration::from_millis(300)).is_ok() {}
            std::thread::sleep(std::time::Duration::from_millis(300));
            let cfg = load();
            on_change(&cfg);
            let _ = app.emit("config-changed", &cfg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("aiusage-test-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        let c = load_at(&tmp_path("missing"));
        assert_eq!(c.poll_seconds, 300);
        assert_eq!(c.providers.len(), 4);
        assert!(c.providers.iter().all(|p| p.enabled));
        assert!(c.ollama_api_key.is_empty());
    }

    #[test]
    fn load_malformed_file_returns_defaults() {
        let p = tmp_path("bad");
        std::fs::write(&p, "{not json").unwrap();
        let c = load_at(&p);
        assert_eq!(c.poll_seconds, 300);
        // file rewritten with valid defaults
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("\"pollSeconds\""));
    }

    #[test]
    fn serde_camel_case_roundtrip_preserves_fields() {
        let mut cfg = defaults();
        cfg.poll_seconds = 60;
        cfg.ollama_api_key = "k".into();
        cfg.autostart = false;
        cfg.bar_thickness = 7;
        cfg.font_size = 12;
        cfg.providers[0].float_windows = Some(vec!["5h".into()]);
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(s.contains("\"pollSeconds\""));
        assert!(s.contains("floatWindows"));
        assert!(s.contains("\"barThickness\":7"));
        assert!(s.contains("\"fontSize\":12"));
        assert!(s.contains("antigravityOauth"));
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn save_then_load_roundtrip_preserves_fields() {
        let p = tmp_path("roundtrip");
        let mut cfg = defaults();
        cfg.poll_seconds = 45;
        cfg.ollama_api_key = "ollama-secret".into();
        cfg.providers = vec![
            ProviderCfg { id: "codex".into(), enabled: true, float_windows: Some(vec!["week".into()]) },
            ProviderCfg { id: "claude".into(), enabled: false, float_windows: None },
        ];
        save_at(&p, &cfg);
        let back = load_at(&p);
        assert_eq!(back, cfg);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_unreadable_file_never_overwrites_user_config() {
        // review I6: a transient read failure (file locked by another process, AV scanner,
        // permissions loss) must NOT rewrite the user's settings with defaults
        let p = tmp_path("iolock");
        let mut cfg = defaults();
        cfg.ollama_api_key = "user-key-keep-me".into();
        cfg.poll_seconds = 77;
        save_at(&p, &cfg);
        // hold the file open without share-read, like another writer would on Windows
        let lock_handle = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&p)
            .expect("opens with no sharing");
        let cfg_back = load_at(&p);
        assert!(cfg_back.ollama_api_key.is_empty(), "transient failure yields defaults in memory");
        drop(lock_handle);
        let after = load_at(&p);
        assert_eq!(after, cfg, "IO failure must keep the stored file untouched");
        assert_eq!(after.ollama_api_key, "user-key-keep-me");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn load_malformed_file_backs_up_before_rewriting() {
        // review I6: malformed config gets defaults AND a .bak so nothing is lost
        let p = tmp_path("bak");
        std::fs::write(&p, "{oops").unwrap();
        load_at(&p);
        let bak = p.with_extension("json.bak");
        assert!(bak.exists(), ".bak must preserve the malformed original");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "{oops");
        assert!(std::fs::read_to_string(&p).unwrap().contains("\"pollSeconds\""));
        std::fs::remove_file(&bak).ok();
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn update_config_applies_and_persists() {
        let p = tmp_path("update");
        update_config_at(&p, |mut c| {
            c.poll_seconds = 33;
            c
        });
        let back = load_at(&p);
        assert_eq!(back.poll_seconds, 33);
        std::fs::remove_file(&p).ok();
    }
}
