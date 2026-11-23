use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::{ipc::Channel, AppHandle, Url};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::state::{issue, Result};

const DEFAULT_UPDATE_ENDPOINT: &str =
    "https://github.com/hikarin31768/beatmaps-starter/releases/latest/download/latest.json";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/hikarin31768/beatmaps-starter/releases/latest";
const PROJECT_URL: &str = "https://github.com/hikarin31768/beatmaps-starter";

#[derive(Serialize)]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum UpdateProgress {
    Downloading { received: u64, total: Option<u64> },
    Installing,
}

#[derive(Deserialize)]
struct GithubRelease {
    body: Option<String>,
    draft: bool,
    prerelease: bool,
}

async fn github_release_notes(installer: &Url) -> Option<String> {
    if installer.scheme() != "https" || installer.host_str() != Some("github.com") {
        return None;
    }
    let segments: Vec<_> = installer.path_segments()?.collect();
    let ["hikarin31768", "beatmaps-starter", "releases", "download", tag, _] = segments.as_slice()
    else {
        return None;
    };
    if tag.is_empty()
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let release: GithubRelease = reqwest::Client::new()
        .get(format!(
            "https://api.github.com/repos/hikarin31768/beatmaps-starter/releases/tags/{tag}"
        ))
        .header(reqwest::header::USER_AGENT, "Beatmaps-Starter")
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    release.body.filter(|body| !body.trim().is_empty())
}

async fn available_update(app: &AppHandle) -> Result<Option<Update>> {
    let endpoint = option_env!("BEATMAPS_UPDATE_ENDPOINT")
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(DEFAULT_UPDATE_ENDPOINT);
    let url = Url::parse(endpoint).map_err(|e| issue("更新检查地址无效", e))?;
    if url.scheme() != "https" {
        return Err("更新检查地址必须使用 HTTPS".into());
    }
    let mut builder = app
        .updater_builder()
        .version_comparator(|current, release| {
            release.version.pre.is_empty() && release.version > current
        })
        .endpoints(vec![url])
        .map_err(|e| issue("无法配置更新", e))?;
    if let Some(pubkey) = option_env!("BEATMAPS_UPDATE_PUBKEY")
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        builder = builder.pubkey(pubkey);
    }
    let updater = builder.build().map_err(|e| issue("无法配置更新", e))?;
    match updater.check().await {
        Ok(update) => Ok(update),
        Err(UpdaterError::ReleaseNotFound) if endpoint == DEFAULT_UPDATE_ENDPOINT => {
            // GitHub also returns 404 when a formal release lacks latest.json.
            let response = reqwest::Client::new()
                .get(LATEST_RELEASE_API)
                .header(reqwest::header::USER_AGENT, "Beatmaps-Starter")
                .timeout(Duration::from_secs(10))
                .send()
                .await
                .map_err(|e| issue("无法查询 GitHub 正式版", e))?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            response
                .error_for_status()
                .map_err(|e| issue("无法查询 GitHub 正式版", e))?;
            Err("GitHub 正式版的更新清单不可用".into())
        }
        Err(e) => Err(issue("检查更新失败", e)),
    }
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>> {
    let Some(update) = available_update(&app).await? else {
        return Ok(None);
    };
    let placeholder = format!("Beatmaps Starter {}", update.version);
    let notes = update
        .body
        .filter(|body| !body.trim().is_empty() && body.trim() != placeholder);
    let notes = match notes {
        Some(notes) => Some(notes),
        None => github_release_notes(&update.download_url).await,
    };
    Ok(Some(UpdateInfo {
        version: update.version,
        notes,
    }))
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    version: String,
    on_event: Channel<UpdateProgress>,
) -> Result<()> {
    let update = available_update(&app)
        .await?
        .filter(|update| update.version == version)
        .ok_or("可用版本已变化，请重新检查更新")?;
    let mut received = 0u64;
    let mut last_sent: Option<Instant> = None;
    update
        .download_and_install(
            |chunk, total| {
                received += chunk as u64;
                let now = Instant::now();
                if last_sent
                    .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(100))
                {
                    let _ = on_event.send(UpdateProgress::Downloading { received, total });
                    last_sent = Some(now);
                }
            },
            || {
                let _ = on_event.send(UpdateProgress::Installing);
            },
        )
        .await
        .map_err(|e| issue("安装更新失败", e))?;
    app.restart();
}

#[tauri::command]
pub fn open_project_source() -> Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(PROJECT_URL)
        .spawn()
        .map(|_| ())
        .map_err(|error| issue("无法打开项目主页", error))
}
