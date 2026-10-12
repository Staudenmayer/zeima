#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    #[error("{field}: {message}")]
    Validation { field: String, message: String },
    #[error("{0}")]
    Conflict(String),
    #[error("invalid hlc timestamp: {0}")]
    InvalidHlc(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[cfg(feature = "db")]
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[cfg(feature = "db")]
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl Error {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }

    /// Stable machine-readable code for the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotFound { .. } => "not_found",
            Error::Validation { .. } => "validation",
            Error::Conflict(_) => "conflict",
            Error::InvalidHlc(_) => "invalid_hlc",
            Error::Json(_) => "json",
            #[cfg(feature = "db")]
            Error::Db(_) | Error::Migrate(_) => "database",
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
