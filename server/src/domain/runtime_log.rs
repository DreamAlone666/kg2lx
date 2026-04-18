use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogStage {
    Precheck,
    EnsureDfid,
    RefreshLogin,
    FetchMusicUrl,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeLog {
    pub log_id: String,
    pub source_id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub action: String,
    pub track_title: Option<String>,
    pub artist_name: Option<String>,
    pub album_name: Option<String>,
    pub request_hash: String,
    pub album_audio_id: Option<String>,
    pub requested_quality: String,
    pub upstream_endpoint: String,
    #[serde(default = "default_runtime_log_stage")]
    pub stage: RuntimeLogStage,
    #[serde(default)]
    pub refresh_attempted: bool,
    #[serde(default)]
    pub retry_count: u8,
    pub ok: bool,
    #[serde(default)]
    pub status_code: Option<u16>,
    #[serde(default)]
    pub error_code: Option<String>,
    pub latency_ms: u128,
    pub error: Option<String>,
    pub created_at: i64,
}

fn default_runtime_log_stage() -> RuntimeLogStage {
    RuntimeLogStage::FetchMusicUrl
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
