//! Shared HTTP client plus the Discord request/retry logic from
//! src/DiscordApiClient.ts and src/ServiceEvents.ts. A single reqwest client is
//! reused for every outbound call (Discord, Spotify, lyric sources).

use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::datastore::now_ms;

pub struct Http {
    pub client: reqwest::Client,
    global_blocked_until: AtomicI64,
    app: OnceLock<AppHandle>,
}

impl Http {
    pub fn new() -> Http {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) LyricsStatus")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Http {
            client,
            global_blocked_until: AtomicI64::new(0),
            app: OnceLock::new(),
        }
    }

    pub fn set_app(&self, app: AppHandle) {
        let _ = self.app.set(app);
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        if let Some(app) = self.app.get() {
            let _ = app.emit(event, payload);
        }
    }

    #[allow(dead_code)]
    pub fn clear_global_block(&self) {
        self.global_blocked_until.store(0, Ordering::SeqCst);
    }

    /// Perform a Discord request with the same 429 retry + global-block behavior
    /// as DiscordApiClient.discordRequest. `source` tags the emitted rate-limit
    /// events ("discord-status" | "spotify-connection" | "auth").
    pub async fn discord_request(
        &self,
        method: reqwest::Method,
        url: &str,
        token: Option<&str>,
        content_type: Option<&str>,
        body: Option<String>,
        source: &str,
        max_attempts: u32,
        timeout_ms: u64,
    ) -> Result<reqwest::Response, String> {
        let max_attempts = max_attempts.max(1);
        let mut was_rate_limited = false;

        for attempt in 0..max_attempts {
            let wait_for = self.global_blocked_until.load(Ordering::SeqCst) - now_ms();
            if wait_for > 0 {
                tokio::time::sleep(Duration::from_millis(wait_for as u64)).await;
            }

            let mut req = self
                .client
                .request(method.clone(), url)
                .timeout(Duration::from_millis(timeout_ms));
            if let Some(t) = token {
                req = req.header("Authorization", t);
            }
            if let Some(ct) = content_type {
                req = req.header("Content-Type", ct);
            }
            if let Some(ref b) = body {
                req = req.body(b.clone());
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => return Err(format!("request error: {e}")),
            };

            let status = response.status().as_u16();
            let is_last = attempt == max_attempts - 1;

            if status != 429 || is_last {
                if response.status().is_success() && was_rate_limited {
                    self.emit(
                        "discord:rate-limit-cleared",
                        json!({ "source": source }),
                    );
                }
                return Ok(response);
            }

            was_rate_limited = true;
            let (delay_ms, global) = retry_delay(response, attempt).await;
            if global {
                let target = now_ms() + delay_ms;
                let cur = self.global_blocked_until.load(Ordering::SeqCst);
                if target > cur {
                    self.global_blocked_until.store(target, Ordering::SeqCst);
                }
            }
            self.emit(
                "discord:rate-limit",
                json!({
                    "source": source,
                    "retryAfterMs": delay_ms,
                    "retryAt": now_ms() + delay_ms,
                    "attempt": attempt + 1,
                    "maxAttempts": max_attempts,
                    "global": global,
                }),
            );
            tokio::time::sleep(Duration::from_millis(delay_ms as u64)).await;
        }

        Err("Discord request retry loop ended unexpectedly".to_string())
    }
}

/// Mirror of DiscordApiClient.retryDelay — prefer the Retry-After header, fall
/// back to the JSON body, else exponential backoff; capped at 120s.
async fn retry_delay(response: reqwest::Response, attempt: u32) -> (i64, bool) {
    let header_retry = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<f64>().ok());

    let mut retry_after = header_retry;
    let mut global = false;

    if let Ok(text) = response.text().await {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(ra) = val.get("retry_after").and_then(|v| v.as_f64()) {
                retry_after = Some(ra);
            }
            global = val.get("global").and_then(|v| v.as_bool()) == Some(true);
        }
    }

    let delay_ms = match retry_after {
        Some(ra) if ra > 0.0 => ((ra * 1000.0).ceil() as i64).min(120_000),
        _ => (1000_i64.saturating_mul(2_i64.pow(attempt.min(16)))).min(120_000),
    };

    (delay_ms, global)
}
