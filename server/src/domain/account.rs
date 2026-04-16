use std::time::{SystemTime, UNIX_EPOCH};

use super::cookie_store::CookieStore;
use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
    Pending,
    Expired,
    Disabled,
    LoginFailed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderAccount {
    pub account_id: String,
    pub provider: ProviderKind,
    pub upstream_userid: String,
    pub display_name: Option<String>,
    pub status: AccountStatus,
    pub vip_type: i32,
    pub vip_active: bool,
    pub cookies: CookieStore,
    pub last_refresh_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
