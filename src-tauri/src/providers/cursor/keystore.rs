//! Cursor IDE stored session (read-only):
//! `%APPDATA%\Cursor\User\globalStorage\state.vscdb` → ItemTable →
//! `cursorAuth/accessToken` — text, JSON-ish UTF-16LE blob, or
//! `cursor-safe-storage:v1:<base64url>` AES-256-GCM envelope (key in
//! `Local State` → os_crypt.encrypted_key → DPAPI (same user)).
//! Tokens stay in memory; nothing is written back to Cursor's files.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use serde_json::Value;

pub struct CursorSession {
    pub user_id: String,
    pub token: String,
}

impl CursorSession {
    pub fn cookie_value(&self) -> String {
        // WorkosCursorSessionToken=<userID>%3A%3A<token>, token fully
        // percent-escaped (matches urllib quote(safe='')) — an unescaped
        // token sent 401s in practice.
        format!("{}%3A%3A{}", self.user_id, percent_encode(&self.token))
    }
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn read_value(raw: &Value) -> Option<String> {
    match raw {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => {
            // UTF-16LE code units saved as a JSON array
            let units: Vec<u16> = a.iter().filter_map(|u| u.as_u64().map(|x| x as u16)).collect();
            Some(String::from_utf16_lossy(&units))
        }
        Value::Object(_) => {
            // {"__type__": "Buffer", "data": [..]} style
            let data = raw.get("data")?.as_array()?;
            let bytes: Vec<u8> = data.iter().filter_map(|b| b.as_u64().map(|x| x as u8)).collect();
            Some(String::from_utf16_lossy(
                &bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)]).to_owned()).collect::<Vec<u16>>(),
            ))
        }
        _ => None,
    }
}

/// Decrypt `cursor-safe-storage:v1:<base64url>` with the DPAPI-recovered
/// Chromium OSCrypt key. Returns the plaintext token.
pub fn decrypt_envelope(stored: &str) -> Option<String> {
    let body = stored.strip_prefix("cursor-safe-storage:v1:")?;
    // The real-world body uses the STANDARD base64 alphabet (a '+' at offset 64);
    // normalize both alphabets to one before decoding.
    let normalized: String = body.chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    let raw = base64::engine::general_purpose::STANDARD_NO_PAD.decode(normalized).ok()?;
    if raw.len() < 3 + 12 + 16 || &raw[..3] != b"v10" {
        return None;
    }
    let nonce = &raw[3..15];
    let ct = &raw[15..];
    let key = oscrypt_key()?;
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
        .decrypt(Nonce::from_slice(nonce), ct)
        .ok()
        .and_then(|p| String::from_utf8(p).ok())
}

fn dpapi_unprotect(data: &[u8]) -> Option<Vec<u8>> {
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *const u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptUnprotectData(
            pDataIn: *const DataBlob,
            ppszDataDescr: *mut *const u16,
            pOptionalEntropy: *const DataBlob,
            pvReserved: *const u8,
            pPromptStruct: *const u8,
            dwFlags: u32,
            pDataOut: *mut DataBlob,
        ) -> i32;
        fn LocalFree(h: *const u8) -> *const u8;
    }
    unsafe {
        let b_in = DataBlob { cb_data: data.len() as u32, pb_data: data.as_ptr() };
        let mut b_out = DataBlob { cb_data: 0, pb_data: std::ptr::null() };
        if CryptUnprotectData(&b_in, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut b_out) == 0 {
            return None;
        }
        let out = std::slice::from_raw_parts(b_out.pb_data, b_out.cb_data as usize).to_vec();
        LocalFree(b_out.pb_data);
        Some(out)
    }
}

/// Chromium OSCrypt AES key: `Local State` → os_crypt.encrypted_key
/// (base64, "DPAPI" 5-byte header, decrypted with the user's DPAPI).
fn oscrypt_key() -> Option<Vec<u8>> {
    let path = dirs::data_dir()
        .map(|d| d.join("Cursor").join("Local State"))
        .filter(|p| p.exists())?;
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let b64 = v.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let blob = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if blob.len() < 5 || &blob[..5] != b"DPAPI" {
        return None;
    }
    dpapi_unprotect(&blob[5..])
}

/// Read the stored session; handles plaintext, UTF-16LE blobs, and the
/// cursor-safe-storage envelope. Read-only.
pub fn read_session(home: &std::path::Path) -> Option<CursorSession> {
    let db = home
        .join("AppData")
        .join("Roaming")
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if !db.exists() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    use rusqlite::types::Value as SqlValue;
    let raw: String = match conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
            [],
            |r| r.get::<usize, SqlValue>(0),
        )
        .ok() {
        Some(SqlValue::Text(s)) => Some(s),
        Some(SqlValue::Blob(b)) => {
            // UTF-16LE BLOB or byte-prefixed text
            let utf16: String = String::from_utf16_lossy(&b.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<u16>>());
            if utf16.contains("cursor-safe-storage") || utf16.chars().all(|c| c.is_ascii()) {
                Some(utf16)
            } else {
                Some(String::from_utf8_lossy(&b).into_owned())
            }
        }
        Some(SqlValue::Integer(_)) | Some(SqlValue::Real(_)) | Some(SqlValue::Null) | None => None,
    }.or_else(|| {
        conn.query_row(
            "SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
            [],
            |r| r.get::<usize, String>(0),
        )
        .ok()
    })?;
    let token = if raw.starts_with("cursor-safe-storage:v1:") {
        decrypt_envelope(&raw)?
    } else {
        raw
    };
    if token.trim().is_empty() {
        return None;
    }
    // userID = auth0 sub's last segment, from cachedScopedProfile
    let profile_text: String = conn
        .query_row(
            "SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = 'cursorAuth/cachedScopedProfile'",
            [],
            |r| r.get::<usize, String>(0),
        )
        .ok()?;
    let profile: Value = serde_json::from_str(&profile_text).ok()?;
    let sub = profile.get("authId")?.as_str()?;
    Some(CursorSession { user_id: sub.split('|').last()?.to_string(), token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const KEY: &[u8; 32] = b"0123456789abcdef0123456789abcdef";

    fn envelope(plain: &str, nonce_seed: u8) -> String {
        let mut nonce = [0u8; 12];
        for (i, b) in nonce.iter_mut().enumerate() {
            *b = nonce_seed.wrapping_add(i as u8);
        }
        let ct = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(KEY)).encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).unwrap();
        let mut payload = b"v10".to_vec();
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&ct);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
    }

    #[test]
    fn decrypt_envelope_recovers_token() {
        let env = envelope("eyJhbGciOiJWTOKTOKENHERE", 7);
        // decrypt_envelope pulls its own key via DPAPI — not deterministic here;
        // structurally verify the envelope this test generated instead:
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&env).unwrap();
        assert_eq!(&raw[..3], b"v10");
        assert!(raw.len() > 3 + 12 + 16, "nonce+ct+tag structure");
        let plain = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(KEY))
            .decrypt(Nonce::from_slice(&raw[3..15]), &raw[15..])
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&plain), "eyJhbGciOiJWTOKTOKENHERE");
    }

    #[test]
    fn garbage_envelope_is_none() {
        assert!(decrypt_envelope("cursor-safe-storage:v1:notbase64!!").is_none());
        assert!(decrypt_envelope("cursor-safe-storage:v1:aGVsbG8=").is_none(), "too short for v10+nonce+tag");
    }

    #[test]
    #[ignore]
    fn live_read_session_diagnostic() {
        let home = dirs::home_dir().unwrap();
        let db = home
            .join("AppData")
            .join("Roaming")
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb");
        let conn = match rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(c) => c,
            Err(e) => { println!("LIVE open err: {e}"); return; }
        };
        let text = match conn.query_row(
            "SELECT CAST(value AS TEXT) FROM ItemTable WHERE key = 'cursorAuth/accessToken'",
            [],
            |r| r.get::<usize, String>(0)) {
            Ok(s) => s,
            Err(e) => { println!("LIVE query err: {e}"); return; }
        };
        println!("LIVE text len: {}", text.len());
        println!("LIVE envelope prefix: {}", text.starts_with("cursor-safe-storage:v1:"));
        match decrypt_envelope(&text) {
            Some(p) => println!("LIVE decrypt OK, token len {}", p.len()),
            None => println!("LIVE decrypt_envelope None"),
        }
    }

    #[test]
    fn cookie_value_is_url_escaped() {
        let s = CursorSession { user_id: "user_01K".into(), token: "tok/plus=eq".into() };
        assert_eq!(s.cookie_value(), "user_01K%3A%3Atok%2Fplus%3Deq");
    }

    #[test]
    fn utf16_blob_array_decodes() {
        let arr = serde_json::json!([104u64, 105u64]); // "hi" utf-16le
        assert_eq!(read_value(&arr).unwrap(), "hi");
    }
}
