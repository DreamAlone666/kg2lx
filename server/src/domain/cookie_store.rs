use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CookieStore {
    pub items: BTreeMap<String, String>,
}

impl CookieStore {
    pub fn new() -> Self {
        Self {
            items: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.get(key).map(|v| v.as_str())
    }

    pub fn is_empty(&self, key: &str) -> bool {
        self.items.get(key).is_none_or(|v| v.is_empty())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.items.insert(key.into(), value.into());
    }

    pub fn merge_set_cookie(&mut self, header: &str) {
        if let Some((k, v)) = parse_set_cookie(header) {
            self.insert(k, v);
        }
    }

    pub fn to_cookie_header(&self) -> String {
        self.items
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl fmt::Display for CookieStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_cookie_header())
    }
}

fn parse_set_cookie(header: &str) -> Option<(String, String)> {
    let segment = header.split(';').next()?.trim();
    let eq_pos = segment.find('=')?;
    let key = segment[..eq_pos].trim().to_string();
    let value = segment[eq_pos + 1..].trim().to_string();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

pub fn merge_cookies_from_headers(store: &mut CookieStore, headers: &reqwest::header::HeaderMap) {
    for val in headers.get_all(reqwest::header::SET_COOKIE) {
        if let Ok(s) = val.to_str() {
            store.merge_set_cookie(s);
        }
    }
}

#[derive(Deserialize)]
struct GenericUpstreamBody {
    #[serde(default)]
    cookie: Option<String>,
}

pub fn merge_cookies_from_body_if_present(store: &mut CookieStore, body: &str) {
    if let Ok(parsed) = serde_json::from_str::<GenericUpstreamBody>(body)
        && let Some(ref cookie_str) = parsed.cookie
    {
        for pair in cookie_str.split(';') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                if !k.is_empty() {
                    store.insert(k, v);
                }
            }
        }
    }
}
