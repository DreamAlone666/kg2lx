use std::env;

pub struct Config {
    pub listen_addr: String,
    pub public_base_url: String,
    pub admin_token: String,
    pub kugou_api_base_url: String,
    pub data_dir: String,
    pub upstream_timeout_ms: u64,
    pub source_name_prefix: String,
    pub refresh_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into()),
            public_base_url: env::var("PUBLIC_BASE_URL").expect("PUBLIC_BASE_URL is required"),
            admin_token: env::var("ADMIN_TOKEN").expect("ADMIN_TOKEN is required"),
            kugou_api_base_url: env::var("KUGOU_API_BASE_URL")
                .expect("KUGOU_API_BASE_URL is required"),
            data_dir: env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()),
            upstream_timeout_ms: env::var("UPSTREAM_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10000),
            source_name_prefix: env::var("SOURCE_NAME_PREFIX")
                .unwrap_or_else(|_| "Kugou Concept VIP".into()),
            refresh_interval_secs: env::var("REFRESH_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6 * 3600),
        }
    }
}
