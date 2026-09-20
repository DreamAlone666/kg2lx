use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AdminUnauthorized,
    SourceNotFound,
    SourceDisabled,
    AccountNotFound,
    AccountDisabled,
    AccountNotVip,
    LoginSessionNotFound,
    UpstreamRequestFailed,
    UpstreamLoginFailed,
    UpstreamVipCheckFailed,
    UpstreamPlayUrlEmpty,
    /// 上游**明确拒绝**了这一次取链（`/song/url` 的 `status:2`）。它与
    /// `UpstreamPlayUrlEmpty` 是两件事：后者是「没拿到 url、且上游也没有表达
    /// 拒绝」，一个 `status:2` 会被后者含混地吞掉，用户就永远看不出是权益/档位
    /// 的问题还是别的什么。
    ///
    /// 开启降级后，每一个被拒的**中间**档也复用这个码——它只描述上游的态度，
    /// 不描述我们的策略；「这是同一次请求的第几档」由日志的
    /// `quality_fallback_index` 与 `client_request_id` 承载。
    UpstreamQualityRefused,
    /// 沿档位阶梯降到底仍被拒。与 `UpstreamQualityRefused` 的唯一区别是
    /// 「我们已经试过更低的档位」，即 `quality_fallback_index > 0`。
    UpstreamQualityExhausted,
    UpstreamRateLimited,
    InvalidRequest,
    /// 改名被拒的两种原因要用户做的事不同（换个名字 / 缩短），合成一个
    /// `INVALID_REQUEST` 会让前端只能去 substr 英文 message 才能分辨。
    SourceNameEmpty,
    SourceNameTooLong,
    InternalError,
}

impl ErrorCode {
    /// 线上字符串（SCREAMING_SNAKE_CASE）的唯一权威定义。
    ///
    /// 此前只有一个来源：`#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`，于是任何
    /// 需要在 JSON 之外拿到字符串的地方都只能绕道 `serde_json::to_string` 再剪引号
    /// （runtime.rs 的 `error_code_string`）。要枚举全部取值就只能靠肉眼看 enum，
    /// 无法写测试。这个 match 刻意**不写 `_ =>` 兜底分支**：新增变体时编译器会直接
    /// 报漏匹配，强制人回来补字符串——`ALL` 是手写的、没有这种强制力。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AdminUnauthorized => "ADMIN_UNAUTHORIZED",
            Self::SourceNotFound => "SOURCE_NOT_FOUND",
            Self::SourceDisabled => "SOURCE_DISABLED",
            Self::AccountNotFound => "ACCOUNT_NOT_FOUND",
            Self::AccountDisabled => "ACCOUNT_DISABLED",
            Self::AccountNotVip => "ACCOUNT_NOT_VIP",
            Self::LoginSessionNotFound => "LOGIN_SESSION_NOT_FOUND",
            Self::UpstreamRequestFailed => "UPSTREAM_REQUEST_FAILED",
            Self::UpstreamLoginFailed => "UPSTREAM_LOGIN_FAILED",
            Self::UpstreamVipCheckFailed => "UPSTREAM_VIP_CHECK_FAILED",
            Self::UpstreamPlayUrlEmpty => "UPSTREAM_PLAY_URL_EMPTY",
            Self::UpstreamQualityRefused => "UPSTREAM_QUALITY_REFUSED",
            Self::UpstreamQualityExhausted => "UPSTREAM_QUALITY_EXHAUSTED",
            Self::UpstreamRateLimited => "UPSTREAM_RATE_LIMITED",
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::SourceNameEmpty => "SOURCE_NAME_EMPTY",
            Self::SourceNameTooLong => "SOURCE_NAME_TOO_LONG",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// 全部变体，供测试遍历（例如逐个校验 `as_str` 与 serde 一致）。
    ///
    /// 这份清单**不受编译器约束**——漏掉一个变体不会有任何编译期报错，真正强制
    /// 改动的只有 `as_str` 的穷尽 match。因此遍历 `ALL` 的测试是这里的安全网：
    /// 新变体若忘了加进来，测试覆盖不到它，而 `as_str` 的部分仍需人工保证。
    ///
    /// `allow(dead_code)`：唯一的消费方是 `#[cfg(test)]` 的测试模块，而非测试的
    /// bin target 看不到它，于是会报「从未被使用」。不要因为这条警告把它删掉——
    /// 删了安全网就没了。
    #[allow(dead_code)]
    pub const ALL: [ErrorCode; 18] = [
        ErrorCode::AdminUnauthorized,
        ErrorCode::SourceNotFound,
        ErrorCode::SourceDisabled,
        ErrorCode::AccountNotFound,
        ErrorCode::AccountDisabled,
        ErrorCode::AccountNotVip,
        ErrorCode::LoginSessionNotFound,
        ErrorCode::UpstreamRequestFailed,
        ErrorCode::UpstreamLoginFailed,
        ErrorCode::UpstreamVipCheckFailed,
        ErrorCode::UpstreamPlayUrlEmpty,
        ErrorCode::UpstreamQualityRefused,
        ErrorCode::UpstreamQualityExhausted,
        ErrorCode::UpstreamRateLimited,
        ErrorCode::InvalidRequest,
        ErrorCode::SourceNameEmpty,
        ErrorCode::SourceNameTooLong,
        ErrorCode::InternalError,
    ];

    /// 「重试可能成功」这件事由服务端判定，而不是交给客户端脚本。
    ///
    /// **Why:** LX 侧的音源脚本在导入时就固定下来，LX 从不自动更新它——脚本一旦
    /// 落到用户机器上，其判断逻辑就再也不会变。若把分类逻辑写在脚本里，将来出现
    /// 新的瞬时错误，就必须让每一个用户手动删掉再重新导入音源才能受益。判定放服务端
    /// 则只随服务端发布生效，已导入的脚本无需做任何事。
    ///
    /// **How to apply:** 只有真正「等一下再试可能就好了」的错误才返回 true；
    /// 参数错误、缺失资源一类的返回 false。目前唯一的瞬时情形是上游限流。
    pub fn is_retryable(self) -> bool {
        matches!(self, Self::UpstreamRateLimited)
    }
}

#[derive(Debug)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub status: StatusCode,
    pub is_admin: bool,
}

impl AppError {
    pub fn admin_unauthorized() -> Self {
        Self {
            code: ErrorCode::AdminUnauthorized,
            message: "unauthorized".into(),
            status: StatusCode::UNAUTHORIZED,
            is_admin: false,
        }
    }

    pub fn source_not_found() -> Self {
        Self {
            code: ErrorCode::SourceNotFound,
            message: "source not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    pub fn source_disabled() -> Self {
        Self {
            code: ErrorCode::SourceDisabled,
            message: "source is disabled".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn account_not_found() -> Self {
        Self {
            code: ErrorCode::AccountNotFound,
            message: "account not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    pub fn account_disabled() -> Self {
        Self {
            code: ErrorCode::AccountDisabled,
            message: "account is disabled".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn account_not_vip() -> Self {
        Self {
            code: ErrorCode::AccountNotVip,
            message: "account vip is not active".into(),
            status: StatusCode::FORBIDDEN,
            is_admin: false,
        }
    }

    pub fn login_session_not_found() -> Self {
        Self {
            code: ErrorCode::LoginSessionNotFound,
            message: "login session not found".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: true,
        }
    }

    pub fn upstream_request_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamRequestFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_login_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamLoginFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_vip_check_failed(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UpstreamVipCheckFailed,
            message: msg.into(),
            status: StatusCode::BAD_GATEWAY,
            is_admin: true,
        }
    }

    pub fn upstream_play_url_empty() -> Self {
        Self {
            code: ErrorCode::UpstreamPlayUrlEmpty,
            message: "play url unavailable".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    /// `is_admin: false` 的理由与 `upstream_rate_limited()` 完全一致：运行时链路
    /// 可达、面向客户端脚本，而脚本只分支 `>= 400`；`into_response` 的脱敏名单
    /// 只覆盖四个诊断码，所以这个 message 无论取什么值都会原样穿过。选 false 是
    /// 为了与相邻的运行时错误同档，不是为了把 message 藏起来。
    pub fn upstream_quality_refused() -> Self {
        Self {
            code: ErrorCode::UpstreamQualityRefused,
            message: "requested quality refused by upstream".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    /// `is_admin: false` 的理由同 `upstream_quality_refused()`。
    pub fn upstream_quality_exhausted() -> Self {
        Self {
            code: ErrorCode::UpstreamQualityExhausted,
            message: "no quality available after fallback".into(),
            status: StatusCode::NOT_FOUND,
            is_admin: false,
        }
    }

    /// `is_admin: false`（与最近的邻居 `upstream_play_url_empty()` 一致）：它是运行
    /// 时链路可达、面向客户端的错误，且 `into_response` 的脱敏名单只覆盖四个诊断码，
    /// 这个 code 的 message 无论取哪个值都会原样穿过——所以这里选 false 不是为了让
    /// message 被藏起来，而是为了跟同类保持一致；不要为此去动 `into_response`。
    pub fn upstream_rate_limited() -> Self {
        Self {
            code: ErrorCode::UpstreamRateLimited,
            message: "upstream rate limited".into(),
            status: StatusCode::TOO_MANY_REQUESTS,
            is_admin: false,
        }
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidRequest,
            message: msg.into(),
            status: StatusCode::BAD_REQUEST,
            is_admin: false,
        }
    }

    /// `is_admin: false` 但 message 不被 `into_response` 脱敏——脱敏名单只有
    /// 上游/内部错误，这里是用户可以自我修正的入参问题，报文要原样到客户端。
    pub fn source_name_empty() -> Self {
        Self {
            code: ErrorCode::SourceNameEmpty,
            message: "name is empty after removing characters that would break the script".into(),
            status: StatusCode::BAD_REQUEST,
            is_admin: false,
        }
    }

    pub fn source_name_too_long(len: usize, limit: usize) -> Self {
        Self {
            code: ErrorCode::SourceNameTooLong,
            message: format!("name is too long: {} bytes (limit {} bytes)", len, limit),
            status: StatusCode::BAD_REQUEST,
            is_admin: false,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InternalError,
            message: msg.into(),
            status: StatusCode::INTERNAL_SERVER_ERROR,
            is_admin: false,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: ErrorCode,
    message: String,
    /// 只在为 true 时出现，为 false 时字段整个消失——见 `is_false` 的说明。
    #[serde(skip_serializing_if = "is_false")]
    retryable: bool,
}

/// 给 `skip_serializing_if` 用：`Option` 之外的字段默认没有「缺省即省略」的语义，
/// 需要一个显式的谓词。
///
/// 为什么必须省略而不是发一个 `retryable: false`：`retryable` 是新字段，而除
/// `UPSTREAM_RATE_LIMITED` 之外的每个错误信封，字节都必须保持不变——SvelteKit 的
/// `describeApiError` 会检查 `code` / `message` 并对四个诊断码做特殊处理，多出来
/// 的字段虽不至于直接破坏解析，却会改变已上线行为的输入面。只在 true 时出现，
/// 等于对其余错误零影响。
fn is_false(v: &bool) -> bool {
    !*v
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let msg = if self.is_admin {
            self.message
        } else {
            match self.code {
                ErrorCode::UpstreamRequestFailed
                | ErrorCode::UpstreamLoginFailed
                | ErrorCode::UpstreamVipCheckFailed
                | ErrorCode::InternalError => "internal error".into(),
                _ => self.message,
            }
        };
        let body = ErrorBody {
            code: self.code,
            message: msg,
            retryable: self.code.is_retryable(),
        };
        (self.status, Json(body)).into_response()
    }
}

impl fmt::Display for AppError {
    /// 两段式 `"{code}: {message}"` 的形状不能动：它以诊断文本的形式落进
    /// `account.last_error`，是运维事后排查的凭据。这里只是不再用 Rust 变体名
    /// （`UpstreamRateLimited`）而用线上字符串（`UPSTREAM_RATE_LIMITED`）——
    /// 两者指的是同一件事，但前者是内部实现名，会随重构无声地变。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// `as_str` 是手写映射，线上格式却由 `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`
    /// 决定。两者一旦漂移，出现在日志 error_code 与脚本分支里的字符串就和接口
    /// 返回的 code 不是同一个东西——而这种不一致不会以任何形式报错。
    #[test]
    fn as_str_matches_serde() {
        for code in ErrorCode::ALL {
            let via_serde = serde_json::to_value(code).unwrap();
            assert_eq!(
                via_serde.as_str().unwrap(),
                code.as_str(),
                "{code:?} 的 serde 表示与 as_str 不一致"
            );
        }
    }

    /// 两个新码的线上字符串是**对外契约**：已导入的脚本用信封里的 code 去查脚本
    /// 自带的本地字典（键正是 `as_str()`），Web 前端也按字面量索引。而 `as_str`
    /// 与 serde 都从变体名派生，重命名变体时两边会一起变、`as_str_matches_serde`
    /// 依旧全绿，契约却在无声漂移。这条断言把这两个字面量钉死：改名必须先改到
    /// 这里，也就必然会被评审看见。
    #[test]
    fn quality_error_code_strings_are_pinned() {
        assert_eq!(
            ErrorCode::UpstreamQualityRefused.as_str(),
            "UPSTREAM_QUALITY_REFUSED"
        );
        assert_eq!(
            ErrorCode::UpstreamQualityExhausted.as_str(),
            "UPSTREAM_QUALITY_EXHAUSTED"
        );
    }

    /// `ALL` 不受编译器约束，重复项会让上面那条测试悄悄少覆盖一个变体。
    #[test]
    fn all_is_duplicate_free() {
        let mut seen = Vec::new();
        for code in ErrorCode::ALL {
            assert!(!seen.contains(&code), "{code:?} 在 ALL 里出现了两次");
            seen.push(code);
        }
    }

    /// 重试与否由服务端说了算：脚本是冻结的，分类逻辑写死在脚本里就无法随服务端演进。
    #[test]
    fn only_rate_limit_is_retryable() {
        for code in ErrorCode::ALL {
            assert_eq!(
                code.is_retryable(),
                code == ErrorCode::UpstreamRateLimited,
                "{code:?} 的 is_retryable 判定与预期不符"
            );
        }
    }

    /// 唯一会带上 retryable 的信封，四个字段都要对得上——脚本真正分支的就是这些值。
    #[tokio::test]
    async fn retryable_envelope_marks_only_retryable() {
        let resp = AppError::upstream_rate_limited().into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["code"], "UPSTREAM_RATE_LIMITED");
        assert_eq!(value["message"], "upstream rate limited");
        assert_eq!(value["retryable"], true);
    }

    /// `UPSTREAM_RATE_LIMITED` 之外的每个错误信封都必须逐字节不变：不是
    /// `retryable: false`，而是这个键根本不存在。
    #[tokio::test]
    async fn non_retryable_envelope_omits_retryable() {
        let resp = AppError::upstream_play_url_empty().into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["code"], "UPSTREAM_PLAY_URL_EMPTY");
        assert!(
            value.get("retryable").is_none(),
            "非重试错误不应出现 retryable 字段，实际为 {:?}",
            value.get("retryable")
        );
    }

    /// 钉住前端 `describeApiError` 依赖的脱敏契约：四个诊断码的原文必须完整穿过
    /// （那才是运维当下的线索），而 `internal` 的报文要变成 "internal error"。
    #[tokio::test]
    async fn diagnostic_codes_keep_raw_message() {
        // is_admin: true 的诊断码——报文原样送到客户端。
        let passthrough = [
            (
                AppError::upstream_request_failed("raw request detail"),
                "UPSTREAM_REQUEST_FAILED",
                "raw request detail",
            ),
            (
                AppError::upstream_login_failed("raw login detail"),
                "UPSTREAM_LOGIN_FAILED",
                "raw login detail",
            ),
            (
                AppError::upstream_vip_check_failed("raw vip detail"),
                "UPSTREAM_VIP_CHECK_FAILED",
                "raw vip detail",
            ),
        ];
        for (err, code, message) in passthrough {
            let bytes = to_bytes(err.into_response().into_body(), usize::MAX)
                .await
                .unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["code"], code);
            assert_eq!(value["message"], message);
        }

        // is_admin: false 的 InternalError——报文被替换，不能泄露内部细节。
        let bytes = to_bytes(
            AppError::internal("boom").into_response().into_body(),
            usize::MAX,
        )
        .await
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["code"], "INTERNAL_ERROR");
        assert_eq!(value["message"], "internal error");
    }
}
