use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AdminUnauthorized,
    SourceNotFound,
    SourceDisabled,
    AccountNotFound,
    AccountDisabled,
    AccountNotVip,
    LoginSessionNotFound,
    UpstreamRequestFailed,
    UpstreamLoginFailed,
    UpstreamVipCheckFailed,
    UpstreamPlayUrlEmpty,
    InvalidRequest,
    InternalError,
}

#[derive(Debug)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub status: StatusCode,
    pub is_admin: bool,
}

impl AppError {
    pub fn admin_unauthorized() -> Self {
        Self {
            code: ErrorCode::AdminUnauthorized,
            message: "unauthorized".into(),
            status: StatusCode::UNAUTHORIZED,
            is_admin: false,
        }
    }

    pub fn source_not_found() -> Self {
        Self {
            code: ErrorCode::SourceNotFound,
            message: "source not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    pub fn source_disabled() -> Self {
        Self {
            code: ErrorCode::SourceDisabled,
            message: "source is disabled".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn account_not_found() -> Self {
        Self {
            code: ErrorCode::AccountNotFound,
            message: "account not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    pub fn account_disabled() -> Self {
        Self {
            code: ErrorCode::AccountDisabled,
            message: "account is disabled".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn account_not_vip() -> Self {
        Self {
            code: ErrorCode::AccountNotVip,
            message: "account vip is not active".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn login_session_not_found() -> Self {
        Self {
            code: ErrorCode::LoginSessionNotFound,
            message: "login session not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: true,
        }
    }

    pub fn upstream_request_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamRequestFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_login_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamLoginFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_vip_check_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamVipCheckFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_play_url_empty() -> Self {
        Self {
            code: ErrorCode::UpstreamPlayUrlEmpty,
            message: "play url unavailable".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidRequest,
            message: msg.into(),
            status: StatusCode::BAD_REQUEST,
            is_admin: false,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InternalError,
            message: msg.into(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            is_admin: false,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: ErrorCode,
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let msg = if self.is_admin {
            self.message
        } else {
            match self.code {
                ErrorCode::UpstreamRequestFailed
                | ErrorCode::UpstreamLoginFailed
                | ErrorCode::UpstreamVipCheckFailed
                | ErrorCode::InternalError => "internal error".into(),
                _ => self.message,
            }
        };
        let body = ErrorBody {
            code: self.code,
            message: msg,
        };
        (self.status, Json(body)).into_response()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(e.to_string())
    }
}
