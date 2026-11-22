use reqwest::{header, Url};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

use crate::{
    state::{issue, Platform, Result, State},
    tools,
};

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const BILI_REFERER: &str = "https://www.bilibili.com/";
pub const NETEASE_LEVELS: [(&str, &str); 4] = [
    ("standard", "标准"),
    ("higher", "较高"),
    ("exhigh", "极高"),
    ("lossless", "无损"),
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quality {
    pub id: String,
    pub label: String,
    pub bitrate: u64,
}

pub enum Source {
    Netease(String),
    Bilibili {
        query: (&'static str, String),
        cid: u64,
    },
    Youtube(String),
}

impl Source {
    pub fn platform(&self) -> Platform {
        match self {
            Self::Netease(_) => Platform::Netease,
            Self::Bilibili { .. } => Platform::Bilibili,
            Self::Youtube(_) => Platform::Youtube,
        }
    }
}

pub struct Metadata {
    pub title: String,
    pub artist: String,
    pub cover_url: String,
    pub duration: f64,
    pub chorus_start: Option<f64>,
    pub qualities: Vec<Quality>,
    pub source: Source,
}

pub fn host_is(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn platform_for(url: &Url) -> Option<Platform> {
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    match url.host_str()? {
        "music.163.com" | "y.music.163.com" | "163cn.tv" | "3.cn" => Some(Platform::Netease),
        "www.bilibili.com" | "bilibili.com" | "m.bilibili.com" | "b23.tv" | "bili2233.cn" => {
            Some(Platform::Bilibili)
        }
        "youtube.com"
        | "www.youtube.com"
        | "m.youtube.com"
        | "music.youtube.com"
        | "youtu.be"
        | "www.youtube-nocookie.com" => Some(Platform::Youtube),
        _ => None,
    }
}

pub async fn resolve(state: &State, url: &str) -> Result<Metadata> {
    let mut url = Url::parse(url.trim()).map_err(|_| "请输入完整的歌曲或视频链接")?;
    let platform = platform_for(&url).ok_or("仅支持网易云音乐、Bilibili 和 YouTube 链接")?;
    let redirect = matches!(
        url.host_str(),
        Some("163cn.tv" | "3.cn" | "b23.tv" | "bili2233.cn")
    ) || url.path().starts_with("/m/");
    if redirect {
        url = expand(state, url, platform).await?;
    }
    let account = state.credential(platform)?;
    let cookie = account
        .as_ref()
        .map_or("", |account| account.cookie.as_str());
    match platform {
        Platform::Netease => netease_metadata(state, &url, cookie).await,
        Platform::Bilibili => bili_metadata(state, &url, cookie).await,
        Platform::Youtube => youtube_metadata(state, &url, cookie).await,
    }
}

async fn expand(state: &State, mut url: Url, platform: Platform) -> Result<Url> {
    for _ in 0..5 {
        let response = state
            .redirects
            .get(url.clone())
            .send()
            .await
            .map_err(http_error)?;
        if !response.status().is_redirection() {
            return Ok(url);
        }
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|x| x.to_str().ok())
            .ok_or("分享链接缺少跳转地址")?;
        url = url.join(location).map_err(|_| "分享链接的跳转地址无效")?;
        if platform_for(&url) != Some(platform) {
            return Err("分享链接跳出了原平台，请粘贴完整链接".into());
        }
    }
    Err("分享链接跳转次数过多，请粘贴完整链接".into())
}

async fn netease_metadata(state: &State, url: &Url, cookie: &str) -> Result<Metadata> {
    let fragment = url.fragment().and_then(|frag| url.join(frag).ok());
    let parsed = fragment
        .as_ref()
        .filter(|url| url.path().starts_with("/song"))
        .unwrap_or(url);
    if !matches!(parsed.path().trim_end_matches('/'), "/song" | "/m/song") {
        return Err("请粘贴网易云的单首歌曲链接".into());
    }
    let id = parsed
        .query_pairs()
        .find(|(key, value)| key == "id" && value.parse::<u64>().is_ok())
        .map(|(_, value)| value.into_owned())
        .ok_or("网易云链接缺少歌曲 ID")?;
    let mut calls = vec![
        json!({ "operation": "song_detail", "params": { "ids": id, "cookie": cookie } }),
        json!({ "operation": "song_chorus", "params": { "id": id, "cookie": cookie } }),
    ];
    calls.extend(
        NETEASE_LEVELS
            .iter()
            .rev()
            .map(|(level, _)| netease_track_call(&id, level, cookie)),
    );
    let response = tools::netease(state, json!(calls)).await?;
    let song = response
        .first()
        .map(|item| &item["body"]["songs"][0])
        .filter(|item| item.is_object())
        .ok_or("网易云未返回歌曲详情，请检查链接及网络")?;
    let mut qualities = Vec::new();
    let mut seen = BTreeSet::new();
    for item in response.iter().skip(2).filter_map(netease_track) {
        let level = item["level"].as_str().unwrap_or_default();
        let Some((rank, (_, name))) = NETEASE_LEVELS
            .iter()
            .enumerate()
            .find(|(_, (id, _))| *id == level)
        else {
            continue;
        };
        if seen.insert(level) {
            let mut quality =
                make_quality(level, name, item["br"].as_u64().unwrap_or_default() / 1000);
            if quality.bitrate == 0 {
                quality.bitrate = [128, 192, 320, 800][rank];
            }
            qualities.push(quality);
        }
    }
    qualities.sort_by_key(|quality| {
        std::cmp::Reverse(
            NETEASE_LEVELS
                .iter()
                .position(|(id, _)| *id == quality.id)
                .unwrap_or_default(),
        )
    });
    let artist = song["ar"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|person| person["name"].as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    Ok(Metadata {
        title: string(&song["name"]),
        artist,
        cover_url: string(&song["al"]["picUrl"]),
        duration: song["dt"].as_f64().unwrap_or_default() / 1000.0,
        chorus_start: response.get(1).and_then(|reply| {
            let body = &reply["body"];
            let chorus = body["data"]
                .as_array()
                .or_else(|| body["chorus"].as_array())?;
            chorus.first()?["startTime"]
                .as_u64()
                .map(|ms| ms as f64 / 1000.0)
        }),
        qualities,
        source: Source::Netease(id),
    })
}

fn netease_track_call(id: &str, level: &str, cookie: &str) -> Value {
    json!({ "operation": "song_url_v1", "params": { "id": id, "level": level, "cookie": cookie } })
}

fn netease_track(result: &Value) -> Option<&Value> {
    let track = &result["body"]["data"][0];
    if track["code"].as_i64() != Some(200)
        || !track["freeTrialInfo"].is_null()
        || !track["url"].as_str().is_some_and(|url| !url.is_empty())
    {
        return None;
    }
    Some(track)
}

async fn bili_metadata(state: &State, url: &Url, cookie: &str) -> Result<Metadata> {
    let video = url
        .path()
        .trim_end_matches('/')
        .strip_prefix("/video/")
        .ok_or("请粘贴 Bilibili 的视频链接")?;
    let query = if video.starts_with("BV")
        && video.len() == 12
        && video.bytes().all(|c| c.is_ascii_alphanumeric())
    {
        ("bvid", video.to_owned())
    } else if let Some(aid) = video
        .strip_prefix("av")
        .filter(|aid| aid.parse::<u64>().is_ok())
    {
        ("aid", aid.to_owned())
    } else {
        return Err("Bilibili 链接的视频 ID 无效".into());
    };
    let video = bili_json(
        state,
        "/x/web-interface/view",
        &[(query.0, &query.1)],
        cookie,
    )
    .await?;
    let page = url
        .query_pairs()
        .find(|(key, _)| key == "p")
        .map(|(_, value)| {
            value
                .parse::<u64>()
                .ok()
                .filter(|page| *page > 0)
                .ok_or("分 P 编号无效")
        })
        .transpose()?
        .unwrap_or(1);
    let pages = video["pages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let part = pages
        .iter()
        .find(|part| part["page"].as_u64() == Some(page));
    let cid = part
        .and_then(|part| part["cid"].as_u64())
        .or_else(|| {
            (pages.is_empty() && page == 1)
                .then(|| video["cid"].as_u64())
                .flatten()
        })
        .ok_or("找不到链接指定的 Bilibili 分 P")?;
    let title = string(&video["title"]);
    let title = match part.and_then(|part| part["part"].as_str()) {
        Some(name) if pages.len() > 1 && name != title => format!("{title} · {name}"),
        _ => title,
    };
    let tracks = bili_tracks(state, &query, cid, cookie).await?;
    Ok(Metadata {
        title,
        artist: string(&video["owner"]["name"]),
        cover_url: string(&video["pic"]),
        duration: part
            .and_then(|part| part["duration"].as_f64())
            .or_else(|| video["duration"].as_f64())
            .unwrap_or_default(),
        chorus_start: None,
        qualities: tracks.into_iter().map(|(quality, _)| quality).collect(),
        source: Source::Bilibili { query, cid },
    })
}

pub async fn bili_json(
    state: &State,
    path: &str,
    params: &[(&str, &str)],
    cookie: &str,
) -> Result<Value> {
    let mut request = state
        .client
        .get(format!("https://api.bilibili.com{path}"))
        .header(header::REFERER, BILI_REFERER)
        .query(params);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    bili_data(request.send().await.map_err(http_error)?).await
}

pub async fn bili_data(response: reqwest::Response) -> Result<Value> {
    let object = response
        .error_for_status()
        .map_err(http_error)?
        .json::<Value>()
        .await
        .map_err(http_error)?;
    if object["code"].as_i64() == Some(0) {
        Ok(object["data"].clone())
    } else {
        Err(format!(
            "Bilibili：{}",
            object["message"].as_str().unwrap_or("接口暂时不可用")
        ))
    }
}

async fn bili_tracks(
    state: &State,
    query: &(&str, String),
    cid: u64,
    cookie: &str,
) -> Result<Vec<(Quality, String)>> {
    let dash = bili_json(
        state,
        "/x/player/playurl",
        &[
            (query.0, &query.1),
            ("cid", &cid.to_string()),
            ("fnval", "4048"),
            ("fourk", "1"),
        ],
        cookie,
    )
    .await?["dash"]
        .clone();
    let mut tracks = Vec::new();
    let mut seen = BTreeSet::new();
    for item in dash["audio"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(
            dash.get("flac")
                .and_then(|flac| flac.get("audio"))
                .filter(|audio| audio.is_object()),
        )
    {
        let Some(id) = item["id"].as_u64().filter(|&id| id != 30250) else {
            continue;
        };
        let url = item["baseUrl"]
            .as_str()
            .or_else(|| item["base_url"].as_str())
            .unwrap_or_default();
        if url.is_empty() || !seen.insert(id) {
            continue;
        }
        let name = match id {
            30251 => "Hi-Res 无损",
            _ => "AAC",
        };
        tracks.push((
            make_quality(
                &id.to_string(),
                name,
                item["bandwidth"].as_u64().unwrap_or_default() / 1000,
            ),
            url.to_owned(),
        ));
    }
    tracks.sort_by_key(|(quality, _)| std::cmp::Reverse(quality.bitrate));
    Ok(tracks)
}

fn youtube_id(url: &Url) -> Result<String> {
    let path: Vec<_> = url.path().trim_matches('/').split('/').collect();
    let id = match (url.host_str(), path.as_slice()) {
        (Some("youtu.be"), [id]) => Some((*id).to_owned()),
        (_, ["watch"]) => url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned()),
        (_, ["shorts" | "live" | "embed", id]) => Some((*id).to_owned()),
        _ => None,
    };
    id.filter(|id| {
        id.len() == 11
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    })
    .ok_or("请粘贴包含有效视频 ID 的 YouTube 链接".into())
}

fn youtube_process(state: &State) -> tokio::process::Command {
    let mut process = tools::command(state, "yt-dlp");
    process.env("PYTHONIOENCODING", "utf-8").args([
        "--ignore-config",
        "--no-playlist",
        "--no-progress",
        "--no-warnings",
        "--socket-timeout",
        "15",
        "--retries",
        "2",
        "--no-js-runtimes",
        "--js-runtimes",
    ]);
    let node = tools::executable(state, "node");
    process.arg(if node.is_absolute() {
        format!("node:{}", node.display())
    } else {
        "node".into()
    });
    let ffmpeg = tools::executable(state, "ffmpeg");
    if ffmpeg.is_absolute() {
        process.arg("--ffmpeg-location").arg(ffmpeg);
    }
    process
}

fn cookies_file(cookie: &str) -> Result<Option<tempfile::TempPath>> {
    use std::io::Write;
    if cookie.is_empty() {
        return Ok(None);
    }
    let mut file = tempfile::NamedTempFile::new().map_err(|e| issue("无法创建会话临时文件", e))?;
    file.write_all(cookie.as_bytes())
        .map_err(|e| issue("无法写入会话临时文件", e))?;
    Ok(Some(file.into_temp_path()))
}

async fn youtube_metadata(state: &State, url: &Url, cookie: &str) -> Result<Metadata> {
    let id = youtube_id(url)?;
    let cookies = cookies_file(cookie)?;
    let mut process = youtube_process(state);
    if let Some(path) = &cookies {
        process.arg("--cookies").arg(path.as_os_str());
    }
    process
        .args(["--dump-single-json", "--skip-download", "--"])
        .arg(youtube_url(&id));
    let output = tools::run(process, "yt-dlp", 90).await?;
    let data: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "无法读取 YouTube 视频详情")?;
    if data["is_live"].as_bool() == Some(true) {
        return Err("直播结束并生成回放后才能解析音频".into());
    }
    let formats = data["formats"].as_array().ok_or("YouTube 未返回可选音质")?;
    let only_audio = formats
        .iter()
        .any(|format| format["vcodec"] == "none" && youtube_quality(format).is_some());
    let mut qualities: Vec<_> = formats
        .iter()
        .filter(|format| !only_audio || format["vcodec"] == "none")
        .filter_map(youtube_quality)
        .collect();
    let mut seen = BTreeSet::new();
    qualities.retain(|quality| seen.insert(quality.id.clone()));
    qualities.sort_by_key(|quality| std::cmp::Reverse(quality.bitrate));
    Ok(Metadata {
        title: string(&data["title"]),
        artist: string(
            data.get("uploader")
                .filter(|value| value.is_string())
                .unwrap_or(&data["channel"]),
        ),
        cover_url: string(&data["thumbnail"]),
        duration: data["duration"].as_f64().unwrap_or_default(),
        chorus_start: None,
        qualities,
        source: Source::Youtube(id),
    })
}

fn youtube_quality(format: &Value) -> Option<Quality> {
    let codec = format["acodec"].as_str().filter(|codec| *codec != "none")?;
    let id = format["format_id"].as_str()?;
    if !id
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        || id.is_empty()
        || format["has_drm"] == true
    {
        return None;
    }
    let label = match codec {
        s if s.starts_with("mp4a") => "AAC",
        "opus" => "Opus",
        _ => codec,
    };
    let bitrate = format["abr"]
        .as_f64()
        .or_else(|| {
            (format["vcodec"] == "none")
                .then(|| format["tbr"].as_f64())
                .flatten()
        })
        .unwrap_or_default()
        .round() as u64;
    Some(make_quality(id, label, bitrate))
}

fn youtube_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

pub async fn download(state: &State, source: &Source, quality: &str, dest: &Path) -> Result<()> {
    let account = state.credential(source.platform())?;
    let cookie = account
        .as_ref()
        .map_or("", |account| account.cookie.as_str());
    match source {
        Source::Youtube(id) => {
            let cookies = cookies_file(cookie)?;
            let mut process = youtube_process(state);
            if let Some(path) = &cookies {
                process.arg("--cookies").arg(path.as_os_str());
            }
            process
                .args([
                    "--no-simulate",
                    "--no-part",
                    "--no-continue",
                    "--fixup",
                    "never",
                    "--max-filesize",
                    "1G",
                    "-f",
                    quality,
                    "-o",
                ])
                .arg(dest)
                .arg("--")
                .arg(youtube_url(id));
            tools::run(process, "yt-dlp", 300).await?;
            if !tokio::fs::metadata(dest)
                .await
                .is_ok_and(|file| file.len() > 0)
            {
                return Err("YouTube 下载未生成音频文件，可能已超出大小限制".into());
            }
            Ok(())
        }
        Source::Netease(id) => {
            let response =
                tools::netease(state, json!([netease_track_call(id, quality, cookie)])).await?;
            let track = response
                .first()
                .and_then(netease_track)
                .filter(|track| track["level"] == quality)
                .ok_or("当前账号已无法获取该网易云音质，请重新解析或登录")?;
            save_track(
                state.client.get(media_url(track["url"].as_str().unwrap())?),
                dest,
            )
            .await
        }
        Source::Bilibili { query, cid } => {
            let (_, url) = bili_tracks(state, query, *cid, cookie)
                .await?
                .into_iter()
                .find(|(track, _)| track.id == quality)
                .ok_or("该 Bilibili 音质已不可用，请重新解析")?;
            save_track(
                state
                    .client
                    .get(media_url(&url)?)
                    .header(header::REFERER, BILI_REFERER),
                dest,
            )
            .await
        }
    }
}

async fn save_track(request: reqwest::RequestBuilder, path: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    const LIMIT: u64 = 1024 * 1024 * 1024;
    let mut response = request
        .send()
        .await
        .map_err(http_error)?
        .error_for_status()
        .map_err(http_error)?;
    if response
        .content_length()
        .is_some_and(|length| length > LIMIT)
    {
        return Err("音频超过 1 GB，无法处理".into());
    }
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|e| issue("无法保存音轨", e))?;
    let mut size = 0;
    while let Some(chunk) = response.chunk().await.map_err(http_error)? {
        size += chunk.len() as u64;
        if size > LIMIT {
            return Err("音频超过 1 GB，无法处理".into());
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| issue("下载音轨失败", e))?;
    }
    file.flush().await.map_err(|e| issue("保存音轨失败", e))?;
    if size == 0 {
        return Err("平台返回了空的音频文件".into());
    }
    Ok(())
}

pub fn media_url(url: &str) -> Result<Url> {
    let url = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_owned()
    };
    let url = Url::parse(&url).map_err(|_| "平台返回了无效的媒体链接")?;
    if matches!(url.scheme(), "https" | "http")
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(url)
    } else {
        Err("平台返回了无效的媒体链接".into())
    }
}

pub fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

pub fn http_error(error: reqwest::Error) -> String {
    issue("平台请求失败", error.without_url())
}

fn make_quality(id: &str, name: &str, bitrate: u64) -> Quality {
    let label = if bitrate > 0 {
        format!("{name} · {bitrate} kbps")
    } else {
        name.to_owned()
    };
    Quality {
        id: id.to_owned(),
        label,
        bitrate,
    }
}
