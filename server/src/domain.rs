use std::time::{SystemTime, UNIX_EPOCH};

pub mod account;
pub mod cookie_store;
pub mod entitlement;
pub mod event_log;
pub mod login_session;
pub mod provider;
pub mod renewal;
pub mod source;

pub fn now_ts() -> i64 {
    now_ts_ms() / 1000
}

pub fn now_ts_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
