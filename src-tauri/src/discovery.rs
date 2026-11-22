use regex::Regex;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn game_dir(input: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(input.trim().trim_matches('"'));
    (dir.is_absolute() && dir.join("Beat Saber.exe").is_file()).then_some(dir)
}

pub fn browse_root() -> Option<PathBuf> {
    let steam = PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common");
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from);
    [
        Some(steam),
        home.as_ref().map(|dir| dir.join("BSManager/BSInstances")),
        home,
    ]
    .into_iter()
    .flatten()
    .find(|dir| dir.is_dir())
}

pub fn discover() -> Option<PathBuf> {
    let profile = env::var_os("USERPROFILE").map(PathBuf::from);
    if let Some(dir) = profile.as_ref().and_then(|profile| {
        let settings =
            profile.join("AppData/LocalLow/BinaryElement/ChroMapper/ChroMapperSettings.json");
        game_dir(&json_path(&settings, "BeatSaberInstallation")?)
    }) {
        return Some(dir);
    }
    if let Some(dir) = steam_libraries(Path::new(
        r"C:\Program Files (x86)\Steam\steamapps\libraryfolders.vdf",
    )) {
        return Some(dir);
    }
    if let Some(dir) = profile
        .as_ref()
        .and_then(|root| find_instance(&root.join("BSManager/BSInstances")))
    {
        return Some(dir);
    }
    let config = PathBuf::from(env::var_os("APPDATA")?).join("bs-manager/config.json");
    let parent = PathBuf::from(json_path(&config, "installation-folder")?);
    parent
        .is_absolute()
        .then(|| find_instance(&parent.join("BSManager/BSInstances")))
        .flatten()
}

fn json_path(path: &Path, property: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    Some(json.get(property)?.as_str()?.to_owned())
}

fn steam_libraries(vdf: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(vdf).ok()?;
    let paths = Regex::new(r#"(?mi)^\s*"(?:path|[0-9]+)"\s*"([^"]+)""#).ok()?;
    let install_dir = Regex::new(r#"(?mi)^\s*"installdir"\s*"([^"\\/:]+)""#).ok()?;
    for path in paths.captures_iter(&text) {
        let root = PathBuf::from(path[1].replace(r"\\", r"\"));
        if !root.is_absolute() {
            continue;
        }
        let steamapps = root.join("steamapps");
        let manifest =
            fs::read_to_string(steamapps.join("appmanifest_620980.acf")).unwrap_or_default();
        let folder = install_dir
            .captures(&manifest)
            .map_or("Beat Saber", |line| line.get(1).unwrap().as_str());
        let dir = steamapps.join("common").join(folder);
        if dir.join("Beat Saber.exe").is_file() {
            return Some(dir);
        }
    }
    None
}

fn find_instance(root: &Path) -> Option<PathBuf> {
    if root.join("Beat Saber.exe").is_file() {
        return Some(root.to_owned());
    }
    let mut dirs: Vec<_> = fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs.into_iter().find_map(|dir| find_instance(&dir))
}
