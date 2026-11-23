use reqwest::{header, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{
    platform::{self, bili_data, bili_json, host_is, http_error},
    state::{issue, Credential, Platform, Result, State},
    tools,
};

const BILI_PASSPORT: &str = "https://passport.bilibili.com";
const BILI_LOGIN_LABEL: &str = "bilibili-login";
const BILI_COOKIES: &[&str] = &[
    "SESSDATA",
    "DedeUserID",
    "DedeUserID__ckMd5",
    "bili_jct",
    "buvid3",
    "buvid4",
    "b_nut",
];
const NETEASE_COOKIES: &[&str] = &["MUSIC_U", "MUSIC_A", "__csrf", "NMTID", "WEVNSM"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Qr {
    key: String,
    url: String,
    expires_in: u64,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum LoginStatus {
    Waiting,
    Scanned,
    Expired,
    Closed,
    Complete { name: String },
}

#[tauri::command]
pub async fn start_qr(platform: Platform, state: tauri::State<'_, State>) -> Result<Qr> {
    let (key, url) = match platform {
        Platform::Netease => {
            let key = netease_one(&state, "login_qr_key", json!({})).await?;
            let key = key["body"]["data"]["unikey"]
                .as_str()
                .ok_or("网易云未返回二维码，请稍后重试")?;
            let data = netease_one(&state, "login_qr_create", json!({ "key": key })).await?;
            (
                key.to_owned(),
                platform::string(&data["body"]["data"]["qrurl"]),
            )
        }
        Platform::Bilibili => {
            let response = state
                .client
                .get(format!(
                    "{BILI_PASSPORT}/x/passport-login/web/qrcode/generate"
                ))
                .header(header::REFERER, platform::BILI_REFERER)
                .send()
                .await
                .map_err(http_error)?;
            let data = bili_data(response).await?;
            (
                platform::string(&data["qrcode_key"]),
                platform::string(&data["url"]),
            )
        }
        Platform::Youtube => return Err("YouTube 使用浏览器登录和 Cookie 导入".into()),
    };
    if key.is_empty() || url.is_empty() {
        return Err("平台未能生成二维码，请重试".into());
    }
    Ok(Qr {
        key,
        url,
        expires_in: 180,
    })
}

#[tauri::command]
pub async fn poll_qr(
    platform: Platform,
    key: String,
    state: tauri::State<'_, State>,
) -> Result<LoginStatus> {
    if key.is_empty() || key.len() > 256 {
        return Err("二维码已失效，请刷新".into());
    }
    match platform {
        Platform::Netease => {
            let data = netease_one(&state, "login_qr_check", json!({ "key": key })).await?;
            match data["body"]["code"].as_i64() {
                Some(800) => Ok(LoginStatus::Expired),
                Some(801) => Ok(LoginStatus::Waiting),
                Some(802) => Ok(LoginStatus::Scanned),
                Some(803) => save_netease_account(&state, &data).await,
                _ => Err("无法获取网易云扫码状态，请重试".into()),
            }
        }
        Platform::Bilibili => {
            let response = state
                .client
                .get(format!("{BILI_PASSPORT}/x/passport-login/web/qrcode/poll"))
                .header(header::REFERER, platform::BILI_REFERER)
                .query(&[("qrcode_key", key)])
                .send()
                .await
                .map_err(http_error)?;
            let mut cookies: Vec<_> = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok().map(str::to_owned))
                .collect();
            let data = bili_data(response).await?;
            match data["code"].as_i64() {
                Some(86038) => Ok(LoginStatus::Expired),
                Some(86101) => Ok(LoginStatus::Waiting),
                Some(86090) => Ok(LoginStatus::Scanned),
                Some(0) => {
                    if !cookies.iter().any(|cookie| cookie.starts_with("SESSDATA=")) {
                        if let Some(url) = data["url"].as_str().and_then(|url| Url::parse(url).ok())
                        {
                            cookies.extend(
                                url.query_pairs()
                                    .filter(|(key, _)| BILI_COOKIES.contains(&key.as_ref()))
                                    .map(|(key, value)| {
                                        format!(
                                            "{key}={}",
                                            if key == "SESSDATA" {
                                                encode_cookie(&value)
                                            } else {
                                                value.into_owned()
                                            }
                                        )
                                    }),
                            );
                        }
                    }
                    let cookie = filtered_cookies(cookies.iter().map(String::as_str), BILI_COOKIES);
                    save_bili_account(&state, cookie).await
                }
                _ => Err("无法获取 Bilibili 扫码状态，请重试".into()),
            }
        }
        Platform::Youtube => Err("YouTube 使用浏览器登录和 Cookie 导入".into()),
    }
}

fn encode_cookie(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

async fn netease_one(state: &State, operation: &str, params: Value) -> Result<Value> {
    tools::netease(state, json!([{ "operation": operation, "params": params }]))
        .await?
        .into_iter()
        .next()
        .ok_or("网易云返回了空响应".into())
}

fn filtered_cookies<'a>(parts: impl Iterator<Item = &'a str>, allowed: &[&str]) -> String {
    let mut values = BTreeMap::new();
    for part in parts.flat_map(|line| line.split(';')) {
        if let Some((key, value)) = part.trim().split_once('=') {
            if allowed.contains(&key)
                && !value.is_empty()
                && !value.bytes().any(|c| c.is_ascii_control())
            {
                values.insert(key, value);
            }
        }
    }
    values
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

async fn save_netease_account(state: &State, data: &Value) -> Result<LoginStatus> {
    let parts = data["cookie"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(data["body"]["cookie"].as_str());
    let cookie = filtered_cookies(parts, NETEASE_COOKIES);
    if !cookie
        .split("; ")
        .any(|cookie| cookie.starts_with("MUSIC_U="))
    {
        return Err("网易云未返回登录会话，请重新登录".into());
    }
    let mut name = platform::string(&data["body"]["profile"]["nickname"]);
    if name.is_empty() {
        let user = netease_one(state, "user_account", json!({ "cookie": cookie })).await?;
        name = platform::string(&user["body"]["profile"]["nickname"]);
    }
    if name.is_empty() {
        return Err("无法验证网易云账号，请重新登录".into());
    }
    state.set_credential(
        Platform::Netease,
        Some(Credential {
            name: name.clone(),
            cookie,
        }),
    )?;
    Ok(LoginStatus::Complete { name })
}

fn validate_phone(phone: &str, country: &str) -> Result<()> {
    if (6..=15).contains(&phone.len())
        && phone.bytes().all(|c| c.is_ascii_digit())
        && (1..=4).contains(&country.len())
        && country.bytes().all(|c| c.is_ascii_digit())
    {
        Ok(())
    } else {
        Err("请输入有效的区号和手机号码".into())
    }
}

#[tauri::command]
pub async fn netease_sms(
    phone: String,
    country: String,
    state: tauri::State<'_, State>,
) -> Result<()> {
    validate_phone(&phone, &country)?;
    let data = netease_one(
        &state,
        "captcha_sent",
        json!({ "phone": phone, "ctcode": country }),
    )
    .await?;
    if data["body"]["code"].as_i64() != Some(200) {
        return Err(netease_login_error(
            &data,
            "网易云未能发送验证码，请稍后重试",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct NeteaseLogin {
    method: String,
    account: String,
    country: String,
    secret: String,
}

#[tauri::command]
pub async fn netease_login(
    payload: NeteaseLogin,
    state: tauri::State<'_, State>,
) -> Result<LoginStatus> {
    let account = payload.account.trim();
    let country = payload.country.trim().trim_start_matches('+');
    let secret = &payload.secret;
    if account.is_empty() || secret.is_empty() {
        return Err("请填写登录信息".into());
    }
    let (operation, params) = match payload.method.as_str() {
        "password" if account.contains('@') => {
            ("login", json!({ "email": account, "password": secret }))
        }
        "password" => {
            validate_phone(account, country)?;
            (
                "login_cellphone",
                json!({ "phone": account, "countrycode": country, "password": secret }),
            )
        }
        "sms" => {
            validate_phone(account, country)?;
            if !(4..=8).contains(&secret.len()) || !secret.bytes().all(|c| c.is_ascii_digit()) {
                return Err("请输入短信验证码".into());
            }
            (
                "login_cellphone",
                json!({ "phone": account, "countrycode": country, "captcha": secret }),
            )
        }
        _ => return Err("未知的登录方式".into()),
    };
    let data = netease_one(&state, operation, params).await?;
    if data["body"]["code"].as_i64() != Some(200) {
        return Err(netease_login_error(
            &data,
            "网易云登录失败，请核对登录信息或使用扫码",
        ));
    }
    save_netease_account(&state, &data).await
}

fn netease_login_error(data: &Value, fallback: &str) -> String {
    data["body"]["message"]
        .as_str()
        .or_else(|| data["body"]["msg"].as_str())
        .filter(|message| !message.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

async fn save_bili_account(state: &State, cookie: String) -> Result<LoginStatus> {
    if !cookie
        .split("; ")
        .any(|cookie| cookie.starts_with("SESSDATA="))
    {
        return Err("Bilibili 未返回登录会话，请重新登录".into());
    }
    let user = bili_json(state, "/x/web-interface/nav", &[], &cookie).await?;
    let name = platform::string(&user["uname"]);
    if user["isLogin"] != true || name.is_empty() {
        return Err("无法验证 Bilibili 账号，请重新登录".into());
    }
    state.set_credential(
        Platform::Bilibili,
        Some(Credential {
            name: name.clone(),
            cookie,
        }),
    )?;
    Ok(LoginStatus::Complete { name })
}

#[tauri::command]
pub async fn open_bili_login(app: AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window(BILI_LOGIN_LABEL) {
        return window.set_focus().map_err(|e| issue("无法打开登录窗口", e));
    }
    let parent = app.get_webview_window("main").ok_or("找不到主窗口")?;
    let url = Url::parse("https://passport.bilibili.com/login").unwrap();
    WebviewWindowBuilder::new(&app, BILI_LOGIN_LABEL, WebviewUrl::External(url))
        .title("Bilibili 登录")
        .inner_size(850.0, 660.0)
        .incognito(true)
        .parent(&parent)
        .map_err(|e| issue("无法打开登录窗口", e))?
        .on_navigation(|url| {
            url.scheme() == "https"
                && url
                    .host_str()
                    .is_some_and(|host| host_is(host, "bilibili.com"))
        })
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .build()
        .map_err(|e| issue("无法打开登录窗口", e))?;
    Ok(())
}

#[tauri::command]
pub async fn poll_bili_login(
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<LoginStatus> {
    let Some(window) = app.get_webview_window(BILI_LOGIN_LABEL) else {
        return Ok(LoginStatus::Closed);
    };
    let url = Url::parse(platform::BILI_REFERER).unwrap();
    let cookies = window
        .cookies_for_url(url)
        .map_err(|e| issue("无法读取 Bilibili 登录结果", e))?;
    if !cookies
        .iter()
        .any(|cookie| cookie.name() == "SESSDATA" && !cookie.value().is_empty())
    {
        return Ok(LoginStatus::Waiting);
    }
    let cookies: Vec<_> = cookies
        .iter()
        .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
        .collect();
    let cookie = filtered_cookies(cookies.iter().map(String::as_str), BILI_COOKIES);
    let result = save_bili_account(&state, cookie).await?;
    let _ = window.close();
    Ok(result)
}

#[tauri::command]
pub fn close_bili_login(app: AppHandle) {
    if let Some(window) = app.get_webview_window(BILI_LOGIN_LABEL) {
        let _ = window.close();
    }
}

#[tauri::command]
pub fn open_youtube() -> Result<()> {
    open_youtube_page("https://www.youtube.com/account")
}

#[tauri::command]
pub fn open_youtube_cookie_extension() -> Result<()> {
    open_youtube_page(
        "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",
    )
}

fn open_youtube_page(url: &str) -> Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map_err(|e| issue("无法打开浏览器", e))?;
    Ok(())
}

#[tauri::command]
pub async fn import_youtube(
    path: String,
    label: String,
    state: tauri::State<'_, State>,
) -> Result<LoginStatus> {
    let path = std::path::Path::new(&path);
    let size = tokio::fs::metadata(path)
        .await
        .map_err(|e| issue("无法读取 Cookie 文件", e))?
        .len();
    if size > 2 * 1024 * 1024 {
        return Err("Cookie 文件超过 2 MB".into());
    }
    let text = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| issue("无法读取 Cookie 文件", e))?;
    let text = text.trim_start_matches('\u{feff}');
    if !text.starts_with("# Netscape HTTP Cookie File") && !text.starts_with("# HTTP Cookie File") {
        return Err("请选择 Netscape 格式的 Cookie 文本文件".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut cookie_file = "# Netscape HTTP Cookie File\n".to_owned();
    let mut auth_pairs = BTreeMap::new();
    for line in text.lines() {
        let domain_line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
        let values: Vec<_> = domain_line.split('\t').collect();
        if let [domain, _, path, _, expires, key, value] = values.as_slice() {
            let host = domain.trim_start_matches('.');
            let expiry = expires.parse::<u64>().unwrap_or_default();
            if host_is(host, "youtube.com")
                && !value.bytes().any(|c| c.is_ascii_control())
                && (expiry == 0 || expiry > now)
            {
                cookie_file.push_str(line);
                cookie_file.push('\n');
                if matches!(host, "youtube.com" | "www.youtube.com") && *path == "/" {
                    auth_pairs.insert(*key, *value);
                }
            }
        }
    }
    if !["SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID"]
        .iter()
        .any(|key| auth_pairs.contains_key(key))
    {
        return Err("文件中缺少有效的 YouTube 登录 Cookie".into());
    }
    let cookie_header = auth_pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    let html = state
        .client
        .get("https://www.youtube.com/account")
        .header(header::COOKIE, cookie_header)
        .send()
        .await
        .map_err(http_error)?
        .error_for_status()
        .map_err(http_error)?
        .text()
        .await
        .map_err(http_error)?;
    let logged_in = regex::Regex::new(r#""LOGGED_IN"\s*:\s*true"#).unwrap();
    if !logged_in.is_match(&html) {
        return Err("YouTube 无法验证该会话，请重新导出已登录的 Cookie".into());
    }
    let name =
        regex::Regex::new(r#""accountName"\s*:\s*\{\s*"simpleText"\s*:\s*("(?:[^"\\]|\\.)*")"#)
            .unwrap()
            .captures(&html)
            .and_then(|capture| serde_json::from_str::<String>(&capture[1]).ok())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                let label: String = label.trim().chars().take(80).collect();
                if label.is_empty() {
                    "YouTube 账号".into()
                } else {
                    label
                }
            });
    state.set_credential(
        Platform::Youtube,
        Some(Credential {
            name: name.clone(),
            cookie: cookie_file,
        }),
    )?;
    Ok(LoginStatus::Complete { name })
}
