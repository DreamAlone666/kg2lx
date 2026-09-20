use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::config::Config;
use crate::domain::cookie_store::{
    CookieStore, merge_cookies_from_body_if_present, merge_cookies_from_headers,
};
use crate::error::AppError;

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum StringOrInt {
    String(String),
    Int(i64),
    Uint(u64),
}

impl StringOrInt {
    fn to_string_value(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Int(value) => value.to_string(),
            Self::Uint(value) => value.to_string(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct QrKeyResponse {
    #[serde(default)]
    data: Option<QrKeyData>,
}

#[derive(Debug, serde::Deserialize)]
struct QrKeyData {
    #[serde(default)]
    #[serde(alias = "qrcode")]
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
    #[serde(alias = "url")]
    qrurl: Option<String>,
    #[serde(default)]
    #[serde(alias = "base64")]
    qrimg: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCheckResponse {
    #[serde(default)]
    data: Option<QrCheckData>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    token: Option<StringOrInt>,
    #[serde(default)]
    userid: Option<StringOrInt>,
}

#[derive(Debug, serde::Deserialize)]
struct QrCheckData {
    #[serde(default)]
    status: Option<i64>,
    #[serde(default)]
    token: Option<StringOrInt>,
    #[serde(default)]
    userid: Option<StringOrInt>,
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
    #[serde(default)]
    busi_vip: Vec<VipBusinessEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct VipBusinessEntry {
    #[serde(default)]
    busi_type: Option<String>,
    #[serde(default)]
    is_vip: Option<i64>,
}

/// `song/url` 响应里"上游到底说了什么"的观测结果。
///
/// 这些字段过去被直接丢掉，导致事后完全无法回答"上游是拒绝了这一次取链，还是
/// 根本就没这首歌"。它们**只记录事实**：`fail_process` 尤其不能当判据用（同一
/// 账号同一小时内实测出现过 `["pkg","buy"]`、`["buy"]`、`[]` 三种取值）。
///
/// 全部字段都是 `Option`：`status:1` / `status:2` / `status:0` 三种形态各自只带
/// 一部分字段，缺失是正常的，解析不得因为缺字段而失败。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpstreamObservation {
    /// 上游业务状态：`1` 成功 / `2` 明确拒绝本次音质 / `0` 资源不存在或参数错。
    pub status: Option<i64>,
    /// 权益状态位。实测 `status:2` 时是 `0`、`status:1` 时是 `1`，但语义未经证实，
    /// 与 `fail_process` 同样只作记录。
    pub priv_status: Option<i64>,
    /// 上游自述的失败环节。取值不稳定，**不可作为任何判断的输入**，只落盘备查。
    pub fail_process: Option<Vec<String>>,
    /// 实际返回的音频格式：`mp3` / `flac` / `mkv`。
    ///
    /// 这是"实际拿到的是不是所要档位"的唯一自证材料：`quality=high` 对没有
    /// Hi-Res 声部的歌会静默回落到 128k mp3，此时 `status` 仍然是 1。
    pub ext_name: Option<String>,
    /// 实际返回的字节数。与 `ext_name` 配合可以校验静默降级（回落的 128k 与
    /// 直接请求 128k 的 `fileSize` 完全相同）。
    pub file_size: Option<i64>,
}

/// `fail_process` 落盘前保留的最大条目数。
///
/// 这个值会进入每一条运行时日志的 jsonl 行，而 `fail_process` 是上游给的长度不
/// 可控的数组：不设上限的话，上游某天返回几百项（或有人构造一个）就会让**每一行**
/// 日志都膨胀成几十 KB，把日志文件与列表接口一起拖垮。20 项足够覆盖实测出现过的
/// 全部取值（最长 2 项），超出的部分对我们没有任何信息量。
const FAIL_PROCESS_MAX_ENTRIES: usize = 20;

#[derive(Debug, serde::Deserialize)]
struct SongUrlResponse {
    #[serde(default)]
    data: Option<SongUrlData>,
    #[serde(default)]
    url: Option<StringOrStrings>,
    // 下面这几个观测字段在三次实测抓包里都在**顶层**（与 `url` 同为顶层键），
    // 但 `url` 的既有处理已经证明上游会把同一个值放在两层里的任意一层，所以这里
    // 照同样的宽容度两层都读（`SongUrlResponse::observation` 里合并，顶层优先）。
    #[serde(default)]
    status: Option<i64>,
    #[serde(default, alias = "privStatus")]
    priv_status: Option<i64>,
    #[serde(default, alias = "failProcess")]
    fail_process: Option<Vec<String>>,
    #[serde(default, alias = "extName")]
    ext_name: Option<String>,
    #[serde(default, alias = "fileSize")]
    file_size: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum StringOrStrings {
    String(String),
    Strings(Vec<String>),
}

impl StringOrStrings {
    fn first_non_empty(self) -> Option<String> {
        match self {
            Self::String(value) => (!value.is_empty()).then_some(value),
            Self::Strings(values) => values.into_iter().find(|value| !value.is_empty()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct SongUrlData {
    #[serde(default)]
    url: Option<StringOrStrings>,
    #[serde(default)]
    status: Option<i64>,
    #[serde(default, alias = "privStatus")]
    priv_status: Option<i64>,
    #[serde(default, alias = "failProcess")]
    fail_process: Option<Vec<String>>,
    #[serde(default, alias = "extName")]
    ext_name: Option<String>,
    #[serde(default, alias = "fileSize")]
    file_size: Option<i64>,
}

impl SongUrlResponse {
    /// 把这次响应里"发生过什么"摘出来，供落盘与拒绝判定使用。
    ///
    /// 每个字段都是"顶层先、`data` 后"：实测抓包都发生在顶层，`data` 只是同一
    /// 宽容度的兜底，两层都没有就是 `None`（不是默认值 0——"上游没说"和"上游说
    /// 了 0"含义不同，前者不能当成后者去判断）。
    fn observation(&self) -> UpstreamObservation {
        let data = self.data.as_ref();
        UpstreamObservation {
            status: self.status.or_else(|| data.and_then(|d| d.status)),
            priv_status: self
                .priv_status
                .or_else(|| data.and_then(|d| d.priv_status)),
            fail_process: self
                .fail_process
                .clone()
                .or_else(|| data.and_then(|d| d.fail_process.clone()))
                .map(truncate_fail_process),
            ext_name: self
                .ext_name
                .clone()
                .or_else(|| data.and_then(|d| d.ext_name.clone())),
            file_size: self.file_size.or_else(|| data.and_then(|d| d.file_size)),
        }
    }
}

fn truncate_fail_process(mut values: Vec<String>) -> Vec<String> {
    values.truncate(FAIL_PROCESS_MAX_ENTRIES);
    values
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
    /// 本次上游响应里可观测到的业务字段。**即使 `url` 为空也一定有值**——
    /// "没有 url"的两种原因（上游明确拒绝 / 上游没这首歌）正是靠它区分的。
    pub observation: UpstreamObservation,
}

/// 取播放地址的最小接口。
///
/// 抽它的原因只有一个：`RuntimeService` 要沿音质阶梯连续调用这个动作（拒绝一档
/// 就降一档再试），而这条循环的正确性必须在**无网络、无账号**的情况下可测。
/// 若仍持有具体的 `KugouLiteClient`，测试就只能真的发请求。
///
/// 刻意只抽这一个方法：登录 / 刷新 / dfid / VIP 查询仍然是具体类型的职责，把它们
/// 塞进来只会让测试替身凭空多出一堆与降级无关的实现。这与仓库既有的
/// `dyn SourceRepo` / `dyn AccountRepo` 注入风格一致。
#[async_trait]
pub trait MusicUrlFetcher: Send + Sync {
    async fn fetch_music_url(
        &self,
        cookies: &CookieStore,
        req: &MusicUrlRequest,
    ) -> Result<MusicUrlResult, AppError>;
}

#[async_trait]
impl MusicUrlFetcher for KugouLiteClient {
    async fn fetch_music_url(
        &self,
        cookies: &CookieStore,
        req: &MusicUrlRequest,
    ) -> Result<MusicUrlResult, AppError> {
        // 写全路径而不是 `self.fetch_music_url(..)`：后者现在是等价的（同名的固有
        // 方法优先级高于 trait 方法），但一旦将来固有方法被改名或删除，那一行会
        // **静默**解析成这个 trait 方法自身，也就是无限递归；全路径写法会当场编译
        // 失败，把问题挡在构建期。
        KugouLiteClient::fetch_music_url(self, cookies, req).await
    }
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
            // `Config::from_env` 已保证该值是不带尾随 `/` 的规范化 http(s) 地址，
            // 这里不再重复归一化。
            base_url: config.kugou_api_base_url.clone(),
            client,
        }
    }

    pub async fn request_qr_key(&self, cookies: &CookieStore) -> Result<QrKeyResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!(
            "{}/login/qr/key?timestamp={}",
            self.base_url,
            timestamp_ms()
        );
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr key: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr key body: {}", e)))?;
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
        let url = format!(
            "{}/login/qr/create?key={}&qrimg=1&timestamp={}",
            self.base_url,
            qr_key,
            timestamp_ms()
        );
        let resp2 = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr create: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp2.headers());
        let body2 = resp2
            .text()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr create body: {}", e)))?;
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
        let url = format!(
            "{}/login/qr/check?key={}&timestamp={}",
            self.base_url,
            qr_key,
            timestamp_ms()
        );
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr check: {}", e)))?;
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("qr check body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: QrCheckResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_request_failed(format!("parse qr check: {}", e)))?;

        let status = parsed.data.as_ref().and_then(|d| d.status).unwrap_or(-1);
        let userid = parsed
            .data
            .as_ref()
            .and_then(|d| d.userid.as_ref().map(|value| value.to_string_value()))
            .or(parsed.userid.as_ref().map(|value| value.to_string_value()))
            .filter(|value| !value.is_empty());
        let token = parsed
            .data
            .as_ref()
            .and_then(|d| d.token.as_ref().map(|value| value.to_string_value()))
            .or(parsed.token.as_ref().map(|value| value.to_string_value()))
            .filter(|value| !value.is_empty());

        if let Some(userid) = userid.as_deref() {
            merged.insert("userid", userid);
        }
        if let Some(token) = token.as_deref() {
            merged.insert("token", token);
        }

        Ok(QrPollResult {
            cookies: merged,
            status_code: status,
            message: parsed.message,
        })
    }

    pub async fn refresh_login(&self, cookies: &CookieStore) -> Result<RefreshResult, AppError> {
        let mut merged = cookies.clone();
        let url = format!("{}/login/token", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Cookie", cookies.to_cookie_header())
            .send()
            .await
            .map_err(|e| AppError::upstream_login_failed(format!("token refresh: {}", e)))?;
        let status = resp.status().as_u16();
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_login_failed(format!("token body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        if !(200..300).contains(&status) {
            return Err(AppError::upstream_login_failed(format!(
                "token refresh status {}: {}",
                status,
                summarize_upstream_body(&body)
            )));
        }

        let _parsed: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_login_failed(format!("parse token: {}", e)))?;

        Ok(RefreshResult { cookies: merged })
    }

    pub async fn ensure_dfid(&self, cookies: &CookieStore) -> Result<CookieStore, AppError> {
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
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("register dev body: {}", e)))?;
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
        let status = resp.status().as_u16();
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_vip_check_failed(format!("vip body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        if !(200..300).contains(&status) {
            return Err(AppError::upstream_vip_check_failed(format!(
                "vip detail status {}: {}",
                status,
                summarize_upstream_body(&body)
            )));
        }

        let parsed: VipDetailResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_vip_check_failed(format!("parse vip: {}", e)))?;

        let data = parsed.data.unwrap_or(VipDetailData {
            vip_type: None,
            is_vip: None,
            is_music_vip: None,
            busi_vip: Vec::new(),
        });

        let top_level_vip = data.is_vip.unwrap_or(0) == 1 || data.is_music_vip.unwrap_or(0) == 1;
        let concept_vip = data.busi_vip.iter().any(|entry| {
            entry.is_vip.unwrap_or(0) == 1
                && entry
                    .busi_type
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case("concept"))
                    .unwrap_or(false)
        });
        let vip_type = data.vip_type.unwrap_or(0) as i32;
        let vip_active = top_level_vip || concept_vip;

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
        // 429 在这里就短路，不读 body。限流响应体常常根本不是 JSON（网关/防护页
        // 直接回 HTML 或空体），若让它继续走到下面 serde_json 那条通用失败路径，
        // 就会被折回 UPSTREAM_REQUEST_FAILED —— 那正是脚本唯一能分支的瞬时信号
        // 被抹掉的地方。刻意不消费 body：这条路径上不需要它。
        if status == 429 {
            return Err(AppError::upstream_rate_limited());
        }
        merge_cookies_from_headers(&mut merged, resp.headers());
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::upstream_request_failed(format!("song url body: {}", e)))?;
        merge_cookies_from_body_if_present(&mut merged, &body);

        let parsed: SongUrlResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::upstream_request_failed(format!("parse song url: {}", e)))?;
        let auth_failed = is_auth_failure(status, &body);

        // 先摘观测、再取 url：`observation()` 只借用 `parsed`，而下面的 url 提取
        // 会把它的一部分 move 走，顺序反了就编译不过。
        let observation = parsed.observation();
        let play_url = parsed
            .data
            .and_then(|d| d.url)
            .or(parsed.url)
            .and_then(|u| u.first_non_empty());

        match play_url {
            Some(u) => Ok(MusicUrlResult {
                url: u,
                cookies: merged,
                status_code: status,
                auth_failed,
                observation,
            }),
            None => Ok(MusicUrlResult {
                url: String::new(),
                cookies: merged,
                status_code: status,
                auth_failed,
                observation,
            }),
        }
    }
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// LX 音质名 → 酷狗 `song/url` 的 quality 参数。
///
/// `pub(crate)` 是因为运行时日志要把它**实际用的值**落盘（见 services/runtime.rs 的
/// append_log）：日志行应当记录发生过的事实，而不是让前端再去复制一份这张表——
/// 那样服务端改了这里，界面会安静地继续说旧话。
pub(crate) fn map_quality(quality: &str) -> &str {
    match quality {
        "128k" => "128",
        "320k" => "320",
        "flac" => "flac",
        "flac24bit" => "high",
        _ => "128",
    }
}

/// LX 音质由高到低的完整阶梯。
///
/// 与 `map_quality` 是同一套线上取值的两个视图：这里是"顺序"，那里是"映射到上游
/// 的 quality 参数"。刻意紧邻放置，好让"新增了档位却只改了其中一边"在评审的 diff
/// 里直接可见（单测里也钉住了两者的取值集合一致）。
pub(crate) const QUALITY_LADDER: [&str; 4] = ["flac24bit", "flac", "320k", "128k"];

/// 阶梯上低一档的 LX 音质；已在最低档（`128k`）或传入未知取值时返回 `None`。
///
/// **陷阱（不要改写这段）**：不要复用 `map_quality` 那个 `_ => "128"` 兜底。
/// 降级循环把 `None` 读作"不能再降了"，而兜底会把一个未知/未来新增的档位（例如
/// `hires`）静默变成"去请求 128k"——用户要的档位、日志里记的 `attempted_quality`
/// 与真实发出的请求三者全部对不上，且没有任何一层会报错。未知取值必须原样暴露成
/// `None`，让调用方去决定怎么处理。
pub(crate) fn next_lower_quality(quality: &str) -> Option<&'static str> {
    let index = QUALITY_LADDER.iter().position(|rung| *rung == quality)?;
    QUALITY_LADDER.get(index + 1).copied()
}

/// 上游是否**明确拒绝**了这一次要的音质。
///
/// 只认 `status == 2`。实测该形态带 `priv_status: 0` 与 `fail_process`、且没有
/// url，含义是"这首歌存在，但这一档你这次拿不到"——这是唯一值得换更低档重试的情形。
///
/// `status == 0`（资源不存在 / 参数非法，带 `error_code` 如 35104、20010）、429
/// 限流、超时、传输错误一律**不算**：换档重试对它们无效，只会把一次失败放大成
/// 四次请求（限流场景下更糟）。`None`（响应体里根本没有 status）同样不算。
///
/// 另见 `is_auth_failure`：它是若干子串启发式（`token` / `dfid` / `需要验证` /
/// `"code":301`），一个 `status:2` 的响应体完全可能命中它。因此调用方必须先问
/// 这里、再问 `auth_failed`，否则会去刷新登录而不是降级。
pub(crate) fn upstream_refused_quality(business_status: Option<i64>) -> bool {
    business_status == Some(2)
}

pub fn is_auth_failure(status_code: u16, body: &str) -> bool {
    if status_code == 401 || status_code == 403 {
        return true;
    }

    let body_lower = body.to_ascii_lowercase();
    body.contains("\"code\":301")
        || body.contains("\"code\":302")
        || body_lower.contains("token")
        || body_lower.contains("dfid")
        || body.contains("本次请求需要验证")
        || body.contains("需要验证")
}

fn summarize_upstream_body(body: &str) -> String {
    const MAX_LEN: usize = 200;

    let trimmed = body.trim();
    if trimmed.len() <= MAX_LEN {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..MAX_LEN])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;

    fn parse(body: &str) -> SongUrlResponse {
        serde_json::from_str(body).expect("抓包 fixture 必须能解析")
    }

    // ---- 音质阶梯 ----

    #[test]
    fn ladder_walks_down_one_rung_at_a_time() {
        assert_eq!(next_lower_quality("flac24bit"), Some("flac"));
        assert_eq!(next_lower_quality("flac"), Some("320k"));
        assert_eq!(next_lower_quality("320k"), Some("128k"));
    }

    #[test]
    fn ladder_bottom_has_no_lower_rung() {
        assert_eq!(next_lower_quality("128k"), None);
    }

    /// 这条专门防"静默兜底"：一旦 `next_lower_quality` 被改成复用 `map_quality`
    /// 的 `_ => "128"`，未知档位就会悄悄变成"去请求 128k"，而调用方以为还能降级。
    /// 断言的是 `None`，**不是** `Some("128k")` 之类的具体值。
    #[test]
    fn unknown_quality_has_no_lower_rung() {
        assert_eq!(next_lower_quality("hires"), None);
        assert_eq!(next_lower_quality(""), None);
        // 上游参数名（`map_quality` 的输出）不是 LX 线上取值，同样必须返回 None：
        // 否则一个"把映射后的值误当线上取值传回来"的调用点会静默生效。
        assert_eq!(next_lower_quality("128"), None);
        assert_eq!(next_lower_quality("high"), None);
    }

    /// `QUALITY_LADDER` 与 `map_quality` 是同一套取值的两个视图。这里把两边都钉住：
    /// - 阶梯的顺序必须与 `next_lower_quality` 的实际行为一致（逐对跳出下一档）；
    /// - 阶梯里每个取值都必须在 `map_quality` 里有自己的映射。
    ///
    /// 于是"新增一档只改了一边"会在这里硬失败，而不是等到线上请求打到错误档位。
    #[test]
    fn ladder_and_map_quality_share_one_vocabulary() {
        assert_eq!(QUALITY_LADDER, ["flac24bit", "flac", "320k", "128k"]);

        for pair in QUALITY_LADDER.windows(2) {
            assert_eq!(next_lower_quality(pair[0]), Some(pair[1]));
        }

        assert_eq!(map_quality("flac24bit"), "high");
        assert_eq!(map_quality("flac"), "flac");
        assert_eq!(map_quality("320k"), "320");
        assert_eq!(map_quality("128k"), "128");
    }

    // ---- 拒绝判定 ----

    #[test]
    fn refusal_is_only_status_two() {
        assert!(upstream_refused_quality(Some(2)));
        assert!(!upstream_refused_quality(Some(1)));
        assert!(!upstream_refused_quality(Some(0)));
        assert!(!upstream_refused_quality(None));
        // `None`（响应体里没有 status）与各种非 2 取值都不能被当成拒绝：
        // 否则限流/参数错误会被放大成四次上游请求。
        assert!(!upstream_refused_quality(Some(-1)));
        assert!(!upstream_refused_quality(Some(20028)));
    }

    // ---- 上游观测解析（三次实测抓包的原样 body） ----

    /// 形态一：明确拒绝这一档。带 `priv_status: 0`、`fail_process`，且**没有 url**。
    #[test]
    fn parses_explicit_refusal() {
        let parsed = parse(
            r#"{"status":2,"data":{},"priv_status":0,"fail_process":["pkg","buy"],"error_code":0}"#,
        );

        assert_eq!(
            parsed.observation(),
            UpstreamObservation {
                status: Some(2),
                priv_status: Some(0),
                fail_process: Some(vec!["pkg".to_string(), "buy".to_string()]),
                ext_name: None,
                file_size: None,
            }
        );
        assert!(upstream_refused_quality(parsed.observation().status));

        // 没有 url 是这一形态的一部分，断言掉以免解析层悄悄"补"出一个空串。
        assert!(parsed.url.is_none());
        assert!(parsed.data.as_ref().and_then(|d| d.url.as_ref()).is_none());
    }

    /// 形态二：资源不存在 / 参数非法。形状与拒绝完全不同（带 `error_code`、没有
    /// `priv_status` 与 `fail_process`），因此观测里除 `status` 外全应为 `None`——
    /// 尤其不能被填成默认值 0。
    #[test]
    fn parses_not_found() {
        let parsed = parse(r#"{"status":0,"data":{},"error_code":35104,"error":"Hash not found"}"#);
        let observation = parsed.observation();

        assert_eq!(
            observation,
            UpstreamObservation {
                status: Some(0),
                ..Default::default()
            }
        );
        // `status:0` 不是拒绝：换档重试对"这首歌不存在/参数错"无效。
        assert!(!upstream_refused_quality(observation.status));
    }

    /// 形态三：成功。`extName` / `fileSize` 是事后判断"实际拿到的是不是所要档位"
    /// 的唯一材料（`quality=high` 会静默回落到 128k mp3 且 `status` 仍是 1）。
    #[test]
    fn parses_success() {
        let parsed = parse(
            r#"{"status":1,"data":{},"priv_status":1,"extName":"flac","fileSize":29183562,"timeLength":180,"fileName":"song.flac","url":"http://example.invalid/a.flac"}"#,
        );
        let observation = parsed.observation();

        assert_eq!(
            observation,
            UpstreamObservation {
                status: Some(1),
                priv_status: Some(1),
                fail_process: None,
                ext_name: Some("flac".to_string()),
                file_size: Some(29183562),
            }
        );
        assert!(!upstream_refused_quality(observation.status));
    }

    /// 上游把值放在顶层还是 `data` 下并不稳定——`url` 的既有处理
    /// （`.data(..).or(top_level)`）就是为此而写。观测字段必须同样两层都认，
    /// 否则换一种包裹方式落盘的就是一片空白。
    #[test]
    fn parses_observation_nested_under_data() {
        let parsed = parse(
            r#"{"data":{"status":2,"priv_status":0,"fail_process":["buy"],"extName":"mp3","fileSize":1234}}"#,
        );

        assert_eq!(
            parsed.observation(),
            UpstreamObservation {
                status: Some(2),
                priv_status: Some(0),
                fail_process: Some(vec!["buy".to_string()]),
                ext_name: Some("mp3".to_string()),
                file_size: Some(1234),
            }
        );
    }

    /// 形状完全不符合预期时也不能让解析失败：观测是"记录"，不该把一次本来能播的
    /// 取链搞挂。空对象与缺字段都读成"上游没说"。
    #[test]
    fn missing_fields_read_as_unknown_not_zero() {
        assert_eq!(parse("{}").observation(), UpstreamObservation::default());
        assert_eq!(
            parse(r#"{"data":{}}"#).observation(),
            UpstreamObservation::default()
        );
    }

    /// `fail_process` 会进每一行运行时日志的 jsonl，长度必须被截断。这里断言的是
    /// "存下来的条目数等于上限"，而不是某个具体条目的内容。
    #[test]
    fn fail_process_is_truncated_before_storage() {
        let long: Vec<serde_json::Value> = (0..FAIL_PROCESS_MAX_ENTRIES + 5)
            .map(|index| serde_json::json!(format!("step{}", index)))
            .collect();
        let body = serde_json::json!({
            "status": 2,
            "data": {},
            "priv_status": 0,
            "fail_process": long,
        })
        .to_string();

        let stored = parse(&body)
            .observation()
            .fail_process
            .expect("fail_process 应当被记录");
        assert_eq!(stored.len(), FAIL_PROCESS_MAX_ENTRIES);
        assert_eq!(stored[0], "step0");
    }

    /// `[]`（上游明确说了"失败环节是空的"）与"这个键根本没出现"是两件不同的事，
    /// 并且这个区别会一路传到日志 wire 上（日志面板按 `!== null` 把空数组渲染成
    /// `[]`，把缺失渲染成"本行没有上游观测"）。
    ///
    /// 因此这里必须保持 `Some(vec![])`：不要为了"顺手清理"而加
    /// `filter(|values| !values.is_empty())` 之类的归一化，那会把一个发生过的事实
    /// 抹成"上游没说"。注意这与截断是两回事——截断只处理"太长"。
    #[test]
    fn empty_fail_process_stays_present_not_absent() {
        assert_eq!(
            parse(r#"{"status":2,"data":{},"priv_status":0,"fail_process":[]}"#)
                .observation()
                .fail_process,
            Some(Vec::new())
        );
        assert_eq!(
            parse(r#"{"status":2,"data":{}}"#)
                .observation()
                .fail_process,
            None
        );
    }

    // ---- 测试接缝 ----

    struct StubFetcher;

    #[async_trait]
    impl MusicUrlFetcher for StubFetcher {
        async fn fetch_music_url(
            &self,
            _cookies: &CookieStore,
            req: &MusicUrlRequest,
        ) -> Result<MusicUrlResult, AppError> {
            Ok(MusicUrlResult {
                url: format!("http://example.invalid/{}", req.quality),
                cookies: CookieStore::new(),
                status_code: 200,
                auth_failed: false,
                observation: UpstreamObservation {
                    status: Some(2),
                    ..Default::default()
                },
            })
        }
    }

    /// 降级循环会通过 `Arc<dyn MusicUrlFetcher>` 调用它，所以这条断言必须成立：
    /// trait 对象安全（能取得 `&dyn`），且实现方只需要这一个方法。
    #[tokio::test]
    async fn fetcher_dispatches_through_dyn() {
        let fetcher: &dyn MusicUrlFetcher = &StubFetcher;
        let result = fetcher
            .fetch_music_url(
                &CookieStore::new(),
                &MusicUrlRequest {
                    hash: "abc".into(),
                    album_audio_id: None,
                    quality: "flac".into(),
                },
            )
            .await
            .expect("stub 不会失败");

        assert_eq!(result.url, "http://example.invalid/flac");
        assert!(upstream_refused_quality(result.observation.status));
    }
}
