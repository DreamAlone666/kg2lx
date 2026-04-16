use std::time::{SystemTime, UNIX_EPOCH};

use super::cookie_store::CookieStore;
use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginSessionStatus {
    Pending,
    WaitingScan,
    WaitingConfirm,
    Authorized,
    Bound,
    Expired,
    Failed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoginSession {
    pub session_id: String,
    pub provider: ProviderKind,
    pub status: LoginSessionStatus,
    pub qr_key: String,
    pub qr_url: String,
    pub qr_base64: Option<String>,
    pub temp_cookies: CookieStore,
    pub bound_account_id: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub updated_at: i64,
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
