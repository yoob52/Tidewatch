//! Tauri 壳：把 tidewatch-core 暴露成命令。业务逻辑都在 core 里。
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;
use tidewatch_core::agent::{adapter, AgentKind, Detection};
use tidewatch_core::{db, git};

pub struct AppState {
    pub db: Mutex<tidewatch_core::db::Connection>,
    pub data_dir: PathBuf,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct SystemInfo {
    version: &'static str,
    data_dir: String,
    db_path: String,
    schema_version: u32,
    sqlite_version: String,
    git_version: Option<String>,
    long_paths_hint: Option<&'static str>,
    os: &'static str,
}

#[tauri::command]
fn system_info(state: tauri::State<AppState>) -> CmdResult<SystemInfo> {
    let conn = state.db.lock().map_err(err)?;
    Ok(SystemInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.display().to_string(),
        db_path: state.data_dir.join("tidewatch.db").display().to_string(),
        schema_version: db::schema_version(&conn).map_err(err)?,
        sqlite_version: db::sqlite_version(&conn).map_err(err)?,
        git_version: git::version(),
        long_paths_hint: git::long_paths_hint(),
        os: std::env::consts::OS,
    })
}

#[tauri::command]
async fn detect_agents() -> CmdResult<Vec<Detection>> {
    tauri::async_runtime::spawn_blocking(|| {
        [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Mock]
            .into_iter()
            .map(|k| adapter(k).detect())
            .collect()
    })
    .await
    .map_err(err)
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = match std::env::var_os("TIDEWATCH_DATA_DIR") {
                Some(p) => PathBuf::from(p),
                None => app.path().app_data_dir()?,
            };
            let conn = db::open(&data_dir.join("tidewatch.db"))?;
            app.manage(AppState {
                db: Mutex::new(conn),
                data_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![system_info, detect_agents])
        .run(tauri::generate_context!())
        .expect("启动观澜失败");
}
