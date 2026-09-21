use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::Value;

use crate::config::Config;
use crate::domain::account::VipEntitlement;
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

/// 这里**只解析 `busi_vip`**。上游同一条响应里还有顶层 `is_vip` / `is_music_vip` /
/// `vip_type` 三个用于聚合的原料字段，本仓曾经拿它们推导出一个账号级的「有没有会员」
/// 布尔值；权益是并列的增量信息（`busi_vip` 才是事实），聚合值既冗余又低效，已经
/// 连同推导一起删除，因此这里不再声明它们——那些键留在响应里也不会让解析失败。
#[derive(Debug, serde::Deserialize)]
struct VipDetailData {
    #[serde(default)]
    busi_vip: Vec<VipBusinessEntry>,
}

/// `data.busi_vip[]` 的一条：上游把「有哪些会员权益」拆成并列的多条，每条各有
/// 自己的 `product_type` 与有效期，**没有任何中文名或描述字段**（整条 dump 过，
/// 13 个 key 里没有 name/title/description），中文名是客户端文案。
///
/// `busi_type`（上游的权益分组标识，实测取值 `concept`）**不再解析**：它唯一的用处
/// 是已删除的聚合推导（判断「概念版权益在不在期」），此后没有任何消费者。上游多发
/// 一个键不影响解析。
///
/// 三个新字段的类型是 `Option<Value>` 而不是 `Option<String>`，理由与
/// `SongUrlResponse` 上那几个观测字段**完全同类**：它们只记录上游说过什么，不参与
/// 任何判定，因此不该有「让整条 VIP 查询失败」的权力。一次原本
/// 成功的 `fetch_vip_status` 因为某个时间字段被写成数字而整条失败，向上表现成
/// `UPSTREAM_VIP_CHECK_FAILED` → 刷新失败 → 账号被标成 `login_failed`，而用户
/// 其实一切正常。取值的窄化在 `to_entitlement`：认不出的形状读成「上游没说」，
/// **不猜值**。
#[derive(Debug, serde::Deserialize)]
struct VipBusinessEntry {
    #[serde(default)]
    is_vip: Option<i64>,
    /// 权益标识（实测 `svip` / `tvip` / `qvip` / `dvip`）。取值域**刻意不枚举**：
    /// 上游新增一档时原样透传给后台，比在解析层「认不出就丢」要好——丢掉的正是
    /// 用户要看的那条信息。
    #[serde(default)]
    product_type: Option<Value>,
    #[serde(default)]
    vip_begin_time: Option<Value>,
    #[serde(default)]
    vip_end_time: Option<Value>,
}

impl VipBusinessEntry {
    /// 把一条 `busi_vip` 条目落成对外的权益记录。
    ///
    /// `product_type` 缺失（或形状认不出）时落成空串而不是丢弃整条：每一条都是
    /// 上游明确列出的一个权益位，丢掉它等于把「上游给了什么」抹成「上游什么都没
    /// 给」。`is_vip` 缺失读成 false——它是「上游没表态」，不是「在期」。
    fn to_entitlement(&self) -> VipEntitlement {
        VipEntitlement {
            product_type: observed_string(self.product_type.as_ref()).unwrap_or_default(),
            active: self.is_vip.unwrap_or(0) == 1,
            begin_at: observed_time(self.vip_begin_time.as_ref()),
            end_at: observed_time(self.vip_end_time.as_ref()),
        }
    }
}

/// 权益时间字段的取值：先按观测字段的宽容度窄化成字符串（形状认不出就是「上游
/// 没说」），再按本地时区解释成时间戳。两步都不允许失败，理由见
/// `VipBusinessEntry`。
fn observed_time(value: Option<&Value>) -> Option<i64> {
    local_time_to_epoch(observed_string(value).as_deref())
}

/// 上游的权益时间串（`YYYY-MM-DD HH:MM:SS`）→ 秒级 Unix 时间戳。
///
/// **假设（必须写明）：上游这个串不带时区。** 酷狗只给本地时间文本，没有偏移量、
/// 也没有 `Z` 后缀，因此唯一可用的解释是「它与本服务端所处时区一致」——即假定
/// 服务端与用户在同一时区。**残留风险**：浏览器与服务器时区不同（出境、多机房、
/// 或用户手动改过系统时区）时，后台按浏览器本地时区渲染这个时间戳会整体偏移一个
/// 时差。这是解释层面的缺陷，不是解析缺陷，在上游补出时区之前无法消除；受影响的
/// 只有展示的钟点，`active`（`is_vip`）这个判断不经过时间。
///
/// 任何读不出来的取值（空串、格式不同、不存在的本地时刻）一律 `None`，且**绝不
/// 让整条响应解析失败**：权益时间只是附带信息，为此把一次成功的 VIP 查询变成
/// `UPSTREAM_VIP_CHECK_FAILED`，等于把「拿到了权益、只是时间读不出来」说成
/// 「上游请求失败」，指向完全错误的方向。
fn local_time_to_epoch(value: Option<&str>) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(value?, "%Y-%m-%d %H:%M:%S").ok()?;
    // 夏令时切换那一小时里的本地时刻不唯一，`single()` 此时返回 `None`。中国区没有
    // 夏令时，取「不唯一即 None」比随手挑一个更诚实：宁可少一个时间戳，也不给出
    // 一个可能错了一小时的值。
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp())
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
    /// 上游自报的码率（bps）。
    ///
    /// **只有 mp3 两档能用**：同一首歌四档实测里 128 / 320 分别是精确的 `128000` /
    /// `320000`，而 `flac` / `high` 两个无损档**恒为 0**——0 在这里是「上游没报这个
    /// 值」，不是「0 bps」。因此它不能单独支撑展示，展示侧要按 `file_size × 8 ÷
    /// time_length` 回退（见 domain/runtime_log.rs 上 `upstream_bit_rate` 的说明）。
    /// 落盘的理由正是另一面：mp3 两档上它是**精确值**，比回退算出来的近似值更可信。
    pub bit_rate: Option<i64>,
    /// 音频时长（秒）。
    ///
    /// **存在的唯一理由是给上面那个回退算法当除数**：`file_size × 8 ÷ time_length`
    /// 实测四档全有值且分离度足够（约 128 / 321 / 1038 / 1747 kbps），是无损档唯一
    /// 可用的码率来源。它单独看没有诊断价值，不要拿它做别的判断。
    pub time_length: Option<i64>,
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
    // 照同样的宽容度两层都读（`observation` 里合并，顶层优先）。
    //
    // 类型是 `Option<Value>` 而不是 `Option<i64>` / `Option<Vec<String>>`：这几个字段
    // **没有一个参与取链**，全是记录，因此不该有「让这次响应解析失败」的权力。定型的
    // `Option<i64>` 会让一份**带着可用 `url` 的成功响应**整条失败——只要上游把某个
    // 数字写成字符串（它已经在同一个响应体里对 `url` 干过一次：有时字符串、有时
    // 数组），用户拿到的还是「上游请求失败」，指向完全错误的方向。
    //
    // 取值时的窄化在 `observation`：认不出的形状读成 `None`（「上游没说」），**不猜值**。
    #[serde(default)]
    status: Option<Value>,
    #[serde(default, alias = "privStatus")]
    priv_status: Option<Value>,
    #[serde(default, alias = "failProcess")]
    fail_process: Option<Value>,
    #[serde(default, alias = "extName")]
    ext_name: Option<Value>,
    #[serde(default, alias = "fileSize")]
    file_size: Option<Value>,
    // `bitRate` / `timeLength` 与上面几个同类：只作记录、类型必须宽松（理由见
    // 本结构体顶上那段）。注意上游对两个无损档把 `bitRate` 写成 0，
    // 那是「没说」而不是「0 bps」——窄化后 `Some(0)` 与 `None` 仍要分开落盘，
    // 由展示侧决定 0 要不要走回退算法。
    #[serde(default, alias = "bitRate")]
    bit_rate: Option<Value>,
    #[serde(default, alias = "timeLength")]
    time_length: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum StringOrStrings {
    String(String),
    Strings(Vec<String>),
}

impl StringOrStrings {
    /// 借用而不是取走：`SongUrlResponse::play_url` 手里只有 `&self`，为拿一个地址把
    /// 整棵树 move 掉没有道理。
    fn first_non_empty(&self) -> Option<String> {
        match self {
            Self::String(value) => (!value.is_empty()).then(|| value.clone()),
            Self::Strings(values) => values.iter().find(|value| !value.is_empty()).cloned(),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct SongUrlData {
    #[serde(default)]
    url: Option<StringOrStrings>,
    /// 与 `SongUrlResponse` 上同名的七个字段同理：只作记录，类型必须宽松。
    #[serde(default)]
    status: Option<Value>,
    #[serde(default, alias = "privStatus")]
    priv_status: Option<Value>,
    #[serde(default, alias = "failProcess")]
    fail_process: Option<Value>,
    #[serde(default, alias = "extName")]
    ext_name: Option<Value>,
    #[serde(default, alias = "fileSize")]
    file_size: Option<Value>,
    #[serde(default, alias = "bitRate")]
    bit_rate: Option<Value>,
    #[serde(default, alias = "timeLength")]
    time_length: Option<Value>,
}

/// 观测字段的窄化：认不出的形状读成 `None`（「上游没说」），**不猜值**。
///
/// 数字字符串（`"9412859"`）刻意**不**当数字用。理由不是洁癖，而是 `status` 参与
/// 「该不该降级」这个判断：在判断输入上猜错，等于在一个错误的档位上执行策略，比不动作
/// 更糟；而收益只是某一行的记录更完整一点。
fn observed_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64)
}

/// 字符串取值：只有真的是字符串才算。
fn observed_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| value.as_str()).map(str::to_string)
}

/// `fail_process` 取值：只要是数组就收下，元素不是字符串就记它的 JSON 文本。
///
/// 这里比别处宽一档，理由不是「形状不可控」（三次实测都是字符串数组），而是：整条读成
/// `None` 会把「上游说了什么」抹成「上游没说」，而那正是本仓反复禁止的那种混淆。
/// 转成文本至少保真。
fn observed_string_list(value: Option<&Value>) -> Option<Vec<String>> {
    value.and_then(|value| value.as_array()).map(|items| {
        items
            .iter()
            .map(|item| match item {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .collect()
    })
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
            status: observed_i64(self.status.as_ref())
                .or_else(|| observed_i64(data.and_then(|d| d.status.as_ref()))),
            priv_status: observed_i64(self.priv_status.as_ref())
                .or_else(|| observed_i64(data.and_then(|d| d.priv_status.as_ref()))),
            fail_process: observed_string_list(self.fail_process.as_ref())
                .or_else(|| observed_string_list(data.and_then(|d| d.fail_process.as_ref())))
                .map(truncate_fail_process),
            ext_name: observed_string(self.ext_name.as_ref())
                .or_else(|| observed_string(data.and_then(|d| d.ext_name.as_ref()))),
            file_size: observed_i64(self.file_size.as_ref())
                .or_else(|| observed_i64(data.and_then(|d| d.file_size.as_ref()))),
            // 这两个走同一套「顶层优先、`data` 兜底」。注意 `observed_i64` 只认 JSON
            // 数字，所以上游把码率写成字符串时读出来是 `None`（不猜值）——这是本仓
            // 既有取舍，不因为它而放宽。
            bit_rate: observed_i64(self.bit_rate.as_ref())
                .or_else(|| observed_i64(data.and_then(|d| d.bit_rate.as_ref()))),
            time_length: observed_i64(self.time_length.as_ref())
                .or_else(|| observed_i64(data.and_then(|d| d.time_length.as_ref()))),
        }
    }

    /// 这次响应里那个可用的播放地址。
    ///
    /// 两层都看，顺序不变（`data` 先、顶层后），但**每层各自判空**：此前是「先选层
    /// 再判空」，`data.url` 只要是 `Some("")` 就已经选中了它，顶层那个有效地址根本
    /// 轮不到——而「同一个值可能放在两层里的任意一层」正是当初写两层的理由。
    fn play_url(&self) -> Option<String> {
        self.data
            .as_ref()
            .and_then(|d| d.url.as_ref())
            .and_then(StringOrStrings::first_non_empty)
            .or_else(|| self.url.as_ref().and_then(StringOrStrings::first_non_empty))
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
    /// 上游给的权益明细：**有哪几条、各自到什么时候**。这是关于会员权益的唯一口径
    /// ——此前还额外返回一个账号级的聚合布尔值（「有没有会员」），那个值既冗余又
    /// 低效，已删除。
    pub entitlements: Vec<VipEntitlement>,
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
        // 走 `/youth/union/vip` 而不是 `/user/vip/detail`：两者打的是同一个上游 URL
        // （`kugouvip.kugou.com/v1/get_union_vip`），差别只在**参数**——`/user/vip/detail`
        // 只带 `busi_type=concept`，返回 2 条权益；这一条额外带
        // `opt_product_types=dvip,qvip`，返回 4 条。换路由的**唯一**原因就是拿到完整
        // 的权益明细，其余语义不变。
        let url = format!("{}/youth/union/vip", self.base_url);
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
            busi_vip: Vec::new(),
        });

        // `busi_vip` 为空是**正常状态**（这个账号一条权益都没有），不是错误：上面的
        // 解析已经成功，此时得到一个空的 entitlements 并照常返回，绝不在这里报
        // 「VIP 校验失败」——那会把「没有权益」和「问不到上游」混成同一件事。
        let entitlements = data
            .busi_vip
            .iter()
            .map(VipBusinessEntry::to_entitlement)
            .collect();

        Ok(VipStatusResult {
            entitlements,
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

        let observation = parsed.observation();
        let play_url = parsed.play_url();

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
                bit_rate: None,
                time_length: None,
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
    ///
    /// 这里照实落的是**无损档**的形状：`bitRate` 是 `0`。断言写 `Some(0)` 而不是
    /// `None` 或「有值」——0 是上游明确报出来的一个数，读成「没说」就等于把
    /// 「上游报了 0」和「上游没报」混成一件事，而展示侧正是靠这个区分走不走
    /// `fileSize × 8 ÷ timeLength` 的回退。
    #[test]
    fn parses_success() {
        let parsed = parse(
            r#"{"status":1,"data":{},"priv_status":1,"extName":"flac","fileSize":29183562,"bitRate":0,"timeLength":180,"fileName":"song.flac","url":"http://example.invalid/a.flac"}"#,
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
                bit_rate: Some(0),
                time_length: Some(180),
            }
        );
        assert!(!upstream_refused_quality(observation.status));
    }

    /// 上游把值放在顶层还是 `data` 下并不稳定——`play_url` 就是为此写了两层。
    /// 观测字段必须同样两层都认，否则换一种包裹方式落盘的就是一片空白。
    ///
    /// 本轮新增的两个字段也走这条路径：这里的 `bitRate` 刻意取一个非 0 的精确值
    /// （mp3 320 的形状，与上一条测试的无损 0 配成两种取值），好让「读出来了」
    /// 与「读成 0 了」不会互相掩盖。
    #[test]
    fn parses_observation_nested_under_data() {
        let parsed = parse(
            r#"{"data":{"status":2,"priv_status":0,"fail_process":["buy"],"extName":"mp3","fileSize":1234,"bitRate":320000,"timeLength":135}}"#,
        );

        assert_eq!(
            parsed.observation(),
            UpstreamObservation {
                status: Some(2),
                priv_status: Some(0),
                fail_process: Some(vec!["buy".to_string()]),
                ext_name: Some("mp3".to_string()),
                file_size: Some(1234),
                bit_rate: Some(320000),
                time_length: Some(135),
            }
        );
    }

    /// 顶层优先这条规则对新字段同样成立：两层都给了值时取顶层的那个。
    ///
    /// 注意这里是**逐字段**择一（每个字段各自 `顶层.or_else(data)`），不是整层择一：
    /// `data` 里的值只在顶层**那个字段**缺失时才被用到。与 play_url 的
    /// `an_empty_data_url_does_not_mask_the_top_level_one` 是同一条宽容度的两个面。
    #[test]
    fn top_level_observation_wins_over_nested_for_bit_rate() {
        let parsed = parse(
            r#"{"data":{"bitRate":128000,"timeLength":135},"bitRate":1746914,"timeLength":135}"#,
        );
        let observation = parsed.observation();

        assert_eq!(observation.bit_rate, Some(1746914), "顶层有值就用顶层的");
        assert_eq!(observation.time_length, Some(135));

        // 反过来（顶层没给）才轮到 `data`：否则上面那条断言可能只是「两层都读到了
        // 顶层」的巧合，而不是真的走了 or_else 的兜底分支。
        let parsed = parse(r#"{"data":{"bitRate":128000},"status":1}"#);
        assert_eq!(parsed.observation().bit_rate, Some(128000));
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

    // ---- 观测字段的宽容度：只让它们读不出来，不让它们弄挂请求 ----

    /// 上游把某个**只作记录**的字段换成另一种写法时，这次响应仍然要能播。
    ///
    /// 钉的是一个已复现的失败：`fileSize` 是字符串时，定型的 `Option<i64>` 会让**整条
    /// 响应解析失败**——而那份响应里 `url` 就在同一个 body 里，用户拿到的却是「上游请求
    /// 失败」，指向完全错误的方向。这几个字段没有一个参与取链，不该有否决这次播放的权力。
    #[test]
    fn unreadable_observation_shapes_do_not_fail_the_request() {
        let parsed = parse(
            r#"{"status":1,"data":{},"extName":"mp3","fileSize":"9412859","bitRate":"320000","timeLength":"135","url":"http://x/y.mp3"}"#,
        );
        let observation = parsed.observation();
        assert_eq!(observation.status, Some(1), "认得出的字段照常读出来");
        assert_eq!(observation.ext_name.as_deref(), Some("mp3"));
        assert_eq!(
            observation.file_size, None,
            "数字字符串不算数字：这里不猜值"
        );
        // 新增的两个字段共用 `observed_i64`，因此同样拒绝数字字符串。这条不是顺带
        // 复述：码率会被展示侧拿去与「所请求档位」对照，猜错一个数等于在一个凭空
        // 捏出来的值上做判断，比留空更糟。
        assert_eq!(observation.bit_rate, None, "码率的数字字符串同样不算数字");
        assert_eq!(
            observation.time_length, None,
            "时长的数字字符串同样不算数字"
        );
        assert_eq!(
            parsed.play_url().as_deref(),
            Some("http://x/y.mp3"),
            "一个读不出来的观测字段不得让这次响应里可用的地址失效"
        );

        // `fail_process` 不是数组：整条读不出来，别的字段不受影响。
        let observation =
            parse(r#"{"status":2,"data":{},"priv_status":0,"fail_process":"pkg"}"#).observation();
        assert_eq!(observation.status, Some(2));
        assert_eq!(observation.priv_status, Some(0));
        assert_eq!(observation.fail_process, None);

        // 元素类型不对：整条数组照收，元素记它的 JSON 文本——读成「没说」等于抹掉
        // 上游刚说过的话。
        assert_eq!(
            parse(r#"{"status":2,"data":{},"fail_process":[1,"buy"]}"#)
                .observation()
                .fail_process,
            Some(vec!["1".to_string(), "buy".to_string()])
        );
    }

    /// 边界写清楚：本次**只**放宽了观测字段。`url` 的形状仍然定型（字符串或字符串
    /// 数组），认不出的形状照样整条失败——那是本次改动之前就有的行为，本次不顺手改。
    #[test]
    fn url_shape_is_still_typed() {
        assert!(
            serde_json::from_str::<SongUrlResponse>(r#"{"url":123}"#).is_err(),
            "这条断言如果开始失败，说明有人放宽了 url 的形状——那是另一件事，要单独论证"
        );
    }

    /// 一层的空值不得盖掉另一层的有效值。这是本次**有意改了行为**的一处（见 `play_url`）。
    ///
    /// 没有证据表明上游真的这么答过；它是「同一个值可能放在两层里任意一层」这条既有前提
    /// 的自然结论——旧的「先选层再判空」在 `data.url` 为空串时会直接返回「没地址」，
    /// 而顶层的地址是可用的。
    #[test]
    fn an_empty_data_url_does_not_mask_the_top_level_one() {
        assert_eq!(
            parse(r#"{"data":{"url":""},"url":"http://top/a.mp3"}"#)
                .play_url()
                .as_deref(),
            Some("http://top/a.mp3")
        );
        // 数组形态取第一个非空项：原有行为，一并钉住。
        assert_eq!(
            parse(r#"{"url":["","http://first/a.mp3"]}"#)
                .play_url()
                .as_deref(),
            Some("http://first/a.mp3")
        );
    }

    // ---- 会员权益明细（`/youth/union/vip`） ----
    //
    // 这些测试只走「解析 → 落成权益」这条纯函数路径：真正的 `fetch_vip_status` 要
    // 打网络（本机 KuGouMusicApi），在这里跑不了，而容易出错的那一段（字段归属、
    // 时间解释、容错）恰好全在纯函数里。

    /// 复刻 `fetch_vip_status` 里取 `busi_vip` 的两行，好把 fixture 走完生产代码
    /// 的映射（`VipBusinessEntry::to_entitlement`），而不是在测试里另写一份。
    fn parse_entitlements(body: &str) -> Vec<VipEntitlement> {
        let parsed: VipDetailResponse = serde_json::from_str(body).expect("vip fixture 必须能解析");
        parsed
            .data
            .unwrap_or(VipDetailData {
                busi_vip: Vec::new(),
            })
            .busi_vip
            .iter()
            .map(VipBusinessEntry::to_entitlement)
            .collect()
    }

    /// 四条并列权益。`busi_vip` 的**字段名与条目形状**取自实际响应，**取值一律是
    /// 合成占位**；顶层包裹
    /// （`status` / `data`）只是形状示意——本仓没有存下整条 body。
    ///
    /// 断言的是「两个时间字段各自归位、且按**本地时区**解释」而不是某个固定的
    /// 时间戳数字：那个数字取决于运行机器的时区，写死它等于要求 CI 在特定时区里跑
    /// （见下面两处断言的说明）。
    #[test]
    fn parses_entitlement_details_with_their_own_windows() {
        let entitlements = parse_entitlements(
            r#"{"status":1,"data":{"is_vip":1,"is_music_vip":0,"vip_type":0,"busi_vip":[
                {"product_type":"svip","is_vip":1,"busi_type":"concept","vip_begin_time":"2030-01-01 00:00:00","vip_end_time":"2030-02-01 00:00:00","vip_limit_quota":{"total":500}},
                {"product_type":"tvip","is_vip":1,"busi_type":"concept","vip_begin_time":"2030-01-01 00:00:00","vip_end_time":"2030-07-01 12:34:56","vip_limit_quota":{}},
                {"product_type":"qvip","is_vip":0,"busi_type":"concept","vip_begin_time":"2030-01-01 00:00:00","vip_end_time":"2030-01-02 00:00:00"},
                {"product_type":"dvip","is_vip":0,"busi_type":"concept","vip_begin_time":"2030-01-01 00:00:00","vip_end_time":"2030-01-02 00:00:00"}
            ]}}"#,
        );

        assert_eq!(
            entitlements
                .iter()
                .map(|e| e.product_type.as_str())
                .collect::<Vec<_>>(),
            vec!["svip", "tvip", "qvip", "dvip"],
            "busi_vip 有几条就落几条，顺序与上游一致：它是并列列表，不是映射"
        );
        assert!(entitlements[0].active);
        assert!(
            !entitlements[2].active,
            "is_vip 为 0 的条目照常落盘，只是 active 为 false"
        );

        let begin = entitlements[0].begin_at.expect("正常时间串必须解析出来");
        let end = entitlements[0].end_at.expect("正常时间串必须解析出来");
        // **不要**在这里断言 `end - begin` 等于 31 天：窗口跨过夏令时切换点时，两个
        // 本地时刻之间的 UTC 秒数会多/少一个小时（澳大利亚 Lord Howe 岛这种带半小时
        // 偏移又有夏令时的时区实测会挂）。能钉且与时区无关的写法是两个字段**各自**按
        // 本地时区渲染回去都等于原串——字段接反、偏移一小时、被当成 UTC 解释，三种
        // 错法都会让其中一条失败。
        let rendered = |ts: i64| {
            Local
                .timestamp_opt(ts, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        assert_eq!(rendered(begin), "2030-01-01 00:00:00");
        assert_eq!(rendered(end), "2030-02-01 00:00:00");
        assert!(end > begin, "接反了这里会失败");
    }

    /// 时间串的三种读不出：空串、格式不同、形状根本不是字符串。
    ///
    /// 它们一律是 `None`，且**不能把整条响应升级成失败**——这一层只是记录。
    #[test]
    fn unreadable_time_strings_become_none_without_failing_the_response() {
        assert!(local_time_to_epoch(Some("2030-01-01 00:00:00")).is_some());
        assert_eq!(local_time_to_epoch(Some("")), None, "空串是「上游没说」");
        assert_eq!(local_time_to_epoch(None), None);
        assert_eq!(
            local_time_to_epoch(Some("2030-01-01")),
            None,
            "只有日期没有钟点的串不是这个格式"
        );
        assert_eq!(local_time_to_epoch(Some("2030/01/01 00:00:00")), None);

        let entitlements = parse_entitlements(
            r#"{"status":1,"data":{"busi_vip":[
                {"product_type":"svip","is_vip":1,"vip_begin_time":"","vip_end_time":"not-a-time"},
                {"product_type":"tvip","is_vip":1,"vip_begin_time":1700000000,"vip_end_time":null}
            ]}}"#,
        );
        assert_eq!(entitlements.len(), 2, "时间读不出来不等于整条响应失败");
        assert_eq!(entitlements[0].begin_at, None);
        assert_eq!(entitlements[0].end_at, None);
        assert!(
            entitlements[0].active,
            "时间缺失不影响 is_vip 的判定：权益在不在期由 active 说"
        );
        assert_eq!(
            entitlements[1].begin_at, None,
            "数字形状的时间不算时间：这里不猜值"
        );
        assert_eq!(entitlements[1].end_at, None);
    }

    /// `busi_vip: []`（乃至整个 `data` 缺失）是**正常状态**：这个账号一条权益都没有。
    /// 它不是「VIP 校验失败」——解析照常成功，给出一个空的权益列表。
    #[test]
    fn empty_busi_vip_is_a_normal_state_not_a_failure() {
        assert!(
            parse_entitlements(r#"{"status":1,"data":{"is_vip":0,"vip_type":0,"busi_vip":[]}}"#)
                .is_empty()
        );
        assert!(
            parse_entitlements(r#"{"status":1,"data":{"is_vip":0,"vip_type":0}}"#).is_empty(),
            "没有 busi_vip 键与空数组是同一件事"
        );
        assert!(
            parse_entitlements(r#"{"status":1}"#).is_empty(),
            "连 data 都没有也只是「没有权益」"
        );
    }

    /// 认不出的 `product_type` 必须**原样透传**。这个取值域不在本仓手里（实测至少
    /// 4 个），丢掉一条就等于在后台把「这个账号还有一条权益」抹掉；缺失与空串同理
    /// ——条目本身不能丢，只能把名字留空。
    #[test]
    fn unknown_product_types_are_passed_through() {
        let entitlements = parse_entitlements(
            r#"{"status":1,"data":{"busi_vip":[
                {"product_type":"future-type","is_vip":1},
                {"product_type":"","is_vip":1},
                {"is_vip":1}
            ]}}"#,
        );

        assert_eq!(entitlements.len(), 3, "三条都要留下");
        assert_eq!(entitlements[0].product_type, "future-type");
        assert_eq!(entitlements[1].product_type, "");
        assert_eq!(
            entitlements[2].product_type, "",
            "缺失读成空串，不是丢掉这一条"
        );
        assert!(entitlements.iter().all(|entry| entry.active));
    }

    /// `is_vip` 缺失读成「上游没表态」，即 **不在期**。
    ///
    /// 这条是 wire 契约的一部分：界面按 `active` 决定这一条的去留与文案，读成
    /// 「在期」会把一条上游从未承认的权益显示成已生效——而那种错误不会有任何一层
    /// 报错。时间字段与它互不影响（读得出时间不等于在期）。
    #[test]
    fn missing_is_vip_reads_as_not_active() {
        let entitlements = parse_entitlements(
            r#"{"status":1,"data":{"busi_vip":[
                {"product_type":"svip","vip_begin_time":"2030-01-01 00:00:00"}
            ]}}"#,
        );

        assert_eq!(entitlements.len(), 1, "`is_vip` 缺失不影响条目本身");
        assert!(!entitlements[0].active);
        assert!(
            entitlements[0].begin_at.is_some(),
            "能不能读出时间与在不在期是两件事"
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
