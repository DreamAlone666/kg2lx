use super::provider::ProviderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogStage {
    /// 取链之前的准入检查失败（`services/runtime.rs` 的 `fetch_music_url` 顶部）。
    ///
    /// 由四条早退路径构造：音源被停用、账号记录不存在、账号被禁用、会员无效。
    /// 它此前只被设计、被前端词典标注（`web/src/lib/domain/labels.ts` 的
    /// 「预检」），生产代码却从不构造——于是最常落到用户身上的几种失败一条日志
    /// 都不写，而日志面板是 kg2lx 用户唯一能看到诊断的地方（洛雪音乐丢弃脚本抛出
    /// 的 `err.message`，只显示自己的「换源失败…」）。
    ///
    /// 本阶段的行只描述「没走到取链」：`upstream_endpoint` 为空、`status_code`
    /// 为 `None`、`retry_count` 为 0、`refresh_attempted` 为 false。该行真正承重的
    /// 是 `error_code`——读路径上 `RuntimeLog::sanitized` 会把 `error` 文本塌成
    /// 常量，机器可读的具体原因只剩这一个字段。
    ///
    /// 未知 runtime token（查不到音源）**不**走这里：`RuntimeLog.source_id` 是必填
    /// 字段、面板也按它过滤，凭空造一个 id 只会产出任何面板都查不出来的孤儿行。
    Precheck,
    EnsureDfid,
    RefreshLogin,
    FetchMusicUrl,
}

/// 一行运行时日志。
///
/// **序列化契约（不要改）**：新增字段一律只加 `#[serde(default)]`，**绝不加
/// `skip_serializing_if`**。理由不在服务端而在读侧：日志面板据此判断「这一行有没有上游
/// 观测」，用的是 `upstream_status !== null`——而在 JS 里 `undefined !== null` 为真。
/// 用 `skip_serializing_if` 把键整个省掉，等价于把「没有观测」变成「有观测但读不到」，
/// 面板会在**每一页**（包括全是历史行的页）渲染出一整列空的上游观测。
///
/// `None` 在这个结构里不是「省略」，而是一个明确的值：`None` = 这条链路没走到那一步 /
/// 上游没说，`Some(0)` / `Some("")` = 走到了并且上游说了 0 / 说了空串。两者常需区分，
/// 所以字段大量是 `Option`。
///
/// **关于 `#[serde(default)]` 到底在哪起作用**（这里曾经写错过，别再传播）：真正必须靠它
/// 才读得进老行的是**非 `Option`** 的字段——`quality_fallback_index: u8` 就是本轮唯一的
/// 一个，`stage` / `refresh_attempted` / `retry_count` 是更早的同例。给 `Option` 字段加
/// `default` 是**冗余**的：serde 内部用 `missing_field(...)` 处理缺键，而它把
/// `deserialize_option` 实现成直接 `visit_none()`，所以缺键的 `Option` 本来就读成 `None`。
/// 冗余但无害，而且能让人一眼看出「这个字段允许缺席」，所以留着——但不要因此以为
/// `Option` 字段的兼容性是被 `default` 保护的，也不要因此以为加了这个属性就可以给
/// 结构体加一个非 `Option` 的新字段。
///
/// 真正的兜底是测试：`reads_rows_that_omit_the_song_metadata_keys_entirely` 用最早那一代
/// 落盘行（连歌曲元数据三个键都没有）钉住整行仍能读入。`FsRuntimeLogRepo::list_by_source`
/// 对反序列化失败是 `Err(_) => continue`——一行失败就是那一行**从面板上消失**。
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
    /// **本次**向上游要的 LX 档位。
    ///
    /// 与 `requested_quality` 不同：后者是用户当初要的，本字段是这一次调用真正发出去的。
    /// 服务端开着「音质自动降级」时，同一首歌的一次入站请求会沿档位阶梯逐档重试，每一档
    /// 各写一行，于是 `requested_quality` 在那些行里保持不变（那是事实，不该被改写），
    /// 要看出「本次要的是另一档」只能靠这个字段。
    ///
    /// 未走到取链的行（预检 / `ensure_dfid` / `refresh_login`）为 `None`：什么都没要过，
    /// 写一个「本来会要的档位」就是发明一件没发生过的事。
    #[serde(default)]
    pub attempted_quality: Option<String>,
    /// 本次调用已经沿阶梯降了几档：0 = 就是用户请求的那一档，1.. = 降了几档。
    ///
    /// 与 `attempted_quality` 一起承载「这是同一次请求的第几档」。此前想着让错误码本身
    /// 表达这一步（例如每档一个码），但码描述的是**上游做了什么**（拒绝了这一档），
    /// 不是**我们做了什么**（我们又试了下一档）；两者混在一个字段里，任何一次策略调整
    /// 都要改动码的含义。分开之后，`UPSTREAM_QUALITY_REFUSED` 可以稳定地只表示
    /// 「上游明确拒绝了这一档」。
    ///
    /// 未走到取链的行为 0——与「第 0 档」同值，因此判断「是否发生降级」必须同时看
    /// `attempted_quality` 是否为 `None`。前端只在 > 0 时显示「降级 N」标记。
    ///
    /// **本行 `#[serde(default)]` 是本批新增字段里唯一真正承重的**：其余新增字段都是
    /// `Option`，而 serde 对缺键的 `Option` 本来就给 `None`（见结构体上的说明）；
    /// `u8` 不是 `Option`，缺了它就等于对**全部历史行**报 `missing field`，而
    /// `FsRuntimeLogRepo::list_by_source` 会把这些行整行跳过。
    #[serde(default)]
    pub quality_fallback_index: u8,
    /// 客户端上报的 LX 档位元数据，**原样**存。
    ///
    /// 来自 LX 脚本边界的扁平 `musicInfo.types`（即宿主 `meta.qualitys`）：它说的是
    /// 「LX 认为这首歌有哪些档位、各是多大」，**与账号权益无关**——那些档位很可能
    /// 这首歌的目录元数据里一直都有，只是这个账号拿不到。因此它是一份**记录**，
    /// 用来对照「客户端以为有什么」与「上游实际给了什么」，**不参与任何判断**。
    ///
    /// 形状不做断言（可能缺失、可能是任意 JSON），所以类型是 `Value` 而不是某个具体
    /// 结构：这是记录，不是契约，给它定 shape 只会让上游一改就整行解析失败。
    #[serde(default)]
    pub reported_types: Option<serde_json::Value>,
    /// 一次入站 LX 请求一个 id。
    ///
    /// 存在的唯一理由是把**同一次请求**写出的多行串起来，并与**两次独立的请求**区分开。
    /// 两件事在日志里长得一模一样，而它们都会发生：洛雪音乐对任何非「用户取消」的失败
    /// 会把整条取链流程无条件重跑一遍（于是同一首歌的失败成对出现），而服务端自己的降级
    /// 也会为一次请求写出多行。只看时间与 hash 分不开这两者，`retry_count` 也分不开
    /// （它只记「刷新登录之后重试同一档」）。
    #[serde(default)]
    pub client_request_id: Option<String>,
    /// 本行所描述的那次上游调用实际使用的酷狗质量参数（由 kugou_lite_client.rs 的
    /// map_quality 映射而来）。
    ///
    /// **由 `attempted_quality` 算出**，而不是由 `requested_quality`：降级那几行要能看出
    /// 「这一档向上游要的是 `128`」，若继续读用户请求的档位，阶梯上每一行都会记成同一个
    /// 值，降级在本该最显眼的那一列上完全不可见。
    ///
    /// 只有 `upstream_endpoint == "song/url"` 的行才有值：`ensure_dfid` / `refresh_login`
    /// 这两条调用根本不带质量参数，给它们写一个「本来会要的质量」等于发明一个没发生过
    /// 的事实——同一个坑 refresh_attempted 已经踩过一次（字段是「试图」、文案是「已完成」）。
    /// 写入规则见 services/runtime.rs 的 append_log。
    ///
    /// 本次改动**之前**落盘的行没有这个键，读出来是 None，前端据此知道「这一行没有记录」，
    /// 而不是「这一行没有音质」。这里的 `#[serde(default)]` 对此是**冗余**的（缺键的
    /// `Option` 本来就读成 `None`，见结构体上的说明），留着只为表意。
    #[serde(default)]
    pub upstream_quality: Option<String>,
    /// 上游返回的业务 status：1 成功 / 2 明确拒绝这一档 / 0 资源不存在或参数非法。
    ///
    /// **`None` 与 `Some(0)` 是两件事**：「上游什么都没说」（未走到取链的行、或响应体里
    /// 根本没有这个键）与「上游说了 0」含义相反，混同会让界面在整页预检行上都渲染出一个
    /// 不存在的上游观测。因此这里宁可要一个 `Option`，也不给一个默认 0。
    ///
    /// 落盘的理由：上游到底说了什么，此前在服务端被整个丢掉，只留下一个笼统的
    /// `UPSTREAM_PLAY_URL_EMPTY`。于是「这首歌没有这个音质」与「账号拿不到这个音质」
    /// 这两种完全不同的情形，事后只能靠人肉重放上游请求才能分辨。
    #[serde(default)]
    pub upstream_status: Option<i64>,
    /// 上游的 `priv_status`。与 `upstream_status` 同批落盘，同为**事实**。
    #[serde(default)]
    pub upstream_priv_status: Option<i64>,
    /// 上游的 `fail_process`，落盘前已在上游客户端侧截断（见 kugou_lite_client.rs 的上限常量）。
    ///
    /// **只作为事实，不作判据**：实测同一个账号在同一小时内，同一种拒绝给出过
    /// `["pkg","buy"]`、`["buy"]`、`[]` 三种取值，因此它不能单独用来判断原因。
    ///
    /// `Some(vec![])`（上游明说「没有阻碍」）与 `None`（上游没说）必须区分：前者本身
    /// 就是一条观测。序列化时**不得**给它加 `skip_serializing_if`。
    #[serde(default)]
    pub upstream_fail_process: Option<Vec<String>>,
    /// 上游实际返回的文件格式（`mp3` / `flac` / `mkv`）。
    ///
    /// 这是唯一能看出**静默回落**的地方：上游对没有 Hi-Res 声部的歌仍然回 `status: 1`，
    /// 只是把 128k 的 mp3 给你。不落这个字段，界面只能显示一个「成功」。
    #[serde(default)]
    pub upstream_ext_name: Option<String>,
    /// 上游实际返回的字节数。与 `upstream_ext_name` 配对，是「实际拿到的是不是所要的档位」
    /// 的自证据（请求 `high` 却拿到与 `quality=128` 完全相同的 `file_size`，就是静默回落）。
    #[serde(default)]
    pub upstream_file_size: Option<i64>,
    /// 上游自报的码率（bps）。**两个无损档恒为 0**——0 是「上游没报」而不是「0 bps」。
    ///
    /// 为什么要两个字段而不是一个：展示侧的码率取「本字段 > 0 时用它，否则用
    /// `upstream_file_size × 8 ÷ upstream_time_length`」。
    /// 实测同一首歌四档（本机带账号 cookie）：mp3 两档 `bitRate` 是精确的 128000 / 320000，
    /// 而 `flac` / `high` 两档**恒为 0**；同四档的 `fileSize × 8 ÷ timeLength` 则全有值，
    /// 约为 128 / 321 / 1038 / 1747 kbps，分离度足够区分 16bit 与 24bit。
    ///
    /// 于是两个字段各承担一半：`upstream_bit_rate` 是 mp3 档上更可信的精确值，
    /// `upstream_time_length` 只是那个回退公式的除数。**缺了时长就没有任何办法从已落盘的
    /// 数据里算出无损档的码率**，而 `ext_name` 分不出 `flac` 与 `flac24bit`（两者都是
    /// `flac`），只靠格式会漏掉「24bit 静默掉到 16bit」这一整类回落。
    ///
    /// 落盘层只陈述事实：这里**不做**码率回退、也不判断「本次返回是否低于所请求档位」。
    /// 回退算法属于展示侧（web/src/lib/domain/format.ts 的 effectiveBitRate）。
    #[serde(default)]
    pub upstream_bit_rate: Option<i64>,
    /// 上游返回的音频时长（秒）。单独看没有诊断价值，是上面那条回退公式的除数。
    #[serde(default)]
    pub upstream_time_length: Option<i64>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 本次改动**之前**落盘的 jsonl 行没有 `upstream_quality` 这个键，也没有后来加进来
    /// 的上游观测 / 本次档位 / 关联 id 那一批。
    ///
    /// 缺了 `#[serde(default)]`，它们会整行反序列化失败——那意味着 logs 目录里现存的
    /// 全部历史行一行都读不出来，日志面板直接空掉（而不是少一个字段）。后来每加一个字段
    /// 都会把这条测试的断言加长一点：它是这一整类改动的安全网，不是「upstream_quality
    /// 专用」的测试。
    /// 这条断言钉住向后兼容，fixture 照落盘行构造（取值合成）。
    #[test]
    fn reads_rows_written_before_upstream_quality_existed() {
        let old_row = r#"{
			"log_id":"log_00000000-0000-4000-8000-000000000002",
			"source_id":"src_00000000-0000-4000-8000-000000000001",
			"account_id":"acc_00000000-0000-4000-8000-000000000000",
			"provider":"kugou_lite",
			"action":"musicUrl",
			"track_title":null,"artist_name":null,"album_name":null,
			"request_hash":"803377EC09F511E4EB9E83FF8C9C8E6F",
			"album_audio_id":null,
			"requested_quality":"flac",
			"upstream_endpoint":"refresh_login",
			"stage":"refresh_login",
			"refresh_attempted":true,"retry_count":0,
			"ok":false,"status_code":null,
			"error_code":"UPSTREAM_LOGIN_FAILED",
			"latency_ms":0,"error":"upstream error",
			"created_at":1650000000
		}"#;

        let log: RuntimeLog =
            serde_json::from_str(old_row).expect("本次改动之前落盘的行必须仍能读入");
        assert_eq!(
            log.upstream_quality, None,
            "老行读出 None，语义是「这一行没有记录」，不是「这一行没有音质」"
        );
        assert_eq!(log.requested_quality, "flac", "其余字段不受影响");

        // 这批新字段在**读**侧也一律是 None / 0：老行根本没有这些键，而
        // `Option` 的缺省必须是「没记录」而不是任何看起来像事实的值。
        assert_eq!(
            (
                log.attempted_quality,
                log.quality_fallback_index,
                log.reported_types,
                log.client_request_id,
                log.upstream_status,
                log.upstream_priv_status,
                log.upstream_fail_process,
                log.upstream_ext_name,
                log.upstream_file_size,
                log.upstream_bit_rate,
                log.upstream_time_length,
            ),
            (
                None, 0, None, None, None, None, None, None, None, None, None
            ),
            "老行读出的这批新字段必须全部是「未记录」"
        );
    }

    /// 最早那一代落盘行连歌曲元数据三个键都没有。
    ///
    /// 这是本文件里唯一能钉住「`Option` 字段缺键也不会整行失败」的 fixture：其余 fixture
    /// 都把 `track_title` / `artist_name` / `album_name` 显式写成 `null`，于是无论 serde
    /// 对缺失的 `Option` 是「给 None」还是「报 missing field」，它们都照样绿。
    ///
    /// **为什么值得单独钉**：`FsRuntimeLogRepo::list_by_source` 对反序列化失败是
    /// `Err(_) => continue`，一行失败就是**这一行从面板上消失**（不是少一个字段），而最先
    /// 消失的恰恰是最老的历史行——它们是最需要保留的「以前是什么样」的记录。实测最早那一代的行全都缺这三个键。
    #[test]
    fn reads_rows_that_omit_the_song_metadata_keys_entirely() {
        // 形状照最早那一代落盘行构造；取值一律合成。
        let oldest_row = r#"{"log_id": "log_00000000-0000-4000-8000-000000000003", "source_id": "src_00000000-0000-4000-8000-000000000001", "account_id": "acc_00000000-0000-4000-8000-000000000000", "provider": "kugou_lite", "action": "musicUrl", "request_hash": "1335F720C701863F127FB14CCC9C08A2", "album_audio_id": null, "requested_quality": "flac24bit", "upstream_endpoint": "song/url", "ok": false, "status_code": 200, "latency_ms": 1, "error": "upstream error", "created_at": 1590000000}"#;

        let log: RuntimeLog = serde_json::from_str(oldest_row)
            .expect("最早那一代的行必须仍能读入，否则会从面板上消失");
        assert_eq!(log.track_title, None);
        assert_eq!(log.artist_name, None);
        assert_eq!(log.album_name, None);
        assert_eq!(log.requested_quality, "flac24bit");
        // 同一代还缺 stage / refresh_attempted / retry_count / error_code —— 那四个有
        // 默认值，顺带一起钉住（它们是同一批「加字段」改动留下的兼容面）。
        assert_eq!(log.stage, RuntimeLogStage::FetchMusicUrl);
        assert!(!log.refresh_attempted);
        assert_eq!(log.retry_count, 0);
        assert_eq!(log.error_code, None);
    }

    /// 新行带上这些键，且能原样读回。
    ///
    /// 取的是一行**降级后成功**的行：用户要 `flac`，上游拒绝了它，我们改要 `320k` 拿到了。
    /// 这一行同时承载了「用户要什么 / 我们实际要什么 / 降了几档 / 上游怎么答 / 真拿到了什么」，
    /// 是新增字段里信息最密的一种组合，用它做 round-trip 才覆盖得住。
    #[test]
    fn round_trips_upstream_quality() {
        let reported =
            serde_json::json!({ "flac": { "size": 29183562 }, "320k": { "size": 9412859 } });
        let log = RuntimeLog {
            log_id: "log_test".into(),
            source_id: "src_x".into(),
            account_id: "acc_x".into(),
            provider: ProviderKind::KugouLite,
            action: "musicUrl".into(),
            track_title: None,
            artist_name: None,
            album_name: None,
            request_hash: "A0AEFE0227938F366FF4F9D4F0F6CBE5".into(),
            album_audio_id: None,
            requested_quality: "flac".into(),
            attempted_quality: Some("320k".into()),
            quality_fallback_index: 1,
            reported_types: Some(reported.clone()),
            client_request_id: Some("req_test".into()),
            upstream_quality: Some("320".into()),
            upstream_status: Some(1),
            upstream_priv_status: Some(1),
            upstream_fail_process: Some(Vec::new()),
            upstream_ext_name: Some("flac".into()),
            upstream_file_size: Some(9_412_859),
            upstream_bit_rate: Some(320_000),
            upstream_time_length: Some(180),
            upstream_endpoint: "song/url".into(),
            stage: RuntimeLogStage::FetchMusicUrl,
            refresh_attempted: false,
            retry_count: 0,
            ok: true,
            status_code: Some(200),
            error_code: None,
            latency_ms: 71,
            error: None,
            created_at: 1_789_478_080,
        };

        let json = serde_json::to_string(&log).expect("serialize");
        assert!(json.contains(r#""upstream_quality":"320""#));
        assert!(
            json.contains(r#""attempted_quality":"320k""#),
            "本次实际要的档位必须落盘：降级后的那一行里 requested_quality 仍是用户当初要的档位"
        );

        let back: RuntimeLog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.upstream_quality.as_deref(), Some("320"));
        assert_eq!(back.requested_quality, "flac");
        assert_eq!(back.attempted_quality.as_deref(), Some("320k"));
        assert_eq!(back.quality_fallback_index, 1);
        assert_eq!(back.reported_types, Some(reported));
        assert_eq!(back.client_request_id.as_deref(), Some("req_test"));
        assert_eq!(back.upstream_status, Some(1));
        assert_eq!(back.upstream_priv_status, Some(1));
        assert_eq!(back.upstream_ext_name.as_deref(), Some("flac"));
        assert_eq!(back.upstream_file_size, Some(9_412_859));
        assert_eq!(back.upstream_bit_rate, Some(320_000));
        assert_eq!(back.upstream_time_length, Some(180));
    }

    /// 无损档的 `bitRate` 是 `0`，落盘必须**如实**存成 `Some(0)`，不能塌成 `None`。
    ///
    /// fixture 是实测那一行（本机带账号 cookie 请求 `quality=flac`）：`extName: "flac"`
    /// 配 `bitRate: 0`。两条断言各钉一个方向——json 里那个键要真的写成 `0`（不是被
    /// skip、也不是被写成 null），读回来要是 `Some(0)`。
    ///
    /// 为什么值得单独钉：`Some(0)` 与 `None` 在这里含义相反（「上游报了个 0」vs
    /// 「上游没说」），混同之后展示侧就分不出「这份观测里码率不可用」与「这份观测里
    /// 压根没有码率这一项」。**本层只陈述事实**：这里不断言任何算出来的码率，
    /// `file_size × 8 ÷ time_length` 那条回退属于展示侧（web 的 effectiveBitRate），
    /// 在服务端断言它等于把一个展示选择钉进落盘契约里。
    #[test]
    fn lossless_zero_bit_rate_is_stored_verbatim() {
        let row = r#"{
			"log_id":"log_flac","source_id":"src_x","account_id":"acc_x",
			"provider":"kugou_lite","action":"musicUrl",
			"track_title":null,"artist_name":null,"album_name":null,
			"request_hash":"H","album_audio_id":null,"requested_quality":"flac",
			"upstream_endpoint":"song/url","stage":"fetch_music_url",
			"refresh_attempted":false,"retry_count":0,"ok":true,"status_code":200,
			"error_code":null,"latency_ms":86,"error":null,"created_at":1690000000,
			"upstream_status":1,"upstream_ext_name":"flac","upstream_file_size":17525993,
			"upstream_bit_rate":0,"upstream_time_length":135
		}"#;

        let log: RuntimeLog = serde_json::from_str(row).expect("落盘行必须能读入");
        assert_eq!(log.upstream_bit_rate, Some(0), "上游报了 0 就得读成 0");
        assert_eq!(log.upstream_time_length, Some(135));
        assert_eq!(log.upstream_ext_name.as_deref(), Some("flac"));

        let json = serde_json::to_string(&log).expect("serialize");
        assert!(
            json.contains(r#""upstream_bit_rate":0"#),
            "0 必须原样写回 json，不能被折叠成 null 或被 skip：实际 json：{json}"
        );
        assert!(json.contains(r#""upstream_time_length":135"#));
    }

    /// **`None` 必须序列化成 `null`，不能把键整个省掉。**
    ///
    /// 这条不是风格问题：日志面板用 `upstream_status !== null` 决定要不要渲染整列上游
    /// 观测，而在 JS 里 `undefined !== null` 为 **true**。一旦给这些 `Option` 加上
    /// `skip_serializing_if = "Option::is_none"`，键会消失，前端读到的就是 `undefined`，
    /// 于是**每一页**（包括全是本次改动之前历史行的页）都会渲染出一整列空的「上游」。
    /// 症状是「多了一列没数据的列」，而根因在服务端的序列化属性上——不钉住的话，
    /// 下一次「顺手给 Option 加个 skip」就会重新引入它。
    #[test]
    fn absent_observation_serializes_as_null_not_omitted() {
        let log = RuntimeLog {
            log_id: "log_test".into(),
            source_id: "src_x".into(),
            account_id: "acc_x".into(),
            provider: ProviderKind::KugouLite,
            action: "musicUrl".into(),
            track_title: None,
            artist_name: None,
            album_name: None,
            request_hash: "A0AEFE0227938F366FF4F9D4F0F6CBE5".into(),
            album_audio_id: None,
            requested_quality: "flac".into(),
            attempted_quality: None,
            quality_fallback_index: 0,
            reported_types: None,
            client_request_id: None,
            upstream_quality: None,
            upstream_status: None,
            upstream_priv_status: None,
            upstream_fail_process: None,
            upstream_ext_name: None,
            upstream_file_size: None,
            upstream_bit_rate: None,
            upstream_time_length: None,
            upstream_endpoint: "".into(),
            stage: RuntimeLogStage::Precheck,
            refresh_attempted: false,
            retry_count: 0,
            ok: false,
            status_code: None,
            error_code: Some("SOURCE_NOT_FOUND".into()),
            latency_ms: 0,
            error: None,
            created_at: 1_789_478_080,
        };

        let json = serde_json::to_string(&log).expect("serialize");
        for key in [
            "upstream_status",
            "upstream_priv_status",
            "upstream_fail_process",
            "upstream_ext_name",
            "upstream_file_size",
            "upstream_bit_rate",
            "upstream_time_length",
            "attempted_quality",
            "client_request_id",
            "reported_types",
            "upstream_quality",
        ] {
            assert!(
                json.contains(&format!(r#""{key}":null"#)),
                "键 `{key}` 必须显式序列化成 null，而不是被 skip 掉：前端用 `!== null` 判断\
                 这一行有没有观测，undefined 会让判断恒真。实际 json：{json}"
            );
        }
        // 数字字段没有「缺失」这回事，恒为一个数——前端据此不必判空。
        assert!(json.contains(r#""quality_fallback_index":0"#));
    }

    /// `Some(vec![])`（上游明说「没有阻碍」）与 `None`（上游没说）必须能区分：
    /// 前者本身就是一条观测，把它读成「没记录」等于丢掉上游刚说的话。
    #[test]
    fn empty_fail_process_is_distinct_from_absent() {
        let row = |fail_process: &str| {
            format!(
                r#"{{"log_id":"log_x","source_id":"src_x","account_id":"acc_x",
                   "provider":"kugou_lite","action":"musicUrl",
                   "track_title":null,"artist_name":null,"album_name":null,
                   "request_hash":"H","album_audio_id":null,"requested_quality":"flac",
                   "upstream_endpoint":"song/url","stage":"fetch_music_url",
                   "refresh_attempted":false,"retry_count":0,"ok":false,
                   "status_code":200,"error_code":"UPSTREAM_QUALITY_REFUSED",
                   "latency_ms":1,"error":"upstream error","created_at":1,
                   "upstream_fail_process":{fail_process}}}"#
            )
        };

        let empty: RuntimeLog = serde_json::from_str(&row("[]")).expect("空数组必须能读入");
        assert_eq!(
            empty.upstream_fail_process,
            Some(Vec::new()),
            "上游明说没有阻碍，读出来必须是「说了，且是空」，不是「没说」"
        );

        let absent: RuntimeLog = serde_json::from_str(&row("null")).expect("null 必须能读入");
        assert_eq!(absent.upstream_fail_process, None);
    }
}
