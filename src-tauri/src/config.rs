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
    }
}

pub fn config_path() -> PathBuf {
    // %APPDATA%\ai-usage\config.json
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("ai-usage").join("config.json")
}

pub fn load_at(path: &std::path::Path) -> Config {
    match std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Config>(&b).ok()) {
        Some(cfg) => cfg,
        None => {
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
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(cfg).expect("config serializes");
    match std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, path)) {
        Ok(_) => {}
        // Windows rename can race an open file handle from our own watcher: keep the tmp write as fallback
        Err(_) => {
            let _ = std::fs::write(path, &json);
        }
    }
}

pub fn save(cfg: &Config) {
    save_at(&config_path(), cfg);
}

pub fn spawn_config_watcher(app: tauri::AppHandle) {
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
            let _ = app.emit("config-changed", &cfg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        cfg.providers[0].float_windows = Some(vec!["5h".into()]);
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(s.contains("\"pollSeconds\""));
        assert!(s.contains("floatWindows"));
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back, cfg);
    }
}
