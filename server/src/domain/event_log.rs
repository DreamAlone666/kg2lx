use crate::domain::now_ts_ms;
use crate::domain::renewal::{RenewalOutcome, RenewalReason};
use crate::error::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    MusicUrl,
    TokenRefresh,
    AccountLogin,
    Entitlement,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MusicUrl => "music_url",
            Self::TokenRefresh => "token_refresh",
            Self::AccountLogin => "account_login",
            Self::Entitlement => "entitlement",
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTrigger {
    Runtime,
    Admin,
    Qr,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventContext {
    pub account_id: Option<String>,
    pub source_id: Option<String>,
    pub correlation_id: String,
    pub trigger: EventTrigger,
}

impl EventContext {
    pub fn new(
        trigger: EventTrigger,
        account_id: Option<String>,
        source_id: Option<String>,
    ) -> Self {
        Self {
            account_id,
            source_id,
            correlation_id: format!("req_{}", uuid::Uuid::new_v4()),
            trigger,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventLog {
    pub log_id: String,
    #[serde(flatten)]
    pub context: EventContext,
    /// 事件创建时间戳（Unix 毫秒）。
    pub created_at_ms: i64,
    pub ok: bool,
    pub latency_ms: Option<u64>,
    pub error_code: Option<String>,
    #[serde(flatten)]
    pub event: Event,
}

impl EventLog {
    pub fn new(
        context: EventContext,
        event: Event,
        latency_ms: Option<u64>,
        error: Option<ErrorCode>,
    ) -> Self {
        Self {
            log_id: format!("log_{}", uuid::Uuid::new_v4()),
            context,
            created_at_ms: now_ts_ms(),
            ok: error.is_none(),
            latency_ms,
            error_code: error.map(|code| code.as_str().into()),
            event,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event_type", content = "detail", rename_all = "snake_case")]
pub enum Event {
    MusicUrl(Box<MusicUrlEvent>),
    TokenRefresh {
        reason: RefreshReason,
        upstream_endpoint: String,
        human_verification: bool,
    },
    AccountLogin {
        outcome: LoginOutcome,
    },
    Entitlement {
        action: EntitlementAction,
        #[serde(flatten, skip_serializing_if = "Option::is_none")]
        observation: Option<EntitlementEvent>,
    },
}

impl Event {
    pub fn event_type(&self) -> EventType {
        match self {
            Self::MusicUrl(_) => EventType::MusicUrl,
            Self::TokenRefresh { .. } => EventType::TokenRefresh,
            Self::AccountLogin { .. } => EventType::AccountLogin,
            Self::Entitlement { .. } => EventType::Entitlement,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    Ttl,
    Rejected,
    Device,
    Manual,
    QrLogin,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginOutcome {
    Bound,
    Rebound,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementAction {
    Sync,
    Claim,
    Upgrade,
    ReconcileRecord,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EntitlementEvent {
    pub reason: RenewalReason,
    pub upstream_day: chrono::NaiveDate,
    pub outcome: RenewalOutcome,
    pub upstream_code: Option<i64>,
    pub http_status: Option<u16>,
    pub human_verification: bool,
    pub retry_at: Option<i64>,
    pub propagating_until: Option<i64>,
    pub estimated_end_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MusicUrlStage {
    Precheck,
    FetchMusicUrl,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MusicUrlEvent {
    pub stage: MusicUrlStage,
    pub track_title: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub request_hash: String,
    pub album_audio_id: Option<String>,
    pub requested_quality: String,
    pub attempted_quality: Option<String>,
    pub quality_fallback_index: u8,
    pub retry_count: u8,
    pub reported_types: Option<serde_json::Value>,
    pub upstream_endpoint: Option<String>,
    pub status_code: Option<u16>,
    pub human_verification: bool,
    pub upstream_quality: Option<String>,
    // Keep null observations on the wire. Zero and empty arrays are observed values.
    pub upstream_status: Option<i64>,
    pub upstream_priv_status: Option<i64>,
    pub upstream_fail_process: Option<Vec<String>>,
    pub upstream_ext_name: Option<String>,
    pub upstream_file_size: Option<i64>,
    pub upstream_bit_rate: Option<i64>,
    pub upstream_time_length: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogView {
    All,
    Errors,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredEventLog {
    pub seq: i64,
    #[serde(flatten)]
    pub log: EventLog,
}

#[derive(Debug, Clone)]
pub struct ListGlobalEvents {
    pub account_id: Option<String>,
    pub source_id: Option<String>,
    pub correlation_id: Option<String>,
    pub event_type: Option<EventType>,
    pub view: LogView,
    pub before_seq: Option<i64>,
    pub after_seq: Option<i64>,
    pub limit: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_created_at_ms_uses_unix_milliseconds() {
        let log = EventLog::new(
            EventContext::new(EventTrigger::Admin, Some("acc_1".into()), None),
            Event::AccountLogin {
                outcome: LoginOutcome::Bound,
            },
            None,
            None,
        );
        assert!(
            log.created_at_ms >= 1_000_000_000_000,
            "expected millisecond timestamp, got {}",
            log.created_at_ms
        );
    }
}
