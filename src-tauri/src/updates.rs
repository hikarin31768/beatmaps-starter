use tauri::{AppHandle, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{
    emit_progress,
    state::{issue, Result},
};

const DEFAULT_UPDATE_ENDPOINT: &str =
    "https://github.com/hikarin31768/beatmaps-starter/releases/latest/download/latest.json";
const DEFAULT_UPDATE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM4Mjc3MTAwMDBDNjhDODQKUldTRWpNWUFBSEVuT0FOcGM4RFI2bWJBdmJZd29WUEdYUmVUN2VOUE9OcXV2eDRNS3UzSmdyaEQK";

async fn available_update(app: &AppHandle) -> Result<Option<Update>> {
    let endpoint = option_env!("BEATMAPS_UPDATE_ENDPOINT")
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(DEFAULT_UPDATE_ENDPOINT);
    let url = Url::parse(endpoint).map_err(|e| issue("更新检查地址无效", e))?;
    if url.scheme() != "https" {
        return Err("更新检查地址必须使用 HTTPS".into());
    }
    let pubkey = option_env!("BEATMAPS_UPDATE_PUBKEY")
        .filter(|key| !key.trim().is_empty())
        .unwrap_or(DEFAULT_UPDATE_PUBKEY);
    let updater = app
        .updater_builder()
        .pubkey(pubkey)
        .endpoints(vec![url])
        .and_then(|builder| builder.build())
        .map_err(|e| issue("无法配置更新", e))?;
    updater.check().await.map_err(|e| issue("检查更新失败", e))
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
