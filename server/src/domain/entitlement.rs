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

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EntitlementSnapshot {
    /// None means no confirmed snapshot. Some(empty) preserves a confirmed empty response.
    #[serde(rename = "entitlements", skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<VipEntitlement>>,
    #[serde(rename = "entitlements_synced_at")]
    pub synced_at: Option<i64>,
    #[serde(rename = "entitlements_sync_failed")]
    pub sync_failed: bool,
}
use crate::domain::renewal::SvipEvidence;

pub struct EntitlementObservation {
    pub items: Vec<VipEntitlement>,
    pub svip: SvipEvidence,
}
