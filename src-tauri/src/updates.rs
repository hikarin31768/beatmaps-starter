use tauri::{AppHandle, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{
    emit_progress,
    state::{issue, Result},
};

const UPDATE_ENDPOINT: Option<&str> = option_env!("BEATMAPS_UPDATE_ENDPOINT");
const UPDATE_PUBKEY: Option<&str> = option_env!("BEATMAPS_UPDATE_PUBKEY");

async fn available_update(app: &AppHandle) -> Result<Option<Update>> {
    let endpoint = UPDATE_ENDPOINT
        .filter(|url| !url.trim().is_empty())
        .ok_or("更新检查地址尚未配置")?;
    let url = Url::parse(endpoint).map_err(|e| issue("更新检查地址无效", e))?;
    if url.scheme() != "https" {
        return Err("更新检查地址必须使用 HTTPS".into());
    }
    let pubkey = UPDATE_PUBKEY
        .filter(|key| !key.trim().is_empty())
        .ok_or("更新签名公钥尚未配置")?;
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
