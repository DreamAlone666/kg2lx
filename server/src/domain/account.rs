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

impl AccountStatus {
    /// 对外表示（snake_case）。
    ///
    /// 此前各处用 `format!("{:?}", ..).to_lowercase()` 生成：四个单词状态恰好与前端词典
    /// 一致，唯独 `LoginFailed` 会产出 `loginfailed`，而前端查的是 `login_failed`，
    /// 于是「登录失败」在界面上回退成原始英文串。改为显式映射，并由
    /// `as_str_matches_serde` 锁死它与 serde 输出的一致性。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Pending => "pending",
            Self::Expired => "expired",
            Self::Disabled => "disabled",
            Self::LoginFailed => "login_failed",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `as_str` 是手写映射，而线上格式由 `#[serde(rename_all = "snake_case")]` 决定。
    /// 两者一旦漂移，前端词典会静默回退成原始英文串（`loginfailed` 就是这么漏出去的）。
    #[test]
    fn as_str_matches_serde() {
        for status in [
            AccountStatus::Active,
            AccountStatus::Pending,
            AccountStatus::Expired,
            AccountStatus::Disabled,
            AccountStatus::LoginFailed,
        ] {
            let via_serde = serde_json::to_value(status).unwrap();
            assert_eq!(
                via_serde.as_str().unwrap(),
                status.as_str(),
                "{status:?} 的 serde 表示与 as_str 不一致"
            );
        }
    }
}
