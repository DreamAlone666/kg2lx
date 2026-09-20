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
    /// 与 `last_error` 成对：`last_error` 是写给运维排查的散文，`last_error_code` 是
    /// 给界面查中文词典用的稳定机器标识。两者缺一，界面就只能把 `UPSTREAM_LOGIN_FAILED: …`
    /// 这样的英文诊断串原样摆给用户看——后台「最后一次错误」此前正是如此。
    ///
    /// `default` 是为了让本次改动**之前**落盘的账号 JSON 仍能读进来（它们没有这个键），
    /// 读出为 None；此时界面退回只显示 `last_error` 原文，而不是整条记录读不出来。
    #[serde(default)]
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

    /// 本次改动**之前**落盘的账号 JSON 没有 `last_error_code` 这个键。
    ///
    /// 缺了 `#[serde(default)]`，既有记录会整条反序列化失败——不是丢一个字段，而是
    /// 账号在后台直接消失（`AccountRepo::find_by_id` 只能报错）。这条断言钉住升级兼容，
    /// fixture 的键集照落盘形态构造，仅把 `last_error`
    /// 填成一次失败留下的旧值。
    #[test]
    fn reads_rows_written_before_last_error_code_existed() {
        let old_row = r#"{
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"upstream_userid":"0000000000",
			"display_name":null,
			"status":"login_failed",
			"vip_type":0,
			"vip_active":false,
			"cookies":{"items":{"userid":"0000000000"}},
			"last_refresh_at":1700000000,
			"last_success_at":1700000000,
			"last_error":"UPSTREAM_LOGIN_FAILED: token expired",
			"created_at":1600000000,
			"updated_at":1700000000
		}"#;
        let account: ProviderAccount = serde_json::from_str(old_row).unwrap();
        assert_eq!(
            account.last_error_code, None,
            "旧记录没有 code 键，应读出 None 而不是读失败"
        );
        assert_eq!(
            account.last_error.as_deref(),
            Some("UPSTREAM_LOGIN_FAILED: token expired"),
            "散文原文必须原样保留，否则旧记录的排查线索就丢了"
        );
    }

    /// 两个字段要各自独立地活过一轮序列化——`last_error_code` 落在 JSON 里但没有
    /// `default` 之外的写入路径时，很容易只写一侧而漏掉另一侧。
    #[test]
    fn round_trips_last_error_pair() {
        let row = serde_json::json!({
            "account_id": "acc_test",
            "provider": "kugou_lite",
            "upstream_userid": "userid_test",
            "display_name": null,
            "status": "login_failed",
            "vip_type": 0,
            "vip_active": false,
            "cookies": {"items": {}},
            "last_refresh_at": null,
            "last_success_at": null,
            "last_error": "upstream login failed",
            "last_error_code": "UPSTREAM_LOGIN_FAILED",
            "created_at": 0,
            "updated_at": 0,
        });
        let account: ProviderAccount = serde_json::from_value(row).unwrap();
        assert_eq!(account.last_error.as_deref(), Some("upstream login failed"));
        assert_eq!(
            account.last_error_code.as_deref(),
            Some("UPSTREAM_LOGIN_FAILED")
        );

        let back: ProviderAccount =
            serde_json::from_value(serde_json::to_value(&account).unwrap()).unwrap();
        assert_eq!(back.last_error.as_deref(), Some("upstream login failed"));
        assert_eq!(
            back.last_error_code.as_deref(),
            Some("UPSTREAM_LOGIN_FAILED")
        );
    }
}
