use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("attachments are disabled (attachment_cache_dir is not configured)")]
    AttachmentsDisabled,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("topic name invalid")]
    TopicInvalid,

    #[error("message too large")]
    MessageTooLarge,

    #[error("too many requests")]
    TooManyRequests,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden")]
    Forbidden,

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::AttachmentsDisabled => StatusCode::BAD_REQUEST,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::TopicInvalid => StatusCode::BAD_REQUEST,
            AppError::MessageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> u32 {
        match self {
            AppError::NotFound => 40401,
            AppError::AttachmentsDisabled => 40003,
            AppError::BadRequest(_) => 40001,
            AppError::TopicInvalid => 40002,
            AppError::MessageTooLarge => 41301,
            AppError::TooManyRequests => 42901,
            AppError::Unauthorized => 40101,
            AppError::Forbidden => 40301,
            AppError::Internal(_) => 50001,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = json!({
            "code":    self.code(),
            "http":    status.as_u16(),
            "error":   self.to_string(),
            "link":    "https://ntfy.sh/docs/publish/#response-codes",
        });
        (status, Json(body)).into_response()
    }
}

impl From<r2d2::Error> for AppError {
    fn from(e: r2d2::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}
