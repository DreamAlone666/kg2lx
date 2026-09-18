use std::env;
use std::path::PathBuf;

pub struct Config {
    pub listen_addr: String,
    pub public_base_url: String,
    pub admin_token: String,
    pub kugou_api_base_url: String,
    pub data_dir: PathBuf,
    pub upstream_timeout_ms: u64,
    pub source_name_prefix: String,
    pub refresh_interval_secs: u64,
    pub web_dist_dir: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen_addr: env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into()),
            public_base_url: require_http_base_url("PUBLIC_BASE_URL"),
            admin_token: env::var("ADMIN_TOKEN").expect("ADMIN_TOKEN is required"),
            kugou_api_base_url: require_http_base_url("KUGOU_API_BASE_URL"),
            data_dir: env::var("DATA_DIR")
                .unwrap_or_else(|_| "./data".into())
                .into(),
            upstream_timeout_ms: env::var("UPSTREAM_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10000),
            source_name_prefix: env::var("SOURCE_NAME_PREFIX")
                .unwrap_or_else(|_| "酷狗概念版".into()),
            refresh_interval_secs: env::var("REFRESH_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6 * 3600),
            web_dist_dir: env::var("WEB_DIST_DIR").ok().map(PathBuf::from),
        }
    }

    /// 当前实例的 authority（`host[:port]`），用于脚本 `@description`。
    ///
    /// 去掉 scheme 与 path 后，`127.0.0.1:8787` 本身已蕴含环境语义，
    /// 因此描述里刻意不再写「本地」这类环境标签——36 字符的建议预算
    /// 不值得花在重复信息上。
    pub fn public_authority(&self) -> String {
        authority_of(&self.public_base_url)
    }
}

/// 校验并归一化一个 `http(s)` 绝对地址配置项。
///
/// 三件事必须成立：是合法的绝对 URL、scheme 是 `http`/`https`、有 host。
/// 返回 `Url` 规范化后的形式，因此调用方拿到的字符串可以直接安全拼接：
/// - 尾部不带 `/`。否则 `{base}/s/x.js` 会拼出双斜杠，而路由 `/s/{script_path}`
///   匹配不到双斜杠路径，表现是「音源导入后脚本 404」；子路径前缀会保留；
/// - host 转小写，空格等按标准 percent-encode；
/// - `http:///host`、`http:/host` 这类少斜杠写法会被规范成 `http://host`。
///
/// **为什么 scheme 不能省**：`PUBLIC_BASE_URL` 的消费者全在客户端侧，且都要把它
/// 当绝对地址去请求——① 用户从后台复制 `script_url` 粘进 LX Music 导入自定义源；
/// ② 脚本里的 `const API = ...` 由 LX 的 `request()` 在播放时请求。（后台页面只
/// 展示与复制这个地址、从不解析它，所以浏览器不是失败点。）
/// 缺 scheme 时上面两条路都走不通：`localhost:8787/...` 会被解析成 scheme 为
/// `localhost` 的非 http URL（HTTP 客户端报 ERR_INVALID_PROTOCOL），
/// `127.0.0.1:8787/...`、`example.com/...` 则直接解析失败。
///
/// 注意 `Url::parse("localhost:8787")` **不会**报错——它被解析成
/// `scheme = "localhost"`、`host = None`。所以「能否 parse」不足以拦住漏写
/// `http://` 的配置，scheme 白名单才是关键。
///
/// 证据边界：以上在标准 URL 解析与标准 HTTP 客户端（Node / Chromium 同款 WHATWG
/// 算法）上实测过；**没有**直接对 LX Music 本身验证，涉及 LX 的部分是「它使用
/// 标准 HTTP 客户端」的推断。按此推断实现，但不把它当成已验证事实。
///
/// 也考虑过缺 scheme 时补 `http://` 的宽松做法，未采用：`localhost:8787` 能补对，
/// 但 `kg2lx.example.com:8787`（反代终结 TLS 的常见线上形态）会补错，等于把
/// 「启动即报错」换成「地址能取但取不对」。代价是：只当后台用、从不导入脚本的
/// 部署也会被拒绝启动。
fn parse_http_base_url(raw: &str) -> Result<String, String> {
    let parsed = url::Url::parse(raw.trim())
        .map_err(|err| format!("must be an absolute http(s) URL: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("must use http or https, got scheme {other:?}")),
    }
    if parsed.host_str().is_none() {
        return Err("must include a host".into());
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

/// 读取必填的 `http(s)` 地址配置；缺失或非法都在启动期 panic。
///
/// 刻意不做降级：这些值都会被拼进脚本 URL，错误配置若被带到运行期，
/// 表现是「音源导入后拿不到脚本」这类难排查的远端现象，而不是本机一条清晰的报错。
fn require_http_base_url(key: &str) -> String {
    let raw = env::var(key).unwrap_or_else(|_| panic!("{key} is required"));
    parse_http_base_url(&raw).unwrap_or_else(|err| panic!("{key} is invalid ({raw:?}): {err}"))
}

/// 从绝对 URL 中提取 authority（`host[:port]`）。
///
/// - 去掉 scheme 与 path / query / fragment；
/// - 去掉 userinfo（`user:pass@`），凭据不应出现在脚本头部注释里；
/// - **保留显式端口**：同域名下 `:8787` / `:8788` 多实例是常见调试形态，
///   省掉端口会让它们重新不可区分；
/// - host 按 `Url` 的标准行为归一化为小写；
/// - IPv6 字面量保留方括号（`[::1]:8787`），host 与 port 的边界才无歧义。
///
/// 解析失败返回空串而非 `Err`：调用方只拿它拼一句展示用描述，
/// 「描述里少了实例信息」远好于「脚本因此不可用」。
pub fn authority_of(base_url: &str) -> String {
    let Ok(parsed) = url::Url::parse(base_url.trim()) else {
        return String::new();
    };
    match (parsed.host_str(), parsed.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, _) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_strips_scheme_path_and_userinfo_keeps_port() {
        assert_eq!(
            authority_of("http://user:pass@host:8787/path"),
            "host:8787",
            "userinfo must not leak into the script header"
        );
        assert_eq!(
            authority_of("https://kg2lx.example.com"),
            "kg2lx.example.com"
        );
        assert_eq!(
            authority_of("https://kg2lx.example.com/api/v1?x=1#frag"),
            "kg2lx.example.com"
        );
    }

    /// 非法输入降级为空串，而不是返回半截垃圾。`Config::from_env` 会在启动期
    /// 就拒绝这些值，这里是纵深防御。
    #[test]
    fn authority_degrades_to_empty_for_unparseable_input() {
        for input in ["127.0.0.1:8787", "127.0.0.1:8787/x", "not a url", ""] {
            assert_eq!(
                authority_of(input),
                "",
                "input {input:?} must degrade to empty"
            );
        }
    }

    /// IPv6 字面量必须保留方括号，否则 host 与 port 无法区分。
    #[test]
    fn authority_keeps_ipv6_brackets() {
        assert_eq!(authority_of("http://[::1]:8787/path"), "[::1]:8787");
        assert_eq!(authority_of("http://[::1]"), "[::1]");
    }

    /// 显式端口必须保留：本地多实例（:8787 / :8788）靠它区分。
    #[test]
    fn authority_preserves_explicit_port() {
        assert_eq!(authority_of("http://127.0.0.1:8787"), "127.0.0.1:8787");
        assert_eq!(authority_of("http://127.0.0.1:8788"), "127.0.0.1:8788");
    }

    /// host 归一化为小写，描述文案不会因运维敲了大小写而抖动。
    #[test]
    fn authority_lowercases_host() {
        assert_eq!(
            authority_of("http://Host.Example:8787"),
            "host.example:8787"
        );
    }

    #[test]
    fn http_base_url_normalizes_trailing_slash() {
        assert_eq!(
            parse_http_base_url("http://127.0.0.1:8787").unwrap(),
            "http://127.0.0.1:8787"
        );
        assert_eq!(
            parse_http_base_url("  http://host/  ").unwrap(),
            "http://host"
        );
        assert_eq!(
            parse_http_base_url("http://host/base/").unwrap(),
            "http://host/base",
            "子路径部署要保留路径前缀，只去掉多余的尾随斜杠"
        );
    }

    /// WHATWG 解析器会把少斜杠的写法「修好」：`http:///host` 与 `http:/host`
    /// 都变成 `http://host`。把这个行为钉住——它与「漏写 scheme」不同，
    /// 修正结果符合书写意图，因此接受而不是拒绝。
    #[test]
    fn http_base_url_repairs_missing_slashes() {
        assert_eq!(parse_http_base_url("http:///host").unwrap(), "http://host");
        assert_eq!(parse_http_base_url("http:/host").unwrap(), "http://host");
    }

    /// scheme 与 host 都被归一化，拼出的 URL 是规范形式。
    #[test]
    fn http_base_url_normalizes_scheme_and_host_case() {
        assert_eq!(
            parse_http_base_url("HTTP://Host.Example:8787").unwrap(),
            "http://host.example:8787"
        );
    }

    /// `localhost:8787` 是 `Url::parse` 的陷阱：它被当成 `scheme = "localhost"`
    /// 而**不会报错**，所以必须靠 scheme 白名单拦下来。
    #[test]
    fn http_base_url_rejects_missing_scheme_and_non_http() {
        for input in [
            "localhost:8787",
            "127.0.0.1:8787",
            "//host:8787",
            "ftp://host",
            "not a url",
            "",
            "http://",
        ] {
            assert!(
                parse_http_base_url(input).is_err(),
                "input {input:?} must be rejected at startup"
            );
        }
    }
}
