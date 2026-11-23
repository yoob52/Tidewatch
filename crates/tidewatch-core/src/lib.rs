//! 观澜 Tidewatch 核心：不依赖 Tauri，可被桌面壳与 `twctl` 命令行共用。
pub mod agent;
pub mod api;
pub mod backup;
pub mod db;
pub mod engine;
pub mod env_path;
pub mod error;
pub mod git;
pub mod guard;
pub mod import;
pub mod process;
pub mod prompt;
pub mod provider;
pub mod runner;
pub mod secrets;
pub mod shell;
pub mod store;
pub mod sync;
pub mod update;
pub mod util;
pub mod workflow;

pub use error::{Error, Result};

pub use rusqlite;
