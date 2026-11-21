use std::time::{SystemTime, UNIX_EPOCH};

use super::cookie_store::CookieStore;
use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
    Pending,
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
            Self::Disabled => "disabled",
            Self::LoginFailed => "login_failed",
        }
    }
}

/// 上游会员权益的本地副本，一条对应上游 `busi_vip[]` 里的一项。
///
/// 保留上游「**并列多条**」的形状，不做「只留最晚到期的那条」之类的合并：同一个
/// 账号可以同时持有互不包含的权益（概念版 VIP 与畅听 VIP 是两条独立权益），合并
/// 会让后台少显示一条，而没有任何一层会报错。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VipEntitlement {
    /// 权益标识（实测 `svip` / `tvip` / `qvip` / `dvip`）。**刻意不收敛成枚举**：
    /// 上游新增一档时后台要原样显示，而不是因为本仓认不出就丢掉这条权益。
    pub product_type: String,
    /// 上游的 `is_vip` 原样落地。**不要**改成按 `end_at` 反推：上游已经用
    /// `is_vip` 直接表态了，时间戳只是它的另一份描述；两者不同步时（到期前后各
    /// 自的刷新节奏不一样）以 `is_vip` 为准。用时间反推等于拿一个派生值取代上游
    /// 的直接表态，而与上游相反的结论不会有任何一层报错。
    pub active: bool,
    /// 起止时刻（秒级 Unix ts）。`None` = 上游没给或读不出来，不是 0（1970 年）。
    pub begin_at: Option<i64>,
    pub end_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ProviderAccount {
    pub account_id: String,
    pub provider: ProviderKind,
    pub upstream_userid: String,
    pub display_name: Option<String>,
    pub status: AccountStatus,
    /// None is an unknown snapshot. An empty list is a confirmed upstream response.
    pub entitlements: Option<Vec<VipEntitlement>>,
    pub cookies: CookieStore,
    pub last_refresh_at: Option<i64>,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_code: Option<String>,
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

    #[test]
    fn as_str_matches_serde() {
        for status in [
            AccountStatus::Active,
            AccountStatus::Pending,
            AccountStatus::Disabled,
            AccountStatus::LoginFailed,
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
        }
    }
}
