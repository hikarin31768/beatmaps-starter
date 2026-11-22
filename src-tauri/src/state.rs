use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tempfile::NamedTempFile;
use tokio::sync::Mutex as AsyncMutex;

use crate::media::Draft;

pub type Result<T> = std::result::Result<T, String>;

const DEFAULT_NETEASE_QUALITY: &str = "lossless";
const DEFAULT_BILIBILI_QUALITY: &str = "30251";
const DEFAULT_YOUTUBE_QUALITY: &str = "160";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Netease,
    Bilibili,
    Youtube,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MapVersion {
    #[default]
    Legacy,
    V4,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OffsetUnit {
    #[default]
    Seconds,
    Beats,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub initialized: bool,
    pub game_dir: String,
    pub theme: String,
    pub version: MapVersion,
    pub mapper: String,
    pub lead_in: f64,
    pub lead_in_unit: OffsetUnit,
    pub tail_offset: f64,
    pub tail_offset_unit: OffsetUnit,
    pub netease_quality: String,
    pub bilibili_quality: String,
    pub youtube_quality: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            initialized: false,
            game_dir: String::new(),
            theme: "dark".into(),
            version: MapVersion::Legacy,
            mapper: String::new(),
            lead_in: 2.0,
            lead_in_unit: OffsetUnit::Seconds,
            tail_offset: 2.0,
            tail_offset_unit: OffsetUnit::Seconds,
            netease_quality: DEFAULT_NETEASE_QUALITY.into(),
            bilibili_quality: DEFAULT_BILIBILI_QUALITY.into(),
            youtube_quality: DEFAULT_YOUTUBE_QUALITY.into(),
        }
    }
}

impl Settings {
    pub fn normalize_quality_preferences(&mut self) {
        if !crate::platform::NETEASE_LEVELS
            .iter()
            .any(|(level, _)| *level == self.netease_quality)
        {
            self.netease_quality = DEFAULT_NETEASE_QUALITY.into();
        }
        if !matches!(
            self.bilibili_quality.as_str(),
            "30251" | "30280" | "30232" | "30216"
        ) {
            self.bilibili_quality = DEFAULT_BILIBILI_QUALITY.into();
        }
        if !matches!(self.youtube_quality.as_str(), "160" | "128" | "64") {
            self.youtube_quality = DEFAULT_YOUTUBE_QUALITY.into();
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Credential {
    pub name: String,
    pub cookie: String,
}

type Credentials = BTreeMap<Platform, Credential>;

pub struct State {
    pub client: reqwest::Client,
    pub redirects: reqwest::Client,
    pub tools: PathBuf,
    pub dir: PathBuf,
    pub draft: AsyncMutex<Option<Draft>>,
    pub(crate) storage: Mutex<()>,
}

impl State {
    pub fn new(dir: PathBuf, tools: PathBuf) -> reqwest::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(crate::platform::USER_AGENT)
            .connect_timeout(std::time::Duration::from_secs(12))
            .timeout(std::time::Duration::from_secs(180));
        Ok(Self {
            client: client
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()?,
            redirects: reqwest::Client::builder()
                .user_agent(crate::platform::USER_AGENT)
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            tools,
            dir,
            draft: AsyncMutex::new(None),
            storage: Mutex::new(()),
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        match fs::read(self.dir.join("settings.json")) {
            Ok(bytes) => {
                let mut settings: Settings =
                    serde_json::from_slice(&bytes).map_err(|e| issue("读取设置失败", e))?;
                settings.normalize_quality_preferences();
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(issue("读取设置失败", e)),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(settings).map_err(|e| issue("保存设置失败", e))?;
        atomic_write(&self.dir, "settings.json", &bytes)
    }

    fn credentials(&self) -> Result<Credentials> {
        let data = match fs::read(self.dir.join("accounts.bin")) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(issue("读取本地账号失败", e)),
        };
        serde_json::from_slice(&protect(&data, false)?)
            .map_err(|_| "本地账号数据损坏，请清除后重新登录".into())
    }

    pub fn account_names(&self) -> Result<BTreeMap<Platform, String>> {
        Ok(self
            .credentials()?
            .into_iter()
            .map(|(platform, account)| (platform, account.name))
            .collect())
    }

    pub fn credential(&self, platform: Platform) -> Result<Option<Credential>> {
        Ok(self.credentials()?.remove(&platform))
    }

    pub fn set_credential(&self, platform: Platform, value: Option<Credential>) -> Result<()> {
        let _lock = self.storage.lock().map_err(|_| "本地账号存储暂时不可用")?;
        let mut accounts = self.credentials()?;
        match value {
            Some(account) => accounts.insert(platform, account),
            None => accounts.remove(&platform),
        };
        let bytes = serde_json::to_vec(&accounts).map_err(|_| "无法保存账号")?;
        atomic_write(&self.dir, "accounts.bin", &protect(&bytes, true)?)
    }
}

pub(crate) fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| issue("无法创建应用数据目录", e))?;
    let mut tmp = NamedTempFile::new_in(dir).map_err(|e| issue("写入本地数据失败", e))?;
    tmp.write_all(bytes)
        .map_err(|e| issue("写入本地数据失败", e))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| issue("写入本地数据失败", e))?;
    tmp.persist(dir.join(name))
        .map_err(|e| issue("保存本地数据失败", e))?;
    Ok(())
}

#[cfg(windows)]
fn protect(data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut _,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // DPAPI protects the payload with the current Windows user's credentials.
    unsafe {
        let success = if encrypt {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if success == 0 {
            return Err("Windows 无法访问本地加密账号数据".into());
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(not(windows))]
fn protect(_data: &[u8], _encrypt: bool) -> Result<Vec<u8>> {
    Err("当前版本的账号存储仅支持 Windows".into())
}

pub fn issue(context: &str, error: impl std::fmt::Display) -> String {
    format!("{context}：{error}")
}
