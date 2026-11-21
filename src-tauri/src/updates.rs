use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::state::AppState;

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/selfsecuredlab/lyrics-status/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    if !state.settings_snapshot().update.enable_autoupdate {
        return Ok(None);
    }

    let response = state
        .http
        .client
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let release: Release = response
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .await
        .map_err(|error| error.to_string())?;

    if release.draft || release.prerelease {
        return Ok(None);
    }

    let Some(tag) = release.tag_name.strip_prefix('v') else {
        return Ok(None);
    };
    let Ok(version) = semver::Version::parse(tag) else {
        return Ok(None);
    };

    let is_newer = version.cmp_precedence(&app.package_info().version).is_gt();
    Ok((version.pre.is_empty() && is_newer).then(|| version.to_string()))
}
