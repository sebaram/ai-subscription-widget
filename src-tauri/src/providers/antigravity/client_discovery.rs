//! Discover Antigravity's bundled Google OAuth client from installed app
//! files, mirroring upstream's policy: files ≤64 MiB; text artifacts only
//! near the `oauthClient.js` module marker; binaries whole; exactly ONE
//! client id and ONE secret pairing, otherwise refuse. The secret is never
//! logged; `ANTIGRAVITY_OAUTH_CLIENT_ID`/`_SECRET` override everything.

const MAX_CLIENT_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const CLIENT_ID_MARKER: &[u8] = b".apps.googleusercontent.com";
const CLIENT_SECRET_MARKER: &[u8] = b"GOCSPX-";
const CLIENT_SECRET_LEN: usize = 36; // "GOCSPX-" + 28
const OAUTH_CLIENT_MODULE_MARKER: &[u8] = b"vs/platform/cloudCode/common/oauthClient.js";
const TEXT_ARTIFACT_WINDOW: usize = 4000;

pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: String,
}

fn is_id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack.get(from..)?;
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = find(haystack, needle, from) {
        out.push(i);
        from = i + needle.len();
    }
    out
}

fn unique_secrets(data: &[u8]) -> Vec<String> {
    find_all(data, CLIENT_SECRET_MARKER)
        .into_iter()
        .filter_map(|i| {
            let end = i + CLIENT_SECRET_LEN;
            let text = std::str::from_utf8(data.get(i..end)?).ok()?;
            // GOCSPX- + exactly 28 alphanumeric chars not followed by more such chars
            if end < data.len() && (is_id_byte(data[end]) && data[end] != b'-') {
                return None;
            }
            if text.len() == CLIENT_SECRET_LEN && text[7..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                Some(text.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Ambiguity counts RAW occurrences (upstream behavior): any second distinct
/// candidate text means several embedded clients → refuse, even if the others
/// don't fully validate.
fn raw_id_texts(data: &[u8]) -> Vec<String> {
    let mut raws: Vec<String> = Vec::new();
    for i in find_all(data, CLIENT_ID_MARKER) {
        let floor = i.saturating_sub(512);
        let mut start = i;
        while start > floor && is_id_byte(data[start - 1]) {
            start -= 1;
        }
        if let Ok(text) = std::str::from_utf8(&data[start..i]) {
            if !text.is_empty() && !raws.contains(&text.to_string()) {
                raws.push(text.to_string());
            }
        }
    }
    raws
}

/// The pairing rule: exactly one distinct id AND one distinct secret in scope.
pub fn parse_client_from_artifact(data: &[u8]) -> Option<OAuthClient> {
    let scope = if std::str::from_utf8(data).is_ok() {
        match find(data, OAUTH_CLIENT_MODULE_MARKER, 0) {
            Some(i) => &data[i.saturating_sub(2048)..((i + TEXT_ARTIFACT_WINDOW).min(data.len()))],
            None => data,
        }
    } else {
        data
    };
    let mut raws = raw_id_texts(scope);
    if raws.len() != 1 {
        return None;
    }
    let id = validate_id(raws.remove(0))?;
    let mut secrets = unique_secrets(scope);
    if secrets.len() == 1 {
        Some(OAuthClient {
            client_id: id,
            client_secret: secrets.remove(0),
        })
    } else {
        None
    }
}

fn validate_id(raw_text: String) -> Option<String> {
    let dash = raw_text.find('-')?;
    let prefix = &raw_text[..dash];
    let random = &raw_text[dash + 1..];
    if prefix.len() < 6 || !prefix.chars().all(|c| c.is_ascii_digit()) || random.len() < 8 {
        return None;
    }
    Some(format!("{raw_text}.apps.googleusercontent.com"))
}

fn candidate_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let local = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    if let Some(local) = local {
        // upstream's Windows layout for the desktop IDE
        let resources = local.join("Programs").join("Antigravity").join("resources");
        paths.push(resources.join("app.asar"));
        paths.push(
            resources
                .join("app")
                .join("extensions")
                .join("antigravity")
                .join("bin")
                .join("language_server_windows_x64.exe"),
        );
        paths.push(resources.join("bin").join("language_server.exe"));
        // standalone agy bundle (this machine ships it separately)
        paths.push(local.join("agy").join("bin").join("agy.exe"));
    }
    // env overrides win
    if let Ok(p) = std::env::var("ANTIGRAVITY_CLIENT_ARTIFACT") {
        paths.push(std::path::PathBuf::from(p));
    }
    paths
}

pub fn configured_client() -> Option<OAuthClient> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    match (env("ANTIGRAVITY_OAUTH_CLIENT_ID"), env("ANTIGRAVITY_OAUTH_CLIENT_SECRET")) {
        (Some(id), Some(secret)) => Some(OAuthClient { client_id: id.trim().to_string(), client_secret: secret.trim().to_string() }),
        _ => None,
    }
}

pub fn discover_client() -> Option<OAuthClient> {
    for path in candidate_paths() {
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() || meta.len() > MAX_CLIENT_ARTIFACT_BYTES {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else { continue };
        if let Some(client) = parse_client_from_artifact(&data) {
            return Some(client);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID_A: &str = "1234567890-abcDEF-_xyz.apps.googleusercontent.com";

    const TEST_SECRET: &str = "GOCSPX-abcdefghij0123456789klmnoqrst"; // 28 after prefix, Google's exact length

    fn text_artifact(client: &str) -> Vec<u8> {
        let mut a = Vec::new();
        a.extend_from_slice(b"// filler module code\nconst x = 1;\n");
        a.extend_from_slice(OAUTH_CLIENT_MODULE_MARKER);
        a.extend_from_slice(b"\nclient_id = \"");
        a.extend_from_slice(client.as_bytes());
        a.extend_from_slice(b"\";\nclient_secret = \"");
        a.extend_from_slice(TEST_SECRET.as_bytes());
        a.extend_from_slice(b"\";\n");
        a
    }

    #[test]
    fn text_artifact_pairs_marker_scoped_client() {
        let a = text_artifact(ID_A);
        let c = parse_client_from_artifact(&a).expect("one clean pairing");
        assert_eq!(c.client_id, ID_A);
        assert_eq!(c.client_secret, TEST_SECRET);
    }

    #[test]
    fn two_ids_is_ambiguous_and_refused() {
        let id_b = "0987654321-zzz.apps.googleusercontent.com";
        let mut a = text_artifact(ID_A);
        a.extend_from_slice(id_b.as_bytes());
        assert!(parse_client_from_artifact(&a).is_none());
    }

    #[test]
    fn missing_secret_refused() {
        let mut a = text_artifact(ID_A);
        let cut = a.windows(TEST_SECRET.len()).position(|w| w == TEST_SECRET.as_bytes()).unwrap();
        a.drain(cut..cut + TEST_SECRET.len());
        a.extend_from_slice(b"notasecretformat");
        assert!(parse_client_from_artifact(&a).is_none());
    }

    #[test]
    fn non_utf8_is_scanned_whole() {
        let mut a = vec![0xFF, 0xFE, 0x00, 0x01]; // invalid utf8 → binary path
        a.extend_from_slice(b"token_file=\"");
        a.extend_from_slice(ID_A.as_bytes());
        a.extend_from_slice(b"\";secret:\"");
        a.extend_from_slice(TEST_SECRET.as_bytes());
        a.extend_from_slice(b"\"");
        let c = parse_client_from_artifact(&a).expect("binary-scope pair");
        assert_eq!(c.client_id, ID_A);
        assert_eq!(c.client_secret, TEST_SECRET);
    }
}
