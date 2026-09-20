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
    /// 本行所描述的那次上游调用实际使用的酷狗质量参数（由 kugou_lite_client.rs 的
    /// map_quality 映射而来）。
    ///
    /// 只有 `upstream_endpoint == "song/url"` 的行才有值：`ensure_dfid` / `refresh_login`
    /// 这两条调用根本不带质量参数，给它们写一个「本来会要的质量」等于发明一个没发生过
    /// 的事实——同一个坑 refresh_attempted 已经踩过一次（字段是「试图」、文案是「已完成」）。
    /// 写入规则见 services/runtime.rs 的 append_log。
    ///
    /// `default` 是为了让本次改动**之前**落盘的 jsonl 行仍能读进来（它们没有这个键），
    /// 那些行读出来是 None，前端据此知道「这一行没有记录」，而不是「这一行没有音质」。
    #[serde(default)]
    pub upstream_quality: Option<String>,
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

    /// 本次改动**之前**落盘的 jsonl 行没有 `upstream_quality` 这个键。
    ///
    /// 缺了 `#[serde(default)]`，它们会整行反序列化失败——那意味着 logs 目录里现存的
    /// 全部历史行一行都读不出来，日志面板直接空掉（而不是少一个字段）。
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
    }

    /// 新行带上这个键，且能原样读回。
    #[test]
    fn round_trips_upstream_quality() {
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
            requested_quality: "flac24bit".into(),
            upstream_quality: Some("high".into()),
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
        assert!(json.contains(r#""upstream_quality":"high""#));

        let back: RuntimeLog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.upstream_quality.as_deref(), Some("high"));
        assert_eq!(back.requested_quality, "flac24bit");
    }
}
