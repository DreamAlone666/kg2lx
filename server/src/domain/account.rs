use super::cookie_store::CookieStore;
use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
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
            Self::LoginFailed => "login_failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderAccount {
    pub account_id: String,
    pub provider: ProviderKind,
    pub upstream_userid: String,
    pub status: AccountStatus,
    pub cookies: CookieStore,
    pub credential_version: i64,
    pub last_refresh_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_matches_serde() {
        for status in [AccountStatus::Active, AccountStatus::LoginFailed] {
            assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
        }
    }
}
