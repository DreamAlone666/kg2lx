use std::time::{SystemTime, UNIX_EPOCH};

use super::cookie_store::CookieStore;
use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
    Pending,
    /// **已弃用：不再有任何生产者。** 账号状态自本次改动起只由**登录态**决定，
    /// 刷新成功后一律是 `Active`；会员权益是**另一件**与账号状态无关的事，由
    /// `entitlements` 单独表达（见 `ProviderAccount`）——有没有权益既不改变账号
    /// 状态，也不再阻断任何动作。
    ///
    /// 变体必须**保留**，不能因为「没人写了」就删：落盘的账号 JSON 里可能已经有
    /// `"status": "expired"` 的记录，而 `repos/fs/account.rs` 的 `find_by_id` 把
    /// 反序列化失败直接映射成内部错误——删掉这个变体会让那些账号**整条读不出来**
    /// （在后台直接消失），且没有任何地方能区分「读不出来」与「不存在」。
    #[deprecated(
        note = "账号状态已与会员权益解耦：刷新成功一律 Active，没有任何路径会再写这个值。保留变体只为读回历史落盘记录，不要构造它。"
    )]
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
            // 这两个 allow 是刻意的：本函数必须覆盖**全部**变体，落盘记录里可能有
            // expired，漏了它 as_str 就不再穷尽。除了这里和下面那条枚举全部变体的测试，
            // 任何地方再碰 `Expired` 都会触发弃用警告——属性本身就是绊线。
            #[allow(deprecated)]
            Self::Expired => "expired",
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderAccount {
    pub account_id: String,
    pub provider: ProviderKind,
    pub upstream_userid: String,
    pub display_name: Option<String>,
    pub status: AccountStatus,
    /// 上游权益明细（`busi_vip` 的本地副本）。空数组 = 这个账号目前一条权益都没有，
    /// 那是**正常状态**，不是错误。
    ///
    /// 这是会员权益的**唯一**口径：账号级「有没有会员」的聚合布尔值（`vip_active`
    /// 与它的同族 `vip_type`）既冗余又低效——权益本就是并列的增量信息，聚合值只是
    /// 它的派生品，且会在上游新增一档权益时静默失真。要回答「还有没有权益」，看
    /// 这里有哪些**有名字**的权益。
    ///
    /// `default` 的理由与 `last_error_code` 完全相同：本次改动**之前**落盘的账号
    /// JSON 没有这个键，缺了它那些记录会整条反序列化失败——账号会在后台直接消失，
    /// 而不是少显示一项权益。读出为空数组。
    #[serde(default)]
    pub entitlements: Vec<VipEntitlement>,
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
    /// 这里必须列出 `Expired`：它仍在落盘记录的可能取值里，漏掉就等于放弃对它的守卫。
    #[allow(deprecated)]
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
    /// fixture 的键集照落盘形态构造，`last_error` 填一次失败留下的
    /// 旧值；**取值一律是合成占位**——fixture 只钉形态与派生性质，不携带实例数据。
    /// 落盘记录里**真的可能有** status 为 expired 的一行——这正是 Expired 变体被保留、
    /// 并用 #[deprecated] 标注而不是删除的全部理由。
    ///
    /// 在补上这条之前，那个理由只是一句注释，没有任何测试钉住它；而它保护的是一个会
    /// **静默**发生的故障：删掉变体后这些记录整个反序列化失败，AccountRepo::find_by_id
    /// 只能报内部错误，账号在后台表现为「消失」，且与「账号不存在」无从区分。
    ///
    /// fixture 的键集照落盘形态构造（含当时还在写的 vip_type / vip_active
    /// 两个键）；status 取 expired——它是落盘记录的合法取值之一，旧记录里未必出现。
    #[allow(deprecated)]
    #[test]
    fn reads_rows_whose_status_is_the_deprecated_expired() {
        let row = r#"{
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"upstream_userid":"0000000000",
			"display_name":null,
			"status":"expired",
			"vip_type":0,
			"vip_active":false,
			"cookies":{"items":{"userid":"0000000000"}},
			"last_refresh_at":1700000000,
			"last_success_at":1700000000,
			"last_error":null,
			"created_at":1600000000,
			"updated_at":1700000000
		}"#;

        let account: ProviderAccount =
            serde_json::from_str(row).expect("带 expired 的旧记录必须读得出来");
        assert_eq!(account.status, AccountStatus::Expired);
        // 读出来还不够：下一次 upsert 会把它写回去，那时线上字符串必须仍是 expired，
        // 否则前端 accountStatusLabel 查不到这个键、退回显示原始英文串。
        assert_eq!(serde_json::to_value(account.status).unwrap(), "expired");
    }

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

    /// 本次改动**之前**落盘的账号 JSON 没有 `entitlements` 这个键。
    ///
    /// 这条比 `last_error_code` 那条更要紧：缺 `default` 时是**整条**反序列化失败，
    /// 而 `FsAccountRepo::find_by_id` 把失败直接映射成内部错误——账号不是少显示一项
    /// 权益，是从后台整个消失（连「这个 id 不存在」都区分不出来）。fixture 与上一条
    /// 同源，额外带上 `last_error_code`，好让「缺的确实只有 entitlements 一个键」
    /// 这件事在断言里是清楚的。
    #[test]
    fn reads_rows_written_before_entitlements_existed() {
        let old_row = r#"{
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"upstream_userid":"0000000000",
			"display_name":null,
			"status":"active",
			"vip_type":0,
			"vip_active":false,
			"cookies":{"items":{"userid":"0000000000"}},
			"last_refresh_at":1700000000,
			"last_success_at":1700000000,
			"last_error":null,
			"last_error_code":null,
			"created_at":1600000000,
			"updated_at":1700000000
		}"#;
        let account: ProviderAccount = serde_json::from_str(old_row).unwrap();

        assert!(
            account.entitlements.is_empty(),
            "旧记录没有 entitlements 键，应读出空列表而不是整条读失败"
        );
        assert_eq!(
            account.upstream_userid, "0000000000",
            "同一条记录里别的字段不受影响"
        );
    }

    /// **删字段的兼容性**：`vip_active` / `vip_type` 被删掉之后，上面两条 fixture
    /// 里仍然带着这两个键——那正是本次改动之前落盘记录的键集。
    ///
    /// 这里把两种形态都钉住：**不带**这两个键的（本次改动之后写出的）与**带着**它们
    /// 的（本次改动之前写出的）都必须读得出来。与 `AccountStatus::Expired` 那种
    /// 「删枚举变体会让记录整条读不出来」的情形**不同**：serde 默认忽略未知键，
    /// 多出两个键不会影响反序列化（`deny_unknown_fields` 没有开，也不该为此开——
    /// 那会把升级兼容变成每次加字段都要论证的事）。缺了这条测试，删字段的代价就只是
    /// 「大家都以为没问题」，而真出问题时表现为账号从后台整条消失。
    #[test]
    fn reads_account_rows_with_and_without_the_removed_vip_columns() {
        let without_the_columns = r#"{
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"upstream_userid":"0000000000",
			"display_name":null,
			"status":"active",
			"cookies":{"items":{"userid":"0000000000"}},
			"last_refresh_at":1700000000,
			"last_success_at":1700000000,
			"last_error":null,
			"created_at":1600000000,
			"updated_at":1700000000
		}"#;
        let with_the_columns = r#"{
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"upstream_userid":"0000000000",
			"display_name":null,
			"status":"active",
			"vip_type":0,
			"vip_active":true,
			"cookies":{"items":{"userid":"0000000000"}},
			"last_refresh_at":1700000000,
			"last_success_at":1700000000,
			"last_error":null,
			"created_at":1600000000,
			"updated_at":1700000000
		}"#;

        let fresh: ProviderAccount =
            serde_json::from_str(without_the_columns).expect("不带已删键的账号 JSON 必须读得出来");
        let old: ProviderAccount =
            serde_json::from_str(with_the_columns).expect("带已删键的老账号 JSON 必须读得出来");

        // 两者只差那两个键，因此除了它们之外的一切都必须一致：这样「读出来了」才
        // 不是「读出来了一个空壳」。
        for account in [&fresh, &old] {
            assert_eq!(
                account.account_id,
                "acc_00000000-0000-4000-8000-000000000000"
            );
            assert_eq!(account.upstream_userid, "0000000000");
            assert_eq!(account.status, AccountStatus::Active);
            assert_eq!(account.last_refresh_at, Some(1700000000));
            assert_eq!(account.cookies.get("userid"), Some("0000000000"));
            assert!(
                account.entitlements.is_empty(),
                "两个 fixture 都没有 entitlements 键，各自读出空列表"
            );
        }
    }

    /// 权益明细必须能独立活过一轮序列化：它既要落盘、又会被详情接口读出，漏掉
    /// `Deserialize`（或漏掉某个字段）时接口照样能编译、能返回，错误要等到下一次
    /// 读盘才出现。
    #[test]
    fn entitlements_round_trip_through_json() {
        let account = ProviderAccount {
            account_id: "acc_test".into(),
            provider: ProviderKind::KugouLite,
            upstream_userid: "0000000000".into(),
            display_name: None,
            status: AccountStatus::Active,
            entitlements: vec![VipEntitlement {
                product_type: "svip".into(),
                active: true,
                begin_at: Some(1700000000),
                end_at: None,
            }],
            cookies: CookieStore::new(),
            last_refresh_at: None,
            last_success_at: None,
            last_error: None,
            last_error_code: None,
            created_at: 0,
            updated_at: 0,
        };

        let back: ProviderAccount =
            serde_json::from_value(serde_json::to_value(&account).unwrap()).unwrap();

        assert_eq!(back.entitlements.len(), 1);
        assert_eq!(back.entitlements[0].product_type, "svip");
        assert!(back.entitlements[0].active);
        assert_eq!(back.entitlements[0].begin_at, Some(1700000000));
        assert_eq!(
            back.entitlements[0].end_at, None,
            "None 必须原样回来，不能被写成 0（那是 1970 年，不是「上游没说」）"
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
