//! Port of src/Settings.ts — typed application settings with partial-merge
//! load/save semantics and an atomic write.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub cookies: String,
    #[serde(default, rename = "clientID")]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub use_external_auth_server: bool,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default = "default_redirect")]
    pub custom_redirect_uri: String,
}

fn default_redirect() -> String {
    "http://127.0.0.1:67/callback".to_string()
}

impl Default for Credentials {
    fn default() -> Self {
        Credentials {
            token: String::new(),
            cookies: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            use_external_auth_server: false,
            code: String::new(),
            refresh_token: String::new(),
            uuid: String::new(),
            custom_redirect_uri: default_redirect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewEmoji {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_emoji_name")]
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub animated: bool,
}

fn default_emoji_name() -> String {
    "🎶".to_string()
}

impl Default for ViewEmoji {
    fn default() -> Self {
        ViewEmoji {
            enabled: false,
            name: default_emoji_name(),
            id: None,
            animated: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewAdvanced {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_custom_status")]
    pub custom_status: String,
}

fn default_custom_status() -> String {
    "[{timestamp}] {lyrics}".to_string()
}

impl Default for ViewAdvanced {
    fn default() -> Self {
        ViewAdvanced {
            enabled: false,
            custom_status: default_custom_status(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBehavior {
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_transition")]
    pub transition: String,
    #[serde(default = "default_alignment")]
    pub alignment: String,
    #[serde(default = "default_max_length")]
    pub max_length: i64,
}

fn default_mode() -> String {
    "single".to_string()
}
fn default_transition() -> String {
    "fade".to_string()
}
fn default_alignment() -> String {
    "center".to_string()
}
fn default_max_length() -> i64 {
    128
}

impl Default for ViewBehavior {
    fn default() -> Self {
        ViewBehavior {
            mode: default_mode(),
            transition: default_transition(),
            alignment: default_alignment(),
            max_length: default_max_length(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct View {
    #[serde(default = "default_true")]
    pub timestamp: bool,
    #[serde(default = "default_true")]
    pub label: bool,
    #[serde(default)]
    pub emoji: ViewEmoji,
    #[serde(default)]
    pub advanced: ViewAdvanced,
    #[serde(default)]
    pub behavior: ViewBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    #[serde(default = "default_send_offset")]
    pub send_time_offset: i64,
    #[serde(default = "default_true")]
    pub enable_autooffset: bool,
    #[serde(default = "default_autooffset")]
    pub autooffset: i64,
}

fn default_send_offset() -> i64 {
    500
}
fn default_autooffset() -> i64 {
    3
}

impl Default for Timings {
    fn default() -> Self {
        Timings {
            send_time_offset: default_send_offset(),
            enable_autooffset: true,
            autooffset: default_autooffset(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct General {
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_theme() -> String {
    "oled".to_string()
}
fn default_language() -> String {
    "en".to_string()
}

impl Default for General {
    fn default() -> Self {
        General {
            auto_start: false,
            start_minimized: false,
            theme: default_theme(),
            language: default_language(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_accent_source")]
    pub accent_source: String,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    #[serde(default = "default_background_style")]
    pub background_style: String,
    #[serde(default = "default_bg_blur")]
    pub background_blur: i64,
    #[serde(default = "default_bg_opacity")]
    pub background_opacity: i64,
    #[serde(default = "default_bg_animation")]
    pub background_animation: String,
    #[serde(default = "default_anim_intensity")]
    pub animation_intensity: i64,
    #[serde(default = "default_anim_speed")]
    pub animation_speed: f64,
}

fn default_accent_source() -> String {
    "spotify".to_string()
}
fn default_accent_color() -> String {
    "#1DB954".to_string()
}
fn default_background_style() -> String {
    "solid".to_string()
}
fn default_bg_blur() -> i64 {
    40
}
fn default_bg_opacity() -> i64 {
    80
}
fn default_bg_animation() -> String {
    "none".to_string()
}
fn default_anim_intensity() -> i64 {
    60
}
fn default_anim_speed() -> f64 {
    1.2
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: default_theme(),
            accent_source: default_accent_source(),
            accent_color: default_accent_color(),
            background_style: default_background_style(),
            background_blur: default_bg_blur(),
            background_opacity: default_bg_opacity(),
            background_animation: default_bg_animation(),
            animation_intensity: default_anim_intensity(),
            animation_speed: default_anim_speed(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    #[serde(default = "default_true")]
    pub enable_autoupdate: bool,
}

impl Default for Update {
    fn default() -> Self {
        Update {
            enable_autoupdate: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AllSettings {
    #[serde(default)]
    pub credentials: Credentials,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub timings: Timings,
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub update: Update,
}

/// Recursively merge `patch` into `base` (objects merge key-by-key; any other
/// value overwrites). Mirrors the renderer sending partial settings sections.
pub fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                deep_merge(base_map.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (base_slot, patch_val) => {
            *base_slot = patch_val.clone();
        }
    }
}

impl AllSettings {
    /// Load from disk, filling any missing field from defaults.
    pub fn load(path: &PathBuf) -> AllSettings {
        let mut base = serde_json::to_value(AllSettings::default()).unwrap_or(Value::Null);
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                deep_merge(&mut base, &parsed);
            }
        }
        serde_json::from_value(base).unwrap_or_default()
    }

    /// Apply a partial settings patch (as sent by the renderer).
    pub fn apply_patch(&mut self, patch: &Value) {
        let mut base = serde_json::to_value(&*self).unwrap_or(Value::Null);
        deep_merge(&mut base, patch);
        if let Ok(merged) = serde_json::from_value::<AllSettings>(base) {
            *self = merged;
        }
    }

    /// Atomic save: write to a temp file then rename.
    pub fn save(&self, path: &PathBuf) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if std::fs::write(&tmp, text).is_ok() {
                    if std::fs::rename(&tmp, path).is_err() {
                        let _ = std::fs::remove_file(&tmp);
                    }
                }
            }
            Err(_) => {}
        }
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}
