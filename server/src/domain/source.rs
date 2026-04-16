use std::time::{SystemTime, UNIX_EPOCH};

use super::provider::ProviderKind;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub source_id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub name: String,
    pub enabled: bool,
    pub script_token: String,
    pub runtime_token: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
