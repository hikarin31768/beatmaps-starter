use std::{
    path::PathBuf,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

use crate::state::{issue, Result, State};

pub fn executable(state: &State, name: &str) -> PathBuf {
    let filename = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    let bundled = state.tools.join(&filename);
    if bundled.is_file() {
        bundled
    } else {
        PathBuf::from(filename)
    }
}

pub fn command(state: &State, name: &str) -> Command {
    let mut process = Command::new(executable(state, name));
    #[cfg(windows)]
    process.creation_flags(0x08000000); // CREATE_NO_WINDOW
    process.kill_on_drop(true).stdin(Stdio::null());
    process
}

pub async fn run(mut process: Command, name: &str, limit: u64) -> Result<Output> {
    let output = tokio::time::timeout(Duration::from_secs(limit), process.output())
        .await
        .map_err(|_| format!("{name} 运行超时，请重试"))?
        .map_err(|e| spawn_error(name, e))?;
    if !output.status.success() {
        return Err(tool_failure(name, &output.stderr));
    }
    Ok(output)
}

pub fn spawn_error(name: &str, error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        format!("找不到 {name}。请使用带运行组件的安装包，或将其添加到 PATH。")
    } else {
        issue(&format!("无法启动 {name}"), error)
    }
}

fn tool_failure(name: &str, stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    if name == "yt-dlp" {
        return if text.contains("Sign in") || text.contains("sign in") || text.contains("cookies") {
            "YouTube 要求登录或登录已失效。请在账号页导入有效 Cookie 后重试。".into()
        } else if text.contains("not available") || text.contains("unavailable") {
            "此 YouTube 视频当前不可用，或没有可下载的音轨。".into()
        } else {
            "YouTube 音轨获取失败，请检查网络、链接及 yt-dlp 版本后重试。".into()
        };
    }
    let detail = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .next_back()
        .unwrap_or("运行失败");
    format!("{name}：{}", detail.chars().take(260).collect::<String>())
}

pub async fn netease(state: &State, calls: serde_json::Value) -> Result<Vec<serde_json::Value>> {
    let script = state.tools.join("netease.cjs");
    if !script.is_file() {
        return Err("找不到网易云 API 助手，请检查应用的运行组件。".into());
    }
    let cache = state.dir.join("netease-cache");
    tokio::fs::create_dir_all(&cache)
        .await
        .map_err(|e| issue("无法创建网易云缓存目录", e))?;
    let mut process = command(state, "node");
    process
        .arg(&script)
        .current_dir(&cache)
        .env("TMP", &cache)
        .env("TEMP", &cache)
        .env("TMPDIR", &cache)
        .env("NODE_TLS_REJECT_UNAUTHORIZED", "1")
        .env_remove("NETEASE_COOKIE")
        .env_remove("ENABLE_RANDOM_CN_IP")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = process.spawn().map_err(|e| spawn_error("node", e))?;
    let output = tokio::time::timeout(Duration::from_secs(35), async {
        let mut input = child.stdin.take().unwrap();
        input.write_all(calls.to_string().as_bytes()).await?;
        input.shutdown().await?;
        drop(input);
        child.wait_with_output().await
    })
    .await
    .map_err(|_| "网易云请求超时，请检查网络后重试")?
    .map_err(|e| issue("无法读取网易云响应", e))?;
    if !output.status.success() {
        if serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .is_ok_and(|reply| reply["error"] == "dependency")
        {
            return Err(
                "网易云 API 缺少运行依赖，请检查应用组件或安装 src-tauri/runtime 的依赖。".into(),
            );
        }
        return Err(format!("网易云 API 助手启动失败（{}）。", output.status));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "无法读取网易云响应".into())
}
