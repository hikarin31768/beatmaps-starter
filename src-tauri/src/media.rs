use base64::{engine::general_purpose::STANDARD, Engine};
use image::{codecs::jpeg::JpegEncoder, ImageReader, Limits};
use ogg::PacketReader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Manager};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    discovery, emit_progress, history,
    platform::{self, Metadata, Quality},
    state::{issue, MapVersion, Platform, Result, State},
    tempo, tools,
};

const COVER_LIMIT: u64 = 20 * 1024 * 1024;
const COVER_BYTES: usize = 1_500_000;
const COVER_FILE: &str = "cover.jpg";
const SONG_BYTES: u64 = 10_000_000;
const SONG_FILE: &str = "song.ogg";
const AUDIO_DATA_FILE: &str = "AudioData.dat";
const DEFAULT_COVER: &[u8] = include_bytes!("../icons/icon.png");

pub struct Draft {
    id: String,
    metadata: Metadata,
    cover: Vec<u8>,
    cover_source: Arc<[u8]>,
    cover_region: Option<CoverRegion>,
    pending_cover: Option<Arc<[u8]>>,
    cached_file: Option<(String, TempDir)>,
    lowest_file: Option<(String, TempDir)>,
    preview_clip: Option<(f64, f64, PreviewClip)>,
    analysis: Option<tempo::Analysis>,
}

impl Draft {
    pub fn platform(&self) -> Platform {
        self.metadata.source.platform()
    }

    fn chorus_preview(&self) -> Option<PreviewRange> {
        let (start, end) = self.metadata.chorus?;
        PreviewRange::chorus(start, end, self.metadata.duration)
    }

    fn shifted_duration(&self, shift: f64, tail_seconds: f64) -> Result<f64> {
        let duration = self.metadata.duration;
        let total = duration + shift + tail_seconds;
        if !shift.is_finite()
            || !tail_seconds.is_finite()
            || tail_seconds < 0.0
            || !total.is_finite()
            || (duration > 0.0 && duration + shift <= 0.0)
        {
            return Err("音频偏移超出可调整范围".into());
        }
        Ok(if duration > 0.0 { total } else { 0.0 })
    }

    fn selected_preview(
        &self,
        preview: Option<PreviewRange>,
        shift: f64,
        tail_seconds: f64,
        to_track_end: bool,
    ) -> Result<PreviewRange> {
        let duration = self.shifted_duration(shift, tail_seconds)?;
        if let Some(preview) = preview {
            if to_track_end {
                if !preview.start.is_finite()
                    || preview.start < 0.0
                    || (duration > 0.0 && preview.start >= duration)
                {
                    return Err("请选择有效的预览入点和出点".into());
                }
                return Ok(PreviewRange {
                    end: duration,
                    ..preview
                });
            }
            return preview.validate(duration);
        }
        Ok(self
            .chorus_preview()
            .unwrap_or_else(|| PreviewRange::full(duration)))
    }

    fn lowest_quality(&self) -> Result<String> {
        self.metadata
            .qualities
            .last()
            .map(|quality| quality.id.clone())
            .ok_or("没有可用音轨".into())
    }
}

struct PreviewClip {
    dir: TempDir,
    scope: tauri::scope::fs::Scope,
}

impl PreviewClip {
    fn path(&self) -> PathBuf {
        self.dir.path().join("preview.ogg")
    }
}

impl Drop for PreviewClip {
    fn drop(&mut self) {
        let _ = self.scope.forbid_file(self.path());
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PreviewRange {
    start: f64,
    end: f64,
}

impl PreviewRange {
    fn chorus(start: f64, end: f64, duration: f64) -> Option<Self> {
        let end = if duration > 0.0 {
            end.min(duration)
        } else {
            end
        };
        (start < end).then_some(Self { start, end })
    }

    fn full(duration: f64) -> Self {
        Self {
            start: 0.0,
            end: duration.max(0.0),
        }
    }

    fn within(self, duration: f64) -> Self {
        if self.start >= duration {
            return Self::full(duration);
        }
        Self {
            end: self.end.min(duration),
            ..self
        }
    }

    fn validate(self, duration: f64) -> Result<Self> {
        if !self.start.is_finite()
            || !self.end.is_finite()
            || self.start < 0.0
            || self.end <= self.start
            || (duration > 0.0 && self.start >= duration)
        {
            return Err("请选择有效的预览入点和出点".into());
        }
        Ok(if duration > 0.0 {
            self.within(duration)
        } else {
            self
        })
    }

    fn length(self) -> f64 {
        self.end - self.start
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaView {
    id: String,
    platform: Platform,
    title: String,
    artist: String,
    cover: String,
    thumbnail: String,
    duration: f64,
    preview: PreviewRange,
    preview_to_track_end: bool,
    qualities: Vec<Quality>,
}

impl MediaView {
    fn new(draft: &Draft, thumbnail: String) -> Self {
        let metadata = &draft.metadata;
        let chorus = draft.chorus_preview();
        Self {
            id: draft.id.clone(),
            platform: draft.platform(),
            title: metadata.title.clone(),
            artist: metadata.artist.clone(),
            cover: cover_data(&draft.cover),
            thumbnail,
            duration: metadata.duration,
            preview: chorus.unwrap_or_else(|| PreviewRange::full(metadata.duration)),
            preview_to_track_end: chorus.is_none(),
            qualities: metadata.qualities.clone(),
        }
    }
}

#[tauri::command]
pub async fn resolve_media(
    url: String,
    history_id: Option<String>,
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<MediaView> {
    let saved = history_id
        .as_deref()
        .map(|id| history::find_entry(&state, id))
        .transpose()?;
    let mut draft = state.draft.lock().await;
    emit_progress(&app, "resolve", "正在获取歌曲信息");
    let metadata = platform::resolve(&state, &url).await?;
    let remote = if saved.as_ref().is_some_and(|entry| entry.draft.custom_cover) {
        None
    } else {
        remote_cover(&state, &metadata).await.ok()
    };
    let (cover_source, cover, thumbnail) = if let Some((source, cover)) = remote {
        let thumbnail = cover_thumbnail(&cover)?;
        (source, cover, thumbnail)
    } else if let Some(entry) = saved {
        let thumbnail = entry.draft.cover;
        let bytes = thumbnail
            .strip_prefix("data:image/jpeg;base64,")
            .filter(|value| value.len() <= 128 * 1024)
            .and_then(|value| STANDARD.decode(value).ok())
            .ok_or("历史封面无效")?;
        let (source, cover) = prepare_cover(bytes).await?;
        (source, cover, thumbnail)
    } else {
        let (source, cover) = prepare_cover(DEFAULT_COVER.to_vec()).await?;
        let thumbnail = cover_thumbnail(&cover)?;
        (source, cover, thumbnail)
    };
    let next = Draft {
        id: Uuid::new_v4().to_string(),
        metadata,
        cover,
        cover_source,
        cover_region: None,
        pending_cover: None,
        cached_file: None,
        lowest_file: None,
        preview_clip: None,
        analysis: None,
    };
    let view = MediaView::new(&next, thumbnail);
    *draft = Some(next);
    Ok(view)
}

async fn remote_cover(state: &State, metadata: &Metadata) -> Result<(Arc<[u8]>, Vec<u8>)> {
    let url = platform::media_url(&metadata.cover_url)?;
    let response = state
        .client
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(platform::http_error)?
        .error_for_status()
        .map_err(platform::http_error)?;
    let mut response = response;
    if response
        .content_length()
        .is_some_and(|size| size > COVER_LIMIT)
    {
        return Err("封面超过 20 MB".into());
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(platform::http_error)? {
        if data.len() as u64 + chunk.len() as u64 > COVER_LIMIT {
            return Err("封面超过 20 MB".into());
        }
        data.extend_from_slice(&chunk);
    }
    prepare_cover(data).await
}

#[derive(Serialize)]
pub struct CoverView {
    cover: String,
    thumbnail: String,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
pub struct CoverRegion {
    x: f64,
    y: f64,
    size: f64,
}

impl CoverRegion {
    fn centered(width: u32, height: u32) -> Self {
        let side = width.min(height);
        Self {
            x: f64::from((width - side) / 2) / f64::from(side),
            y: f64::from((height - side) / 2) / f64::from(side),
            size: 1.0,
        }
    }

    fn square(self, width: u32, height: u32) -> Result<(u32, u32, u32)> {
        if !self.x.is_finite() || !self.y.is_finite() || !(0.0 < self.size && self.size <= 1.0) {
            return Err("请选择有效的裁切区域".into());
        }
        let short = width.min(height);
        let pixels = |value: f64| (value * f64::from(short)).round() as u32;
        let side = pixels(self.size).clamp(1, short);
        Ok((
            pixels(self.x).min(width - side),
            pixels(self.y).min(height - side),
            side,
        ))
    }
}

#[derive(Serialize)]
pub struct CoverCropView {
    src: String,
    width: u32,
    height: u32,
    region: CoverRegion,
}

async fn read_cover_file(path: &Path) -> Result<Arc<[u8]>> {
    use tokio::io::AsyncReadExt;
    if !tokio::fs::metadata(path)
        .await
        .is_ok_and(|file| file.is_file() && file.len() <= COVER_LIMIT)
    {
        return Err("请选择不超过 20 MB 的 JPG、PNG 或 WebP 图片".into());
    }
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await
        .map_err(|e| issue("无法读取封面", e))?
        .take(COVER_LIMIT + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| issue("无法读取封面", e))?;
    if bytes.len() as u64 > COVER_LIMIT {
        return Err("封面超过 20 MB".into());
    }
    Ok(bytes.into())
}

#[tauri::command]
pub async fn prepare_cover_crop(
    id: String,
    path: Option<String>,
    state: tauri::State<'_, State>,
) -> Result<CoverCropView> {
    let mut guard = state.draft.lock().await;
    let draft = find_draft(&mut guard, &id)?;
    let (source, region) = match path {
        Some(path) => (read_cover_file(Path::new(&path)).await?, None),
        None => (draft.cover_source.clone(), draft.cover_region),
    };
    let preview_source = source.clone();
    let view = tauri::async_runtime::spawn_blocking(move || {
        let image = decode_cover(&preview_source)?;
        let (width, height) = (image.width(), image.height());
        let preview = image
            .resize(1200, 1200, image::imageops::FilterType::Triangle)
            .to_rgb8();
        Ok::<_, String>(CoverCropView {
            src: cover_data(&encode_cover_jpeg(&preview, 88)?),
            width,
            height,
            region: region.unwrap_or_else(|| CoverRegion::centered(width, height)),
        })
    })
    .await
    .map_err(|_| "无法准备封面裁切".to_owned())??;
    draft.pending_cover = Some(source);
    Ok(view)
}

#[tauri::command]
pub async fn apply_cover_crop(
    id: String,
    region: CoverRegion,
    state: tauri::State<'_, State>,
) -> Result<CoverView> {
    let mut guard = state.draft.lock().await;
    let draft = find_draft(&mut guard, &id)?;
    let source = draft.pending_cover.clone().ok_or("请重新打开裁切工具")?;
    let cover = crop_background(source.clone(), Some(region)).await?;
    let thumbnail = cover_thumbnail(&cover)?;
    draft.cover = cover;
    draft.cover_source = source;
    draft.cover_region = Some(region);
    draft.pending_cover = None;
    Ok(CoverView {
        cover: cover_data(&draft.cover),
        thumbnail,
    })
}

#[tauri::command]
pub async fn discard_cover_crop(id: String, state: tauri::State<'_, State>) -> Result<()> {
    find_draft(&mut *state.draft.lock().await, &id)?.pending_cover = None;
    Ok(())
}

async fn prepare_cover(bytes: Vec<u8>) -> Result<(Arc<[u8]>, Vec<u8>)> {
    let source: Arc<[u8]> = bytes.into();
    let cover = crop_background(source.clone(), None).await?;
    Ok((source, cover))
}

async fn crop_background(source: Arc<[u8]>, region: Option<CoverRegion>) -> Result<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || crop_cover(&source, region))
        .await
        .map_err(|_| "无法处理封面".to_owned())?
}

fn decode_cover(bytes: &[u8]) -> Result<image::DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| issue("无法识别图片", e))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(10000);
    limits.max_image_height = Some(10000);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "无法读取图片，请选择 JPG、PNG 或 WebP 封面".into())
}

fn crop_cover(bytes: &[u8], region: Option<CoverRegion>) -> Result<Vec<u8>> {
    let image = decode_cover(bytes)?;
    let (width, height) = (image.width(), image.height());
    let region = region.unwrap_or_else(|| CoverRegion::centered(width, height));
    let (x, y, side) = region.square(width, height)?;
    let cropped = image.crop_imm(x, y, side, side);
    for size in [1024, 896, 768, 640, 513] {
        let square = cropped
            .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
            .to_rgb8();
        let jpg = encode_cover_jpeg(&square, 100)?;
        if jpg.len() <= COVER_BYTES {
            return Ok(jpg);
        }
    }
    Err("无法生成不超过 1.5 MB 的 JPG 封面".into())
}

fn encode_cover_jpeg(image: &image::RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut jpg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpg, quality)
        .encode_image(image)
        .map_err(|e| issue("无法生成 JPG 封面", e))?;
    Ok(jpg)
}

fn cover_data(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))
}

fn cover_thumbnail(cover: &[u8]) -> Result<String> {
    let image = image::load_from_memory_with_format(cover, image::ImageFormat::Jpeg)
        .map_err(|e| issue("无法读取封面", e))?
        .resize_exact(128, 128, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    Ok(cover_data(&encode_cover_jpeg(&image, 88)?))
}

fn find_draft<'a>(guard: &'a mut Option<Draft>, id: &str) -> Result<&'a mut Draft> {
    guard
        .as_mut()
        .filter(|draft| draft.id == id)
        .ok_or("当前解析结果已失效，请返回并重新解析".into())
}

async fn audio_source(state: &State, draft: &mut Draft, quality: &str) -> Result<PathBuf> {
    if !draft
        .metadata
        .qualities
        .iter()
        .any(|item| item.id == quality)
    {
        return Err("请选择一种可用的音质".into());
    }
    let cache = if draft
        .metadata
        .qualities
        .last()
        .is_some_and(|item| item.id == quality)
    {
        &mut draft.lowest_file
    } else {
        &mut draft.cached_file
    };
    if let Some((id, dir)) = &*cache {
        if id == quality {
            return Ok(dir.path().join("source.media"));
        }
    }
    let dir = tempfile::Builder::new()
        .prefix("beatmaps-audio-")
        .tempdir()
        .map_err(|e| issue("无法创建音频临时目录", e))?;
    let path = dir.path().join("source.media");
    platform::download(state, &draft.metadata.source, quality, &path).await?;
    *cache = Some((quality.to_owned(), dir));
    Ok(path)
}

#[tauri::command]
pub async fn analyze_audio(id: String, state: tauri::State<'_, State>) -> Result<tempo::Analysis> {
    let mut guard = state.draft.lock().await;
    let draft = find_draft(&mut guard, &id)?;
    if let Some(analysis) = draft.analysis {
        return Ok(analysis);
    }
    let quality = draft.lowest_quality()?;
    let input = audio_source(&state, draft, &quality).await?;
    let mut process = ffmpeg_input(&state, &input, "error");
    process.args([
        "-ac",
        "2",
        "-ar",
        &tempo::RATE.to_string(),
        "-c:a",
        "pcm_f32le",
        "-f",
        "wav",
        "pipe:1",
    ]);
    let output = tools::run(process, "FFmpeg", 300).await?;
    drop(guard);
    let analysis = tauri::async_runtime::spawn_blocking(move || {
        tempo::analyze(tempo::wave_samples(&output.stdout)?)
    })
    .await
    .map_err(|e| issue("无法分析音频", e))??;
    if let Ok(draft) = find_draft(&mut *state.draft.lock().await, &id) {
        draft.analysis = Some(analysis);
    }
    Ok(analysis)
}

#[tauri::command]
pub async fn preview_audio(
    id: String,
    shift: Option<f64>,
    tail_seconds: Option<f64>,
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<String> {
    let mut guard = state.draft.lock().await;
    let draft = find_draft(&mut guard, &id)?;
    let shift = shift.unwrap_or_default();
    let tail_seconds = tail_seconds.unwrap_or_default();
    draft.shifted_duration(shift, tail_seconds)?;
    if let Some((previous_shift, previous_tail, clip)) = &draft.preview_clip {
        let path = clip.path();
        if *previous_shift == shift && *previous_tail == tail_seconds && path.is_file() {
            return Ok(path.to_string_lossy().into_owned());
        }
    }
    let quality = draft.lowest_quality()?;
    emit_progress(&app, "preview", "正在获取最低音质音轨");
    let path = audio_source(&state, draft, &quality).await?;
    emit_progress(&app, "preview", "正在准备试听音轨");
    let clip = PreviewClip {
        dir: tempfile::Builder::new()
            .prefix("beatmaps-preview-")
            .tempdir()
            .map_err(|e| issue("无法创建预览临时目录", e))?,
        scope: app.asset_protocol_scope(),
    };
    let clip_path = clip.path();
    let mut process = ffmpeg_input(&state, &path, "error");
    if shift != 0.0 || tail_seconds != 0.0 {
        let source = probe_audio(&state, &path).await?;
        let filters = offset_filters(shift, tail_seconds, source.rate)?;
        if !filters.is_empty() {
            process.args(["-af", &filters]);
        }
    }
    process
        .args(["-c:a", "libvorbis", "-q:a", "3"])
        .arg(&clip_path);
    tools::run(process, "FFmpeg", 300).await?;
    clip.scope
        .allow_file(&clip_path)
        .map_err(|e| issue("无法加载试听音轨", e))?;
    draft.preview_clip = Some((shift, tail_seconds, clip));
    Ok(clip_path.to_string_lossy().into_owned())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    id: String,
    quality: String,
    title: String,
    subtitle: String,
    artist: String,
    mapper: String,
    bpm: f64,
    version: MapVersion,
    preview: Option<PreviewRange>,
    #[serde(default)]
    preview_to_track_end: bool,
    #[serde(default)]
    shift: f64,
    #[serde(default)]
    tail_seconds: f64,
}

#[tauri::command]
pub async fn export_map(
    payload: ExportRequest,
    app: AppHandle,
    state: tauri::State<'_, State>,
) -> Result<String> {
    if payload.title.trim().is_empty() || !(1.0..=1000.0).contains(&payload.bpm) {
        return Err("请输入曲名以及 1 至 1000 之间的 BPM".into());
    }
    let game = discovery::game_dir(&state.settings()?.game_dir)
        .ok_or("导出前请在设置中选择 Beat Saber 游戏目录")?;
    let mut guard = state.draft.lock().await;
    let draft = find_draft(&mut guard, &payload.id)?;
    let preview = draft.selected_preview(
        payload.preview,
        payload.shift,
        payload.tail_seconds,
        payload.preview_to_track_end,
    )?;
    emit_progress(&app, "export", "正在获取所选音轨");
    let input = audio_source(&state, draft, &payload.quality).await?;
    let mut source = probe_audio(&state, &input).await?;
    if source.bitrate == 0 && !source.lossless() {
        source.bitrate = draft
            .metadata
            .qualities
            .iter()
            .find(|item| item.id == payload.quality)
            .map_or(0, |item| item.bitrate.saturating_mul(1000));
    }
    let parent = game.join("Beat Saber_Data/CustomWIPLevels");
    fs::create_dir_all(&parent).map_err(|e| issue("无法创建 CustomWIPLevels 目录", e))?;
    let stage = tempfile::Builder::new()
        .prefix(".beatmaps-")
        .tempdir_in(&parent)
        .map_err(|e| issue("无法创建导出目录，请检查游戏目录的写入权限", e))?;
    let song = stage.path().join(SONG_FILE);
    emit_progress(&app, "export", "正在转换为 Ogg");
    let alignment = offset_filters(payload.shift, payload.tail_seconds, source.rate)?;
    let (rate, samples, lufs) = encode_song(&state, &input, &song, &source, &alignment, || {
        emit_progress(&app, "export", "正在压缩 Ogg");
    })
    .await?;
    emit_progress(&app, "export", "正在写入谱面文件");
    fs::write(stage.path().join(COVER_FILE), &draft.cover).map_err(|e| issue("无法写入封面", e))?;
    write_map(stage.path(), &payload, rate, samples, lufs, preview)?;
    let folder = folder_name(&payload.title, &payload.artist);
    let published = publish(stage.path(), &parent, &folder)?;
    Ok(published.to_string_lossy().into_owned())
}

async fn encode_song(
    state: &State,
    input: &Path,
    song: &Path,
    source: &AudioSource,
    alignment: &str,
    on_size_limit: impl FnOnce(),
) -> Result<(u32, u64, f64)> {
    const LUFS_FILTER: &str = "ebur128=framelog=verbose";
    let filters = if alignment.is_empty() {
        LUFS_FILTER.into()
    } else {
        format!("{alignment},{LUFS_FILTER}")
    };
    let encode = |option: &str, value: &str| {
        let mut process = ffmpeg_input(state, input, "info");
        process
            .args([
                "-y",
                "-af",
                &filters,
                "-c:a",
                "libvorbis",
                option,
                value,
                "-f",
                "ogg",
            ])
            .arg(song);
        tools::run(process, "FFmpeg", 300)
    };
    let size = || {
        fs::metadata(song)
            .map(|file| file.len())
            .map_err(|e| issue("无法读取 Ogg 文件大小", e))
    };
    let mut copied = source.codec == "vorbis" && alignment.is_empty();
    let mut output = if copied {
        let mut process = ffmpeg_input(state, input, "info");
        process.args(["-c:a", "copy", "-f", "ogg"]).arg(song);
        tools::run(process, "FFmpeg", 300).await?
    } else if source.lossless() {
        encode("-q:a", "10").await?
    } else if source.bitrate > 0 {
        encode("-b:a", &source.bitrate.to_string()).await?
    } else {
        encode("-q:a", "7").await?
    };

    if size()? > SONG_BYTES {
        on_size_limit();
        let (rate, samples) = vorbis_samples(song)?;
        let mut bitrate =
            (SONG_BYTES as f64 * 8.0 * f64::from(rate) / samples as f64 * 0.98) as u64;
        let mut within_limit = false;
        for _ in 0..4 {
            match encode("-b:a", &bitrate.to_string()).await {
                Ok(encoded) => output = encoded,
                Err(error)
                    if error.contains("encoder setup failed")
                        || error.contains("Error while opening encoder") =>
                {
                    break
                }
                Err(error) => return Err(error),
            }
            let bytes = size()?;
            if bytes <= SONG_BYTES {
                within_limit = true;
                break;
            }
            bitrate = (bitrate as f64 * SONG_BYTES as f64 / bytes as f64 * 0.97) as u64;
        }
        if !within_limit {
            for quality in ["5", "2", "0", "-1"] {
                output = encode("-q:a", quality).await?;
                if size()? <= SONG_BYTES {
                    break;
                }
            }
        }
        copied = false;
    }

    if size()? > SONG_BYTES {
        return Err("保留整首音轨时无法将 song.ogg 压到 10 MB 以内".into());
    }

    if copied {
        let mut measure = ffmpeg_input(state, song, "info");
        measure
            .args([
                "-af",
                LUFS_FILTER,
                "-c:a",
                "pcm_f32le",
                "-f",
                "wav",
                "pipe:1",
            ])
            .stdout(std::process::Stdio::null());
        output = tools::run(measure, "FFmpeg", 300).await?;
    }
    let lufs = String::from_utf8_lossy(&output.stderr)
        .lines()
        .rev()
        .find_map(|line| {
            line.trim()
                .strip_prefix("I:")?
                .split_whitespace()
                .next()?
                .parse::<f64>()
                .ok()
        })
        .unwrap_or_default();
    let song = song.to_path_buf();
    let (rate, samples) = tauri::async_runtime::spawn_blocking(move || vorbis_samples(&song))
        .await
        .map_err(|_| "无法读取导出音频".to_owned())??;
    Ok((rate, samples, lufs))
}

fn ffmpeg_input(state: &State, path: &Path, loglevel: &str) -> tokio::process::Command {
    let mut process = tools::command(state, "ffmpeg");
    process
        .args([
            "-hide_banner",
            "-nostdin",
            "-nostats",
            "-v",
            loglevel,
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args(["-map", "0:a:0", "-map_metadata", "-1", "-vn", "-sn", "-dn"]);
    process
}

struct AudioSource {
    codec: String,
    rate: u32,
    bitrate: u64,
}

impl AudioSource {
    fn lossless(&self) -> bool {
        matches!(self.codec.as_str(), "flac" | "alac") || self.codec.starts_with("pcm_")
    }
}

fn probe_number(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .unwrap_or_default()
}

async fn probe_audio(state: &State, path: &Path) -> Result<AudioSource> {
    let mut probe = tools::command(state, "ffprobe");
    probe
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_name,sample_rate,bit_rate,duration:format=duration",
            "-of",
            "json",
        ])
        .arg(path);
    let output = tools::run(probe, "FFprobe", 60).await?;
    let json: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| issue("无法读取音轨参数", e))?;
    let audio = &json["streams"][0];
    let rate = probe_number(&audio["sample_rate"]) as u32;
    if rate == 0 {
        return Err("无法读取音轨采样率".into());
    }
    let mut source = AudioSource {
        codec: audio["codec_name"].as_str().unwrap_or_default().into(),
        rate,
        bitrate: probe_number(&audio["bit_rate"]),
    };
    if !source.lossless() && source.bitrate == 0 {
        // Opus in WebM generally exposes no stream bitrate. Count the selected audio packets.
        let duration = [&audio["duration"], &json["format"]["duration"]]
            .into_iter()
            .filter_map(|value| value.as_str()?.parse::<f64>().ok())
            .find(|value| *value > 0.0);
        if let Some(duration) = duration {
            let mut probe = tools::command(state, "ffprobe");
            probe
                .args([
                    "-v",
                    "error",
                    "-protocol_whitelist",
                    "file,pipe",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "packet=size",
                    "-of",
                    "csv=p=0",
                ])
                .arg(path);
            let output = tools::run(probe, "FFprobe", 60).await?;
            let bytes: u64 = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.split(',').next()?.trim().parse::<u64>().ok())
                .sum();
            source.bitrate = (bytes as f64 * 8.0 / duration).round() as u64;
        }
    }
    Ok(source)
}

fn offset_filters(shift: f64, tail: f64, rate: u32) -> Result<String> {
    let front = (shift * f64::from(rate)).round();
    let back = (tail * f64::from(rate)).round();
    if !front.is_finite()
        || front.abs() > f64::from(i32::MAX)
        || !(0.0..=f64::from(i32::MAX)).contains(&back)
    {
        return Err("音频偏移超出可调整范围".into());
    }
    let mut filters = Vec::new();
    match front as i64 {
        0 => {}
        count if count < 0 => filters.push(format!(
            "atrim=start_sample={},asetpts=PTS-STARTPTS",
            -count
        )),
        count => filters.push(format!("adelay={count}S:all=1")),
    }
    if back > 0.0 {
        filters.push(format!("apad=pad_len={}", back as i64));
    }
    Ok(filters.join(","))
}

fn vorbis_samples(path: &Path) -> Result<(u32, u64)> {
    let file = fs::File::open(path).map_err(|e| issue("无法打开导出音频", e))?;
    let mut reader = PacketReader::new(std::io::BufReader::new(file));
    let header = reader
        .read_packet()
        .map_err(|e| issue("无法读取 Ogg 音频", e))?
        .filter(|packet| packet.data.starts_with(b"\x01vorbis") && packet.data.len() >= 16)
        .ok_or("导出的文件缺少 Vorbis 音轨")?;
    let rate = u32::from_le_bytes(header.data[12..16].try_into().unwrap());
    while let Some(packet) = reader
        .read_packet()
        .map_err(|e| issue("无法读取 Ogg 音频", e))?
    {
        if packet.last_in_stream()
            && packet.stream_serial() == header.stream_serial()
            && rate > 0
            && (1..=i32::MAX as u64).contains(&packet.absgp_page())
        {
            return Ok((rate, packet.absgp_page()));
        }
    }
    Err("导出音频没有有效时长".into())
}

fn write_map(
    dir: &Path,
    data: &ExportRequest,
    rate: u32,
    samples: u64,
    lufs: f64,
    preview: PreviewRange,
) -> Result<()> {
    let duration = samples as f64 / rate as f64;
    let end_beat = duration * data.bpm / 60.0;
    let preview = if data.preview_to_track_end {
        PreviewRange {
            end: duration,
            ..preview
        }
    } else {
        preview
    }
    .within(duration);
    let version = env!("CARGO_PKG_VERSION");
    let mapper = data.mapper.trim();
    let info = match data.version {
        MapVersion::Legacy => {
            write_json(
                dir,
                "BPMInfo.dat",
                &json!({
                    "_version": "2.0.0",
                    "_songSampleCount": samples,
                    "_songFrequency": rate,
                    "_regions": [{
                        "_startSampleIndex": 0,
                        "_endSampleIndex": samples,
                        "_startBeat": 0,
                        "_endBeat": end_beat
                    }]
                }),
            )?;
            json!({
                "_version": "2.1.0",
                "_songName": data.title.trim(),
                "_songSubName": data.subtitle.trim(),
                "_songAuthorName": data.artist.trim(),
                "_levelAuthorName": mapper,
                "_beatsPerMinute": data.bpm,
                "_songTimeOffset": 0,
                "_shuffle": 0,
                "_shufflePeriod": 0,
                "_previewStartTime": preview.start,
                "_previewDuration": preview.length(),
                "_songFilename": SONG_FILE,
                "_coverImageFilename": COVER_FILE,
                "_environmentName": "DefaultEnvironment",
                "_allDirectionsEnvironmentName": "GlassDesertEnvironment",
                "_environmentNames": ["DefaultEnvironment"],
                "_colorSchemes": [],
                "_difficultyBeatmapSets": [],
                "_customData": {
                    "_editors": {
                        "_lastEditedBy": "BeatmapsStarter",
                        "BeatmapsStarter": { "version": version }
                    }
                }
            })
        }
        MapVersion::V4 => {
            let mappers = if mapper.is_empty() {
                Vec::new()
            } else {
                vec![mapper]
            };
            write_json(dir, "ExpertPlus.dat", &json!({ "version": "4.0.0" }))?;
            write_json(dir, "Lightshow.dat", &json!({ "version": "4.0.0" }))?;
            write_json(
                dir,
                AUDIO_DATA_FILE,
                &json!({
                    "version": "4.0.0",
                    "songChecksum": "",
                    "songSampleCount": samples,
                    "songFrequency": rate,
                    "bpmData": [{ "si": 0, "ei": samples, "sb": 0, "eb": end_beat }],
                    "lufsData": [{ "si": 0, "ei": samples, "l": lufs }]
                }),
            )?;
            json!({
                "version": "4.0.1",
                "song": {
                    "title": data.title.trim(),
                    "subTitle": data.subtitle.trim(),
                    "author": data.artist.trim()
                },
                "audio": {
                    "songFilename": SONG_FILE,
                    "songDuration": duration,
                    "audioDataFilename": AUDIO_DATA_FILE,
                    "bpm": data.bpm,
                    "lufs": lufs,
                    "previewStartTime": preview.start,
                    "previewDuration": preview.length()
                },
                "songPreviewFilename": SONG_FILE,
                "coverImageFilename": COVER_FILE,
                "environmentNames": ["DefaultEnvironment"],
                "colorSchemes": [],
                "difficultyBeatmaps": [{
                    "characteristic": "Standard",
                    "difficulty": "ExpertPlus",
                    "beatmapAuthors": { "mappers": mappers, "lighters": [] },
                    "environmentNameIdx": 0,
                    "beatmapColorSchemeIdx": -1,
                    "noteJumpMovementSpeed": 10.0,
                    "noteJumpStartBeatOffset": 0.0,
                    "beatmapDataFilename": "ExpertPlus.dat",
                    "lightshowDataFilename": "Lightshow.dat"
                }],
                "customData": {
                    "editors": {
                        "lastEditedBy": "BeatmapsStarter",
                        "BeatmapsStarter": { "version": version }
                    }
                }
            })
        }
    };
    write_json(dir, "Info.dat", &info)
}

fn write_json(dir: &Path, name: &str, data: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(data).map_err(|e| issue("无法生成谱面数据", e))?;
    fs::write(dir.join(name), bytes).map_err(|e| issue("无法写入谱面数据", e))
}

fn folder_name(title: &str, artist: &str) -> String {
    let name = if artist.trim().is_empty() {
        title.trim().to_owned()
    } else {
        format!("{} - {}", title.trim(), artist.trim())
    };
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(90)
        .collect();
    let name = name.trim_matches([' ', '.']);
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (stem.starts_with("COM") || stem.starts_with("LPT"))
        && ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&&stem[3..]);
    if name.is_empty() {
        "Beatmap".into()
    } else if reserved {
        format!("_{name}")
    } else {
        name.into()
    }
}

fn publish(stage: &Path, parent: &Path, title: &str) -> Result<PathBuf> {
    for index in 1..=1000 {
        let name = if index == 1 {
            title.to_owned()
        } else {
            format!("{title} ({index})")
        };
        let dest = parent.join(name);
        if dest.exists() {
            continue;
        }
        match fs::rename(stage, &dest) {
            Ok(()) => return Ok(dest),
            Err(_) if dest.exists() => continue,
            Err(e) => return Err(issue("无法完成谱面导出", e)),
        }
    }
    Err("同名谱面过多，请更改曲名后重试".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_exports_square_jpeg_within_limits() {
        let cover = crop_cover(DEFAULT_COVER, None).unwrap();
        assert!(cover.starts_with(&[0xff, 0xd8, 0xff]));
        assert!(cover.len() <= COVER_BYTES);
        let image = image::load_from_memory(&cover).unwrap();
        assert_eq!(image.width(), image.height());
        assert!(image.width() > 512);
    }

    #[test]
    fn detailed_cover_is_resized_to_fit_the_limit() {
        let mut seed = 1u32;
        let pixels = image::RgbImage::from_fn(1024, 1024, |_, _| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let bytes = seed.to_be_bytes();
            image::Rgb([bytes[0], bytes[1], bytes[2]])
        });
        let mut input = Cursor::new(Vec::new());
        pixels
            .write_to(&mut input, image::ImageFormat::Png)
            .unwrap();
        let cover = crop_cover(&input.into_inner(), None).unwrap();
        assert!(cover.len() <= 1_500_000);
        let image = image::load_from_memory(&cover).unwrap();
        assert_eq!(image.width(), image.height());
        assert!((513..1024).contains(&image.width()));
    }

    #[test]
    #[ignore = "requires FFmpeg and an oversized Ogg Vorbis input set via BEATMAPS_TEST_OVERSIZED_OGG"]
    fn oversized_ogg_is_reencoded_without_cutting_the_song() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let input = PathBuf::from(
            std::env::var_os("BEATMAPS_TEST_OVERSIZED_OGG")
                .expect("set BEATMAPS_TEST_OVERSIZED_OGG to an Ogg Vorbis file larger than 10 MB"),
        );
        assert!(fs::metadata(&input).unwrap().len() > SONG_BYTES);
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.ogg");
        let state = State::new(dir.path().into(), manifest.join("runtime")).unwrap();
        tauri::async_runtime::block_on(async {
            let (original_rate, original_samples) = vorbis_samples(&input).unwrap();
            let flac = dir.path().join("source.flac");
            let mut convert = ffmpeg_input(&state, &input, "error");
            convert.args(["-c:a", "flac"]).arg(&flac);
            tools::run(convert, "FFmpeg", 300).await.unwrap();

            for (source_path, lossless) in [(&input, false), (&flac, true)] {
                let source = probe_audio(&state, source_path).await.unwrap();
                assert_eq!(source.lossless(), lossless);
                let mut capped = false;
                let (rate, samples, _) =
                    encode_song(&state, source_path, &song, &source, "", || capped = true)
                        .await
                        .unwrap();
                let size = fs::metadata(&song).unwrap().len();
                assert!(capped, "{} did not exercise the size limit", source.codec);
                assert!(size <= 10_000_000);
                assert_eq!(rate, original_rate);
                assert!(samples.abs_diff(original_samples) <= rate as u64 / 40);
                println!(
                    "{} input: {} bytes, {} Hz, {} samples",
                    source.codec, size, rate, samples
                );
                fs::remove_file(&song).unwrap();
            }
        });
    }

    #[test]
    fn preview_defaults_to_the_entire_track() {
        assert_eq!(
            PreviewRange::full(60.0),
            PreviewRange {
                start: 0.0,
                end: 60.0
            }
        );
        assert_eq!(
            PreviewRange::full(6.25),
            PreviewRange {
                start: 0.0,
                end: 6.25
            }
        );
    }

    #[test]
    fn chorus_preview_is_clamped_to_the_available_audio() {
        let preview = PreviewRange::chorus(24.5, 38.0, 30.0).unwrap();
        assert_eq!(
            preview,
            PreviewRange {
                start: 24.5,
                end: 30.0
            }
        );
        assert_eq!(preview.within(29.0).length(), 4.5);
        assert_eq!(preview.within(24.0), PreviewRange::full(24.0));
    }

    #[test]
    fn edited_preview_preserves_both_points_within_the_track() {
        let preview = PreviewRange {
            start: 6.125,
            end: 18.25,
        };
        assert_eq!(preview.validate(20.0).unwrap(), preview);
        assert_eq!(
            preview.validate(10.0).unwrap(),
            PreviewRange {
                end: 10.0,
                ..preview
            }
        );
        assert!(preview.validate(6.0).is_err());
        assert!(PreviewRange {
            start: 8.0,
            end: 7.0
        }
        .validate(20.0)
        .is_err());
    }

    #[test]
    fn bpm_info_uses_final_samples_and_fractional_bpm() {
        let dir = tempfile::tempdir().unwrap();
        let preview = PreviewRange {
            start: 0.0,
            end: 3.0,
        };
        let data = ExportRequest {
            id: String::new(),
            quality: String::new(),
            title: "Song".into(),
            subtitle: String::new(),
            artist: String::new(),
            mapper: String::new(),
            bpm: 98.625,
            version: MapVersion::Legacy,
            preview: Some(preview),
            preview_to_track_end: false,
            shift: 0.0,
            tail_seconds: 0.0,
        };
        write_map(dir.path(), &data, 48_000, 144_000, -14.0, preview).unwrap();
        let info: Value =
            serde_json::from_slice(&fs::read(dir.path().join("Info.dat")).unwrap()).unwrap();
        assert_eq!(info["_coverImageFilename"], COVER_FILE);
        let bpm_info: Value =
            serde_json::from_slice(&fs::read(dir.path().join("BPMInfo.dat")).unwrap()).unwrap();
        assert_eq!(bpm_info["_songFrequency"], json!(48_000));
        assert_eq!(bpm_info["_songSampleCount"], json!(144_000));
        assert_eq!(
            bpm_info["_regions"][0],
            json!({"_startSampleIndex": 0, "_endSampleIndex": 144_000, "_startBeat": 0, "_endBeat": 4.93125})
        );
    }
}
