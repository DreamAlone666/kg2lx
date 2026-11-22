use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::NaiveDate;
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde_json::Value;

use super::{
    KugouLiteClient, VipBusinessEntry, decode_vip_snapshot, requires_human_verification,
    upstream_code,
};
use crate::domain::cookie_store::{
    CookieStore, merge_cookies_from_body_if_present, merge_cookies_from_headers,
};
use crate::domain::entitlement::EntitlementObservation;
use crate::domain::renewal::{GrantStep, SvipEvidence, upstream_svip_timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Timeout,
    Unknown,
    Rejected,
    RateLimited,
    Verification,
    Gateway,
}

#[derive(Clone, Copy, Debug)]
pub struct UpstreamFailure {
    pub kind: FailureKind,
    pub code: Option<i64>,
    pub http_status: Option<u16>,
    pub message: &'static str,
}

impl UpstreamFailure {
    pub fn write_may_have_applied(self) -> bool {
        matches!(
            self.kind,
            FailureKind::Timeout | FailureKind::Unknown | FailureKind::Gateway
        )
    }
}

pub struct UpstreamReply<T> {
    pub result: Result<T, UpstreamFailure>,
    pub cookies: CookieStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantResult {
    Confirmed { added_hours: Option<u32> },
    AlreadyDone { code: i64 },
}

#[derive(Debug, Default)]
pub struct DailyRecord {
    pub days: BTreeMap<NaiveDate, GrantStep>,
    pub truncated: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RecordEntry {
    Found(GrantStep),
    Absent,
    Uncovered,
}

impl DailyRecord {
    pub fn entry(&self, day: NaiveDate) -> RecordEntry {
        if let Some(step) = self.days.get(&day) {
            RecordEntry::Found(*step)
        } else if !self.truncated
            || self
                .days
                .first_key_value()
                .is_some_and(|(first, _)| day > *first)
        {
            RecordEntry::Absent
        } else {
            RecordEntry::Uncovered
        }
    }
}

#[async_trait]
pub trait EntitlementUpstream: Send + Sync {
    async fn snapshot(
        &self,
        cookies: &CookieStore,
        now: i64,
    ) -> UpstreamReply<EntitlementObservation>;
    async fn grant(
        &self,
        cookies: &CookieStore,
        step: GrantStep,
        day: NaiveDate,
        timeout: Duration,
    ) -> UpstreamReply<GrantResult>;
    async fn daily_record(&self, cookies: &CookieStore) -> UpstreamReply<DailyRecord>;
}

#[async_trait]
impl EntitlementUpstream for KugouLiteClient {
    async fn snapshot(
        &self,
        cookies: &CookieStore,
        now: i64,
    ) -> UpstreamReply<EntitlementObservation> {
        let request = self
            .client
            .get(format!("{}/youth/union/vip", self.base_url))
            .timeout(self.default_timeout);
        exchange(request, cookies, |body, status| {
            let response = business_response(body, status)?;
            let snapshot =
                decode_vip_snapshot(body).map_err(|_| response.invalid("invalid VIP snapshot"))?;
            let entries = snapshot.data.unwrap().busi_vip;
            let svip = if snapshot.status == Some(1) && response.code.is_none() {
                svip_evidence(&entries, now)
            } else {
                SvipEvidence::Unknown
            };
            Ok(EntitlementObservation {
                items: entries
                    .iter()
                    .map(VipBusinessEntry::to_entitlement)
                    .collect(),
                svip,
            })
        })
        .await
    }

    async fn grant(
        &self,
        cookies: &CookieStore,
        step: GrantStep,
        day: NaiveDate,
        timeout: Duration,
    ) -> UpstreamReply<GrantResult> {
        let url = match step {
            GrantStep::Claim => format!("{}/youth/day/vip?receive_day={day}", self.base_url),
            GrantStep::Upgrade => format!("{}/youth/day/vip/upgrade", self.base_url),
        };
        let request = self.entitlement_writes.post(url).timeout(timeout);
        exchange(request, cookies, |body, status| {
            parse_grant(body, status, step)
        })
        .await
    }

    async fn daily_record(&self, cookies: &CookieStore) -> UpstreamReply<DailyRecord> {
        let request = self
            .client
            .get(format!("{}/youth/month/vip/record", self.base_url))
            .timeout(self.default_timeout);
        exchange(request, cookies, |body, status| {
            let mut response = business_response(body, status)?;
            let data = response.data.take().unwrap_or_default();
            if response.status != Some(1) || response.code.is_some() {
                return Err(response.invalid("missing successful daily record status"));
            }
            parse_daily_record(data)
                .ok_or_else(|| response.invalid("unrecognised daily record response"))
        })
        .await
    }
}

async fn exchange<T>(
    request: RequestBuilder,
    cookies: &CookieStore,
    parse: impl FnOnce(&str, u16) -> Result<T, UpstreamFailure>,
) -> UpstreamReply<T> {
    let mut merged = cookies.clone();
    let mut http_status = None;
    let received = async {
        let response = request
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await?;
        http_status = Some(response.status().as_u16());
        merge_cookies_from_headers(&mut merged, response.headers());
        response.text().await
    }
    .await;
    let result = match received {
        Ok(body) => {
            merge_cookies_from_body_if_present(&mut merged, &body);
            parse(&body, http_status.unwrap())
        }
        Err(error) => {
            let kind = match http_status {
                Some(429) => FailureKind::RateLimited,
                Some(502) => FailureKind::Gateway,
                _ if error.is_timeout() => FailureKind::Timeout,
                _ => FailureKind::Unknown,
            };
            Err(UpstreamFailure {
                kind,
                code: None,
                http_status,
                message: if error.is_timeout() {
                    "entitlement request timed out"
                } else {
                    "entitlement request ended without a response"
                },
            })
        }
    };
    // A challenge may return cookies for the verification flow. Leave the login cookies alone.
    if result.as_ref().is_err_and(|failure| {
        matches!(
            failure.kind,
            FailureKind::Verification | FailureKind::Gateway
        )
    }) {
        merged = cookies.clone();
    }
    UpstreamReply {
        result,
        cookies: merged,
    }
}

struct BusinessResponse {
    status: Option<i64>,
    code: Option<i64>,
    http: u16,
    data: Option<Value>,
}

impl BusinessResponse {
    fn invalid(&self, message: &'static str) -> UpstreamFailure {
        UpstreamFailure {
            kind: FailureKind::Unknown,
            code: self.code,
            http_status: Some(self.http),
            message,
        }
    }
}

fn business_response(body: &str, http: u16) -> Result<BusinessResponse, UpstreamFailure> {
    let mut parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let code = ["error_code", "errcode", "code"]
        .iter()
        .filter_map(|key| upstream_code(&parsed, key))
        .find(|&code| code != 0);
    let response = BusinessResponse {
        status: parsed.get("status").and_then(Value::as_i64),
        code,
        http,
        data: parsed.get_mut("data").map(Value::take),
    };
    let failure = if requires_human_verification(Some(&parsed), body)
        || (parsed.is_null() && requires_human_verification(None, body))
    {
        Some((FailureKind::Verification, "upstream requires verification"))
    } else if http == 502 {
        Some((
            FailureKind::Gateway,
            "entitlement gateway returned HTTP 502",
        ))
    } else if http == 429 || code == Some(429) {
        Some((
            FailureKind::RateLimited,
            "upstream rate limited entitlement request",
        ))
    } else if (200..300).contains(&http) && parsed.is_object() {
        None
    } else {
        Some((
            FailureKind::Unknown,
            "entitlement response did not confirm an outcome",
        ))
    };
    match failure {
        Some((kind, message)) => Err(UpstreamFailure {
            kind,
            code,
            http_status: Some(http),
            message,
        }),
        None => Ok(response),
    }
}

fn parse_grant(body: &str, status: u16, step: GrantStep) -> Result<GrantResult, UpstreamFailure> {
    let response = business_response(body, status)?;
    let expected = match step {
        GrantStep::Claim => 131001,
        GrantStep::Upgrade => 297002,
    };
    if response.code == Some(expected) {
        return Ok(GrantResult::AlreadyDone { code: expected });
    }
    if response.status == Some(1) && response.code.is_none() {
        let added_hours = response
            .data
            .as_ref()
            .and_then(|data| data.get("recharge_hours"))
            .and_then(Value::as_u64)
            .filter(|hours| *hours == 24)
            .map(|_| 24);
        return Ok(GrantResult::Confirmed { added_hours });
    }
    if response.status == Some(0) && response.code.is_some() {
        return Err(UpstreamFailure {
            kind: FailureKind::Rejected,
            ..response.invalid("upstream rejected entitlement write")
        });
    }
    Err(response.invalid("unrecognised entitlement write response"))
}

fn svip_evidence(entries: &[VipBusinessEntry], now: i64) -> SvipEvidence {
    if entries.is_empty() {
        return SvipEvidence::Inactive;
    }
    let mut entries = entries
        .iter()
        .filter(|entry| entry.product_type.as_ref().and_then(Value::as_str) == Some("svip"));
    let Some(entry) = entries.next() else {
        return SvipEvidence::Unknown;
    };
    if entries.next().is_some() {
        return SvipEvidence::Unknown;
    }
    let end_at = match entry.vip_end_time.as_ref() {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value.is_empty() => None,
        Some(value) => match value.as_str().and_then(upstream_svip_timestamp) {
            Some(timestamp) => Some(timestamp),
            None => return SvipEvidence::Unknown,
        },
    };
    match (entry.is_vip, end_at) {
        (Some(1), Some(end_at)) if end_at > now => SvipEvidence::ActiveUntil { end_at },
        (Some(0), end_at) if end_at.is_none_or(|end_at| end_at <= now) => SvipEvidence::Inactive,
        _ => SvipEvidence::Unknown,
    }
}

#[derive(Deserialize)]
struct RecordRow {
    #[serde(alias = "day", alias = "date", alias = "recharge_day")]
    receive_day: NaiveDate,
    vip_type: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RecordData {
    Rows(Vec<RecordRow>),
    Page {
        #[serde(alias = "list")]
        records: Vec<RecordRow>,
        total: Option<usize>,
        #[serde(default)]
        has_more: bool,
    },
}

fn parse_daily_record(data: Value) -> Option<DailyRecord> {
    let (rows, truncated) = match serde_json::from_value(data).ok()? {
        RecordData::Rows(rows) => (rows, false),
        RecordData::Page {
            records,
            total,
            has_more,
        } => {
            if total.is_some_and(|total| total < records.len()) {
                return None;
            }
            let truncated = has_more || total.is_some_and(|total| total > records.len());
            (records, truncated)
        }
    };
    let mut record = DailyRecord {
        truncated: truncated || rows.len() >= 100,
        ..DailyRecord::default()
    };
    for row in rows {
        let step = match row.vip_type.as_str() {
            "tvip" => GrantStep::Claim,
            "svip" => GrantStep::Upgrade,
            _ => return None,
        };
        if record
            .days
            .insert(row.receive_day, step)
            .is_some_and(|previous| previous != step)
        {
            return None;
        }
    }
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(n: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2035, 2, n).unwrap()
    }

    #[test]
    fn only_a_valid_record_can_supply_positive_or_negative_evidence() {
        let record = parse_daily_record(json!({
            "total": 3, "records": [
                {"receive_day": "2035-02-03", "vip_type": "tvip"},
                {"receive_day": "2035-02-02", "vip_type": "svip"}
            ]
        }))
        .unwrap();
        assert_eq!(record.entry(day(3)), RecordEntry::Found(GrantStep::Claim));
        assert_eq!(record.entry(day(2)), RecordEntry::Found(GrantStep::Upgrade));
        assert_eq!(record.entry(day(4)), RecordEntry::Absent);
        assert_eq!(record.entry(day(1)), RecordEntry::Uncovered);
        assert_eq!(
            parse_daily_record(json!([])).unwrap().entry(day(3)),
            RecordEntry::Absent
        );
        assert_eq!(
            parse_daily_record(json!({"list":[], "has_more":true}))
                .unwrap()
                .entry(day(3)),
            RecordEntry::Uncovered,
        );
        for malformed in [
            json!({"empty":true}),
            json!({"records":[{"vip_type":"svip"}]}),
            json!([{"receive_day":"2035-02-30","vip_type":"svip"}]),
            json!([{"receive_day":"2035-02-03","vip_type":"unexpected"}]),
            json!({"list":[{"date":"2035-02-03","vip_type":"svip"}],"total":0}),
            json!([
                {"receive_day":"2035-02-02","vip_type":"svip"},
                {"receive_day":"2035-02-02","vip_type":"tvip"},
            ]),
        ] {
            assert!(parse_daily_record(malformed).is_none());
        }
    }

    #[test]
    fn scheduling_evidence_requires_unambiguous_svip_and_a_matching_expiry() {
        let now = upstream_svip_timestamp("2035-02-01 12:00:00").unwrap();
        let observation = |entries: Value| {
            let response =
                serde_json::to_string(&json!({"status":1,"data":{"busi_vip":entries}})).unwrap();
            svip_evidence(
                &decode_vip_snapshot(&response)
                    .unwrap()
                    .data
                    .unwrap()
                    .busi_vip,
                now,
            )
        };
        assert_eq!(
            observation(
                json!([{"product_type":"tvip","is_vip":1,"vip_end_time":"2035-02-02 12:00:00"}])
            ),
            SvipEvidence::Unknown
        );
        assert_eq!(
            observation(
                json!([{"product_type":"svip","is_vip":1,"vip_end_time":"2035-02-02 12:00:00"}])
            ),
            SvipEvidence::ActiveUntil {
                end_at: now + 86_400
            },
        );
        assert_eq!(
            observation(
                json!([{"product_type":"svip","is_vip":0,"vip_end_time":"2035-02-01 11:59:59"}])
            ),
            SvipEvidence::Inactive,
        );
        for entries in [
            json!([{"product_type":"svip","is_vip":0,"vip_end_time":"2035-02-02 12:00:00"}]),
            json!([{"product_type":"svip","is_vip":1,"vip_end_time":"2035-02-01 11:59:59"}]),
            json!([{"product_type":"svip","is_vip":1}]),
            json!([{"product_type":"svip","is_vip":0},{"product_type":"svip","is_vip":0}]),
        ] {
            assert_eq!(observation(entries), SvipEvidence::Unknown);
        }
    }

    #[test]
    fn business_codes_are_step_specific_and_a_bare_gateway_failure_is_uncertain() {
        for (step, code) in [(GrantStep::Claim, 131001), (GrantStep::Upgrade, 297002)] {
            let body = format!(r#"{{"status":0,"error_code":"{code}"}}"#);
            assert_eq!(
                parse_grant(&body, 200, step).unwrap(),
                GrantResult::AlreadyDone { code }
            );
            let other_step = if step == GrantStep::Claim {
                GrantStep::Upgrade
            } else {
                GrantStep::Claim
            };
            assert!(parse_grant(&body, 200, other_step).is_err());
        }
        let failure =
            parse_grant(r#"{"status":0,"errcode":20028}"#, 502, GrantStep::Upgrade).unwrap_err();
        assert_eq!(failure.kind, FailureKind::Verification);
        assert!(!failure.write_may_have_applied());

        let failure = parse_grant("gateway error", 502, GrantStep::Upgrade).unwrap_err();
        assert_eq!(failure.kind, FailureKind::Gateway);
        assert!(failure.write_may_have_applied());
        assert_eq!(
            parse_grant("", 429, GrantStep::Upgrade).unwrap_err().kind,
            FailureKind::RateLimited
        );
        assert_eq!(
            parse_grant(
                r#"{"status":1,"data":{"recharge_hours":24}}"#,
                200,
                GrantStep::Upgrade
            )
            .unwrap(),
            GrantResult::Confirmed {
                added_hours: Some(24)
            },
        );
    }
}
