//! 应用内更新：用户点了「下载并安装」才做。从发布页的 `latest.json` 找到安装包，下载后先用
//! 打包时内置的公钥校验签名，校验不过不安装；之后 Windows 上启动安装程序并退出，装完自动重开。
//!
//! 公钥和更新源写在 `tauri.conf.json` 的 `plugins.updater` 里，运行时不能改。没有内置公钥的版本
//! （比如自己从源码构建的）不能在应用内安装，只给出发布页地址。
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error, UpdaterExt};

pub const PROGRESS_EVENT: &str = "tw://update";

/// 读 `latest.json` 最多等这么久。
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 下载安装包最多等这么久。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Progress {
    Checking,
    Downloading { downloaded: u64, total: Option<u64> },
    Installing,
    Restarting,
}

/// 这个版本为什么不能在应用内安装；能装时为 `None`。
pub fn blocked(app: &AppHandle) -> Option<String> {
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u["pubkey"].as_str())
        .unwrap_or("");
    if pubkey.trim().is_empty() {
        return Some("这个版本没有内置更新签名的公钥，不能在应用内安装。".into());
    }
    if cfg!(windows) || (cfg!(target_os = "linux") && app.env().appimage.is_some()) {
        None
    } else {
        Some("当前的安装方式不支持应用内更新。".into())
    }
}

fn readable(e: Error) -> String {
    match e {
        Error::ReleaseNotFound | Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "最新发布里没有适用于这台电脑的更新包。".into()
        }
        Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) => {
            "安装包的签名校验没有通过，没有安装。".into()
        }
        Error::Reqwest(e) if e.is_timeout() => "连接更新源超时，请稍后再试。".into(),
        Error::Reqwest(e) => format!("连接更新源失败：{e}"),
        Error::Network(e) => format!("下载更新失败：{e}"),
        e => format!("更新没有完成：{e}"),
    }
}

/// 下载、校验并安装最新版本。成功时不返回：Windows 上进程退出交给安装程序，其它平台直接重启。
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if let Some(why) = blocked(app) {
        return Err(why);
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("更新已经在进行中。".into());
    }
    let res = run(app).await;
    BUSY.store(false, Ordering::SeqCst);
    res
}

async fn run(app: &AppHandle) -> Result<(), String> {
    let emit = |p: Progress| {
        let _ = app.emit(PROGRESS_EVENT, p);
    };
    emit(Progress::Checking);
    let updater = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        .build()
        .map_err(readable)?;
    let mut update = updater
        .check()
        .await
        .map_err(readable)?
        .ok_or("更新源里没有比当前版本更新的安装包。")?;
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    let mut downloaded = 0u64;
    let mut last = Instant::now();
    emit(Progress::Downloading {
        downloaded: 0,
        total: None,
    });
    // `download` 里已经校验过签名，校验不过会直接报错
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if last.elapsed() >= Duration::from_millis(150) {
                    last = Instant::now();
                    emit(Progress::Downloading { downloaded, total });
                }
            },
            || {},
        )
        .await
        .map_err(readable)?;
    emit(Progress::Installing);
    update.install(bytes).map_err(readable)?;
    // Windows 上到不了这里
    emit(Progress::Restarting);
    app.restart()
}
