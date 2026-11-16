//! Port of the discord:extract-token handler in src/main.ts. Detects the local
//! Discord token for the user's own account — plaintext, or the modern
//! DPAPI + AES-256-GCM encrypted blob — then validates it against Discord.
//!
//! The master-key DPAPI unprotect is delegated to PowerShell exactly as the
//! original did, so no extra native crypto dependency is pulled in for it.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use once_cell::sync::Lazy;
use regex::bytes::Regex as BytesRegex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::http::Http;

static RE_PLAIN: Lazy<Vec<BytesRegex>> = Lazy::new(|| {
    vec![
        BytesRegex::new(r"[A-Za-z0-9_-]{24,28}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{25,110}").unwrap(),
        BytesRegex::new(r"mfa\.[A-Za-z0-9_-]{80,100}").unwrap(),
    ]
});
static RE_ENC: Lazy<BytesRegex> =
    Lazy::new(|| BytesRegex::new(r"dQw4w9WgXcQ:([A-Za-z0-9+/=]+)").unwrap());

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn discord_dirs() -> Vec<(String, PathBuf)> {
    let roaming = match dirs::config_dir() {
        Some(d) => d,
        None => return vec![],
    };
    [
        ("Discord", "discord"),
        ("Discord Canary", "discordcanary"),
        ("Discord PTB", "discordptb"),
        ("Discord PTB (Alt)", "DiscordPTB"),
    ]
    .into_iter()
    .map(|(name, dir)| (name.to_string(), roaming.join(dir)))
    .collect()
}

/// Read a (possibly locked) store file: try directly, then via a temp copy.
fn read_lenient(path: &Path) -> Option<Vec<u8>> {
    if let Ok(bytes) = std::fs::read(path) {
        return Some(bytes);
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("x");
    let tmp = std::env::temp_dir().join(format!("lst_{}_{}", std::process::id(), name));
    if std::fs::copy(path, &tmp).is_ok() {
        let bytes = std::fs::read(&tmp).ok();
        let _ = std::fs::remove_file(&tmp);
        return bytes;
    }
    None
}

/// Decrypt the Chromium `os_crypt` master key from `<app>/Local State` using
/// DPAPI (CurrentUser scope) via PowerShell.
fn master_key(app_dir: &Path) -> Option<[u8; 32]> {
    let text = std::fs::read_to_string(app_dir.join("Local State")).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    let enc_key_b64 = json.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    let enc_key = b64().decode(enc_key_b64).ok()?;
    if enc_key.len() <= 5 {
        return None;
    }
    let unprotected = dpapi_unprotect(&enc_key[5..])?; // strip the "DPAPI" prefix
    if unprotected.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&unprotected);
    Some(key)
}

fn dpapi_unprotect(data: &[u8]) -> Option<Vec<u8>> {
    let input_b64 = b64().encode(data);
    let script = format!(
        "Add-Type -AssemblyName System.Security; \
         $b=[Convert]::FromBase64String('{input_b64}'); \
         $u=[Security.Cryptography.ProtectedData]::Unprotect($b,$null,'CurrentUser'); \
         [Convert]::ToBase64String($u)"
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    b64().decode(out).ok()
}

/// AES-256-GCM decrypt a `v10`-prefixed blob: [3 version][12 nonce][ct][16 tag].
fn decrypt_token(enc_b64: &[u8], key: &[u8; 32]) -> Option<String> {
    let enc = b64().decode(enc_b64).ok()?;
    if enc.len() < 3 + 12 + 16 {
        return None;
    }
    let nonce = Nonce::from_slice(&enc[3..15]);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher.decrypt(nonce, &enc[15..]).ok()?;
    String::from_utf8(plaintext).ok()
}

/// Scan every Discord install's LevelDB for token candidates (blocking I/O).
fn collect_candidates() -> (Vec<String>, Vec<String>) {
    let mut tokens: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut debug: Vec<String> = Vec::new();

    for (name, app_dir) in discord_dirs() {
        let leveldb = app_dir.join("Local Storage").join("leveldb");
        if !leveldb.exists() {
            debug.push(format!("{name}: not found"));
            continue;
        }
        let key = master_key(&app_dir);
        let mut count = 0;

        if let Ok(rd) = std::fs::read_dir(&leveldb) {
            for entry in rd.flatten() {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "ldb" && ext != "log" {
                    continue;
                }
                let content = match read_lenient(&path) {
                    Some(c) => c,
                    None => continue,
                };

                for re in RE_PLAIN.iter() {
                    for m in re.find_iter(&content) {
                        if let Ok(s) = std::str::from_utf8(m.as_bytes()) {
                            if seen.insert(s.to_string()) {
                                tokens.push(s.to_string());
                                count += 1;
                            }
                        }
                    }
                }

                if let Some(k) = &key {
                    for cap in RE_ENC.captures_iter(&content) {
                        if let Some(g) = cap.get(1) {
                            if let Some(tok) = decrypt_token(g.as_bytes(), k) {
                                if tok.contains('.') && tok.len() > 50 && seen.insert(tok.clone()) {
                                    tokens.push(tok);
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        debug.push(format!(
            "{name}: {count} candidate(s){}",
            if key.is_some() { "" } else { " (no master key)" }
        ));
    }

    (tokens, debug)
}

pub async fn extract_token(http: &Http) -> Value {
    let (candidates, debug) = tokio::task::spawn_blocking(collect_candidates)
        .await
        .unwrap_or_else(|_| (vec![], vec![]));

    for token in &candidates {
        if let Some((username, id)) = validate(http, token).await {
            return json!({
                "success": true,
                "token": token,
                "username": username,
                "id": id,
            });
        }
    }

    let mut err = String::from("No valid Discord token found.\n\nChecked:\n");
    for line in &debug {
        err.push_str(line);
        err.push('\n');
    }
    if !candidates.is_empty() {
        err.push_str(&format!(
            "\nFound {} candidate(s) but Discord rejected them.\n",
            candidates.len()
        ));
    }
    err.push_str("\nMake sure Discord desktop is installed and signed in, or paste your token manually.");
    json!({ "success": false, "error": err })
}

async fn validate(http: &Http, token: &str) -> Option<(String, String)> {
    let resp = http
        .client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let j: Value = resp.json().await.ok()?;
    let username = j
        .get("global_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| j.get("username").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let id = j.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Some((username, id))
}
