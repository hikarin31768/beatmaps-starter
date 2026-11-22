use tauri::{AppHandle, Url};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::{
    emit_progress,
    state::{issue, Result},
};

const DEFAULT_UPDATE_ENDPOINT: &str =
    "https://github.com/hikarin31768/beatmaps-starter/releases/latest/download/latest.json";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/hikarin31768/beatmaps-starter/releases/latest";

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
                .timeout(std::time::Duration::from_secs(10))
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
pub async fn check_update(app: AppHandle) -> Result<Option<String>> {
    Ok(available_update(&app).await?.map(|update| update.version))
}

#[tauri::command]
pub async fn install_update(app: AppHandle, version: String) -> Result<()> {
    let update = available_update(&app)
        .await?
        .filter(|update| update.version == version)
        .ok_or("可用版本已变化，请重新检查更新")?;
    update
        .download_and_install(
            |_, _| {},
            || emit_progress(&app, "update", "正在校验并安装更新"),
        )
        .await
        .map_err(|e| issue("安装更新失败", e))?;
    app.restart();
}
