use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("数据库错误：{0}")]
    Db(#[from] rusqlite::Error),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("git 失败：{0}")]
    Git(String),
    #[error("未找到可执行文件：{0}")]
    BinaryNotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
}
