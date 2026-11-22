#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod discovery;
mod media;
mod platform;
mod state;
mod tempo;
mod tools;
mod updates;

use std::collections::BTreeMap;
use tauri::{AppHandle, Manager};

use state::{Platform, Result, Settings, State};

#[tauri::command]
fn load_settings(state: tauri::State<'_, State>) -> Result<Settings> {
    state.settings()
}

#[tauri::command]
fn save_settings(mut settings: Settings, state: tauri::State<'_, State>) -> Result<Settings> {
    if !settings.lead_in.is_finite() || settings.lead_in < 0.0 {
        return Err("默认音频偏移必须是大于或等于 0 的秒数".into());
    }
    settings.game_dir = settings.game_dir.trim().to_owned();
    if !settings.game_dir.is_empty() {
        settings.game_dir = discovery::game_dir(&settings.game_dir)
            .ok_or("请选择包含 Beat Saber.exe 的游戏目录")?
            .to_string_lossy()
            .into_owned();
    }
    settings.normalize_quality_preferences();
    state.save_settings(&settings)?;
    Ok(settings)
}

#[tauri::command]
fn validate_game_dir(path: String) -> Result<String> {
    discovery::game_dir(&path)
        .map(|path| path.to_string_lossy().into_owned())
        .ok_or_else(|| "请选择包含 Beat Saber.exe 的游戏目录".into())
}

#[tauri::command]
async fn discover_game() -> Option<String> {
    tauri::async_runtime::spawn_blocking(discovery::discover)
        .await
        .ok()
        .flatten()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn game_dialog_directory(
    path: String,
    state: tauri::State<'_, State>,
) -> Result<Option<String>> {
    let configured = state.settings()?.game_dir;
    tauri::async_runtime::spawn_blocking(move || {
        let typed = std::path::Path::new(path.trim().trim_matches('"'));
        let dir = if typed.is_absolute() && typed.is_dir() {
            Some(typed.to_path_buf())
        } else {
            discovery::game_dir(&configured).or_else(discovery::discover)
        };
        dir.or_else(discovery::browse_root)
            .and_then(|path| dunce::canonicalize(path).ok())
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| state::issue("无法读取游戏目录", e))
}

#[tauri::command]
fn account_names(state: tauri::State<'_, State>) -> Result<BTreeMap<Platform, String>> {
    state.account_names()
}

#[tauri::command]
async fn logout(platform: Platform, state: tauri::State<'_, State>) -> Result<()> {
    let mut draft = state.draft.lock().await;
    state.set_credential(platform, None)?;
    if draft
        .as_ref()
        .is_some_and(|draft| draft.platform() == platform)
    {
        *draft = None;
    }
    Ok(())
}

#[tauri::command]
fn open_export_folder(path: String, state: tauri::State<'_, State>) -> Result<()> {
    let root = discovery::game_dir(&state.settings()?.game_dir).ok_or("请先设置游戏目录")?;
    let root = dunce::canonicalize(root.join("Beat Saber_Data/CustomWIPLevels"))
        .map_err(|e| state::issue("找不到谱面目录", e))?;
    let dir = dunce::canonicalize(path).map_err(|e| state::issue("找不到已导出的谱面目录", e))?;
    if dir.parent() != Some(root.as_path()) || !dir.is_dir() {
        return Err("找不到已导出的谱面目录".into());
    }
    show_directory(&dir)
}

#[cfg(windows)]
fn show_directory(dir: &std::path::Path) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null};
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let path: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let code = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            null(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(format!("无法打开谱面目录（{code}）"))
    }
}

#[cfg(not(windows))]
fn show_directory(dir: &std::path::Path) -> Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| state::issue("无法打开谱面目录", e))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = if cfg!(debug_assertions) {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".dev-data")
            } else {
                app.path().app_data_dir()?
            };
            let tools = if cfg!(debug_assertions) {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("runtime")
            } else {
                app.path().resource_dir()?.join("runtime")
            };
            // Node's module loader needs a normal Windows path for the helper script.
            app.manage(State::new(dir, dunce::simplified(&tools).to_path_buf())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            validate_game_dir,
            discover_game,
            game_dialog_directory,
            account_names,
            logout,
            open_export_folder,
            accounts::start_qr,
            accounts::poll_qr,
            accounts::netease_sms,
            accounts::netease_login,
            accounts::open_bili_login,
            accounts::poll_bili_login,
            accounts::close_bili_login,
            accounts::import_youtube,
            accounts::open_youtube,
            media::resolve_media,
            media::replace_cover,
            media::analyze_audio,
            media::preview_audio,
            media::export_map,
            updates::check_update,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Beatmaps Starter 失败");
}

fn emit_progress(app: &AppHandle, operation: &str, message: &str) {
    use tauri::Emitter;
    let _ = app.emit_to(
        "main",
        "media-progress",
        serde_json::json!({ "operation": operation, "message": message }),
    );
}
