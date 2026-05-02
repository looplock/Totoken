use std::fmt;

use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    Validation(String),
    NotFound(String),
    Database(rusqlite::Error),
    Pool(r2d2::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Time(std::time::SystemTimeError),
    Tauri(tauri::Error),
    Http(reqwest::Error),
    Internal(String),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppErrorPayload<'a> {
    code: &'a str,
    message: String,
}

impl AppError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Pool(_) => "DB_POOL_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Json(_) => "JSON_ERROR",
            Self::Time(_) => "TIME_ERROR",
            Self::Tauri(_) => "TAURI_ERROR",
            Self::Http(_) => "HTTP_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Validation(message) => message.clone(),
            Self::NotFound(message) => message.clone(),
            Self::Database(error) => error.to_string(),
            Self::Pool(error) => error.to_string(),
            Self::Io(error) => error.to_string(),
            Self::Json(error) => error.to_string(),
            Self::Time(error) => error.to_string(),
            Self::Tauri(error) => error.to_string(),
            Self::Http(error) => error.to_string(),
            Self::Internal(message) => message.clone(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let payload = AppErrorPayload {
            code: self.code(),
            message: self.message(),
        };

        payload.serialize(serializer)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<r2d2::Error> for AppError {
    fn from(value: r2d2::Error) -> Self {
        Self::Pool(value)
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<std::time::SystemTimeError> for AppError {
    fn from(value: std::time::SystemTimeError) -> Self {
        Self::Time(value)
    }
}

impl From<tauri::Error> for AppError {
    fn from(value: tauri::Error) -> Self {
        Self::Tauri(value)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<walkdir::Error> for AppError {
    fn from(value: walkdir::Error) -> Self {
        let message = value.to_string();
        match value.into_io_error() {
            Some(error) => Self::Io(error),
            None => Self::Internal(message),
        }
    }
}
