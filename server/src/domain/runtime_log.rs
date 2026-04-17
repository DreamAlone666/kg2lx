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

impl RuntimeLog {
    pub fn sanitized(mut self) -> Self {
        self.error = sanitize_runtime_log_error(self.error.as_deref());
        self
    }
}

pub fn sanitize_runtime_log_error(error: Option<&str>) -> Option<String> {
    error.map(|_| "upstream error".to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeLogView {
    All,
    Errors,
}

#[derive(Debug, Clone)]
pub struct ListRuntimeLogsQuery {
    pub source_id: String,
    pub limit: usize,
    pub view: RuntimeLogView,
}
