//! Antigravity stores its own Google OAuth tokens inside the IDE's global
//! state keystore (`state.vscdb`, ItemTable → `antigravityUnifiedStateSync.oauthToken`)
//! as a nested-base64 protobuf. This extracts the token pair read-only and
//! refreshes it with the *IDE's own* OAuth client (discovered from the install).
//! Tokens stay in memory/cache only — nothing is written back.

pub struct MinedTokens {
    pub access: String,
    pub refresh: String,
}

/// Decode the stored blob: outer base64 → one printable (inner base64) run →
/// contains a `ya29.…` access token and a `1//…` refresh token.
pub fn decode_oauth_blob(value: &str) -> Option<MinedTokens> {
    let blob = b64_decode(value)?;
    for layer in &inner_layers(&blob) {
        let access = find_pref(layer, b"ya29.", 40, 400);
        let refresh = find_pref(layer, b"1//", 20, 200);
        if let (Some(a), Some(r)) = (access, refresh) {
            return Some(MinedTokens { access: a, refresh: r });
        }
    }
    None
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .ok()
}

/// The blob nests one base64 payload deeper; search the raw blob and its first
/// inner base64 layer.
fn inner_layers(blob: &[u8]) -> Vec<Vec<u8>> {
    let mut layers = vec![blob.to_vec()];
    if let Some(w) = blob
        .windows(40)
        .find(|w| w.iter().all(|b| b.is_ascii_graphic() && *b != b' '))
    {
        // expand the full printable run around that window
        let mid = (w.as_ptr() as usize) - (blob.as_ptr() as usize);
        let mut start = mid;
        while start > 0 && blob[start - 1].is_ascii_graphic() && blob[start - 1] != b' ' {
            start -= 1;
        }
        let mut end = mid + w.len();
        while end < blob.len() && (blob[end].is_ascii_graphic() && blob[end] != b' ') {
            end += 1;
        }
        if let Ok(s) = std::str::from_utf8(&blob[start..end]) {
            if let Some(inner) = b64_decode(s) {
                layers.push(inner);
            }
        }
    }
    layers
}

fn find_pref(hay: &[u8], prefix: &[u8], min_len: usize, max_len: usize) -> Option<String> {
    let mut from = 0;
    while let Some(i) = find(hay, prefix, from) {
        let mut end = i + prefix.len();
        while end - i < max_len && hay.get(end).map(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_').unwrap_or(false) {
            end += 1;
        }
        if end - i >= min_len {
            return Some(String::from_utf8_lossy(&hay[i..end]).into_owned());
        }
        from = i + prefix.len();
    }
    None
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// Mint a fresh access token with the IDE's own client.
pub async fn refresh_access_token(
    http: &reqwest::Client,
    client: &super::client_discovery::OAuthClient,
    refresh_token: &str,
) -> Result<(String, u64), String> {
    let resp = http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client.client_id.as_str()),
            ("client_secret", client.client_secret.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("refresh failed: {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let access = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or("no access_token")?
        .to_string();
    let expires_in = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600);
    Ok((access, crate::providers::now_epoch_ms() + expires_in * 1000))
}

/// Read the IDE keystore (read-only).
pub fn read_stored_tokens(home: &std::path::Path) -> Option<MinedTokens> {
    let db_path = home
        .join("AppData")
        .join("Roaming")
        .join("Antigravity")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    let conn = rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let value: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'antigravityUnifiedStateSync.oauthToken'",
            [],
            |r| r.get(0),
        )
        .ok()?;
    decode_oauth_blob(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn decode_oauth_blob_finds_both_tokens_nested() {
        // observed shape: outer base64 → protobuf with one printable run that is
        // base64 of the layer holding raw ya29./1// token strings
        let inner = "\u{5}ya29.a0AToNOZZyxFXvWl9Kp2x1YhUuZ3abcdEFGH0123456789_-xYxYxYxYxYxYxYxYxYxY\u{6}1//0aBcDeFgHiJkLmNoPqRsTuVwXyZ12";
        let run = base64::engine::general_purpose::STANDARD.encode(inner);
        let blob = base64::engine::general_purpose::STANDARD.encode(format!("\x05{run}"));
        let t = decode_oauth_blob(&blob).expect("tokens found");
        assert!(t.access.starts_with("ya29.a0"));
        assert!(t.refresh.starts_with("1//0a"));
    }

    #[test]
    fn decode_garbage_is_none() {
        assert!(decode_oauth_blob("aGVsbG8=").is_none());
        assert!(decode_oauth_blob("!!!").is_none());
    }

    #[test]
    fn access_without_refresh_is_none() {
        let inner = "\u{5}ya29.a0AToNOZZyxFXvWl9Kp2x1YhUuZ3abcdEFGH0123456789_-xYxYxYxYxYxYxYxYxYxY\u{6}notarefreshtoken";
        let run = base64::engine::general_purpose::STANDARD.encode(inner);
        let blob = base64::engine::general_purpose::STANDARD.encode(run);
        assert!(decode_oauth_blob(&blob).is_none());
    }
}
