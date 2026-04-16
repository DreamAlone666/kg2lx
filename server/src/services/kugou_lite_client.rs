use std::time::Duration;

use crate::config::Config;
use crate::domain::cookie_store::{merge_cookies_from_body_if_present, merge_cookies_from_headers, CookieStore};
use crate::error::AppError;

#[derive(Debug, serde::Deserialize)]
struct QrKeyResponse {
    #[serde(default)]
    data: Option<QrKeyData>,
}

#[derive(Debug, serde::Deserialize)]
struct QrKeyData {
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCreateResponse {
    #[serde(default)]
    data: Option<QrCreateData>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCreateData {
    #[serde(default)]
    qrurl: Option<String>,
    #[serde(default)]
    qrimg: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCheckResponse {
    #[serde(default)]
    data: Option<QrCheckData>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    userid: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCheckData {
    #[serde(default)]
    status: Option<i64>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    userid: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct VipDetailResponse {
    #[serde(default)]
    data: Option<VipDetailData>,
}

#[derive(Debug, serde::Deserialize)]
struct VipDetailData {
    #[serde(default)]
    vip_type: Option<i64>,
    #[serde(default)]
    is_vip: Option<i64>,
    #[serde(default)]
    is_music_vip: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct SongUrlResponse {
    #[serde(default)]
    data: Option<SongUrlData>,
}

#[derive(Debug, serde::Deserialize)]
struct SongUrlData {
    #[serde(default)]
    url: Option<String>,
}

pub struct QrKeyResult {
    pub qr_key: String,
    pub cookies: CookieStore,
}

pub struct QrCreateResult {
    pub qr_url: String,
    pub qr_base64: Option<String>,
    pub cookies: CookieStore,
}

pub struct QrPollResult {
    pub cookies: CookieStore,
    pub status_code: i64,
    pub message: Option<String>,
}

pub struct RefreshResult {
    pub cookies: CookieStore,
}

pub struct VipStatusResult {
    pub vip_type: i32,
    pub vip_active: bool,
    pub cookies: CookieStore,
}

pub struct MusicUrlRequest {
    pub hash: String,
    pub album_audio_id: Option<String>,
    pub quality: String,
}

pub struct MusicUrlResult {
    pub url: String,
    pub cookies: CookieStore,
    pub status_code: u16,
    pub auth_failed: bool,
}

pub struct KugouLiteClient {
    base_url: String,
    client: reqwest::Client,
}

impl KugouLiteClient {
    pub fn new(config: &Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.upstream_timeout_ms))
            .build()
            .expect("build reqwest client");
        Self {
            base_url: config.kugou_api_base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    pub async fn request_qr_key(&self, cookies: &CookieStore) -> Result<QrKeyResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/login/qr/key", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr key: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_request_failed(format!("qr key body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: QrKeyResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_request_failed(format!("parse qr key: {}", e)))?;
        let qr_key = parsed
            .data
            .and_then(|d| d.key)
            .ok_or_else(|| AppError::upstream_login_failed("no qr key in response"))?;

        Ok(QrKeyResult {
            qr_key,
            cookies: merged,
        })
    }

    pub async fn create_qr_code(
        &self,
        qr_key: &str,
        cookies: &CookieStore,
    ) -> Result<QrCreateResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/login/qr/create?key={}&qrimg=1", self.base_url, qr_key);
        let resp2 = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr create: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp2.headers());
        let body2 = resp2.text().await.map_err(|e| AppError::upstream_request_failed(format!("qr create body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body2);

        let parsed2: QrCreateResponse = serde_json::from_str(&body2)
            .map_err(|e| AppError::upstream_request_failed(format!("parse qr create: {}", e)))?;
        let data2 = parsed2
            .data
            .ok_or_else(|| AppError::upstream_login_failed("no qr create data"))?;

        Ok(QrCreateResult {
            qr_url: data2.qrurl.unwrap_or_default(),
            qr_base64: data2.qrimg,
            cookies: merged,
        })
    }

    pub async fn poll_qr_login(
        &self,
        qr_key: &str,
        cookies: &CookieStore,
    ) -> Result<QrPollResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/login/qr/check?key={}", self.base_url, qr_key);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr check: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_request_failed(format!("qr check body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: QrCheckResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_request_failed(format!("parse qr check: {}", e)))?;

        let status = parsed.data.as_ref().and_then(|d| d.status).unwrap_or(-1);
        let userid = parsed
            .data
            .as_ref()
            .and_then(|d| d.userid.as_deref())
            .or(parsed.userid.as_deref())
            .filter(|value| !value.is_empty());
        let token = parsed
            .data
            .as_ref()
            .and_then(|d| d.token.as_deref())
            .or(parsed.token.as_deref())
            .filter(|value| !value.is_empty());

        if let Some(userid) = userid {
            merged.insert("userid", userid);
        }
        if let Some(token) = token {
            merged.insert("token", token);
        }

        Ok(QrPollResult {
            cookies: merged,
            status_code: status,
            message: parsed.message,
        })
    }

    pub async fn refresh_login(
        &self,
        cookies: &CookieStore,
    ) -> Result<RefreshResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/login/token", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_login_failed(format!("token refresh: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_login_failed(format!("token body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let _parsed: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_login_failed(format!("parse token: {}", e)))?;

        Ok(RefreshResult { cookies: merged })
    }

    pub async fn ensure_dfid(
        &self,
        cookies: &CookieStore,
    ) -> Result<CookieStore, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/register/dev", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("register dev: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_request_failed(format!("register dev body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        Ok(merged)
    }

    pub async fn fetch_vip_status(
        &self,
        cookies: &CookieStore,
    ) -> Result<VipStatusResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/user/vip/detail", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_vip_check_failed(format!("vip detail: {}", e)))?;
        let _status = resp.status().as_u16();
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_vip_check_failed(format!("vip body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: VipDetailResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_vip_check_failed(format!("parse vip: {}", e)))?;

        let data = parsed.data.unwrap_or(VipDetailData {
            vip_type: None,
            is_vip: None,
            is_music_vip: None,
        });

        let vip_type = data.vip_type.unwrap_or(0) as i32;
        let vip_active = data.is_vip.unwrap_or(0) == 1 || data.is_music_vip.unwrap_or(0) == 1;

        Ok(VipStatusResult {
            vip_type,
            vip_active,
            cookies: merged,
        })
    }

    pub async fn fetch_music_url(
        &self,
        cookies: &CookieStore,
        req: &MusicUrlRequest,
    ) -> Result<MusicUrlResult, AppError> {
        let mut merged = cookies.clone();
        let quality_param = map_quality(&req.quality);
        let mut url = format!(
            "{}/song/url?hash={}&quality={}",
            self.base_url, req.hash, quality_param
        );
        if let Some(ref aaid) = req.album_audio_id {
            url = format!("{}&album_audio_id={}", url, aaid);
        }

        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("song url: {}", e)))?;
        let status = resp.status().as_u16();
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp.text().await.map_err(|e| AppError::upstream_request_failed(format!("song url body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: SongUrlResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_request_failed(format!("parse song url: {}", e)))?;
        let auth_failed = is_auth_failure(status, &body);

        let play_url = parsed
            .data
            .and_then(|d| d.url)
            .filter(|u| !u.is_empty());

        match play_url {
            Some(u) => Ok(MusicUrlResult {
                url: u,
                cookies: merged,
                status_code: status,
                auth_failed,
            }),
            None => Ok(MusicUrlResult {
                url: String::new(),
                cookies: merged,
                status_code: status,
                auth_failed,
            }),
        }
    }
}

fn map_quality(quality: &str) -> &str {
    match quality {
        "128k" => "128",
        "320k" => "320",
        "flac" => "flac",
        "flac24bit" => "high",
        _ => "128",
    }
}

pub fn is_auth_failure(status_code: u16, body: &str) -> bool {
    if status_code == 401 || status_code == 403 {
        return true;
    }
    body.contains("\"code\":301") || body.contains("\"code\":302") || body.contains("token")
}
