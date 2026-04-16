use super::provider::ProviderKind;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeLog {
    pub log_id: String,
    pub source_id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub action: String,
    pub request_hash: String,
    pub album_audio_id: Option<String>,
    pub requested_quality: String,
    pub upstream_endpoint: String,
    pub ok: bool,
    pub status_code: u16,
    pub latency_ms: u128,
    pub error: Option<String>,
    pub created_at: i64,
}
