use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    media::PreviewRange,
    state::{atomic_write, issue, MapVersion, OffsetUnit, Result, State},
};

const FILE: &str = "history.json";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offset {
    value: f64,
    unit: OffsetUnit,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryForm {
    title: String,
    subtitle: String,
    artist: String,
    mapper: String,
    bpm: Option<f64>,
    version: MapVersion,
    quality: String,
    preview: PreviewRange,
    #[serde(default)]
    preview_to_track_end: bool,
    preview_source: PreviewRange,
    shift: f64,
    alignment: String,
    fixed_lead: Option<Offset>,
    #[serde(default)]
    editing_fixed_lead: bool,
    fixed_lead_input: Option<f64>,
    tail_offset: Option<f64>,
    tail_offset_unit: OffsetUnit,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDraft {
    id: String,
    url: String,
    pub(crate) cover: String,
    pub(crate) custom_cover: bool,
    form: HistoryForm,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    created_at: u64,
    #[serde(flatten)]
    pub(crate) draft: HistoryDraft,
}

fn read_history(state: &State) -> Result<Vec<HistoryEntry>> {
    match fs::read(state.dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| issue("读取历史失败", e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(issue("读取历史失败", e)),
    }
}

fn write_history(state: &State, entries: &[HistoryEntry]) -> Result<()> {
    let bytes = serde_json::to_vec(entries).map_err(|e| issue("保存历史失败", e))?;
    atomic_write(&state.dir, FILE, &bytes)
}

pub(crate) fn find_entry(state: &State, id: &str) -> Result<HistoryEntry> {
    read_history(state)?
        .into_iter()
        .find(|entry| entry.draft.id == id)
        .ok_or_else(|| "未找到这条历史记录".into())
}

#[tauri::command(async)]
pub fn list_history(state: tauri::State<'_, State>) -> Result<Vec<HistoryEntry>> {
    read_history(&state)
}

#[tauri::command(async)]
pub fn save_history(payload: HistoryDraft, state: tauri::State<'_, State>) -> Result<()> {
    let _lock = state.storage.lock().map_err(|_| "本地存储暂时不可用")?;
    let mut entries = read_history(&state)?;
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.draft.id == payload.id)
    {
        entry.draft = payload;
    } else {
        entries.insert(
            0,
            HistoryEntry {
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| issue("无法获取创建日期", e))?
                    .as_millis() as u64,
                draft: payload,
            },
        );
    }
    write_history(&state, &entries)
}

#[tauri::command(async)]
pub fn delete_history(ids: Vec<String>, state: tauri::State<'_, State>) -> Result<()> {
    let ids: HashSet<_> = ids.into_iter().collect();
    if ids.is_empty() {
        return Ok(());
    }
    let _lock = state.storage.lock().map_err(|_| "本地存储暂时不可用")?;
    let mut entries = read_history(&state)?;
    entries.retain(|entry| !ids.contains(&entry.draft.id));
    write_history(&state, &entries)
}
