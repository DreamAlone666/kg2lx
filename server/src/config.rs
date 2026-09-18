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
    pub web_dist_dir: Option<String>,
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
                .unwrap_or_else(|_| "酷狗概念版".into()),
            refresh_interval_secs: env::var("REFRESH_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(6 * 3600),
            web_dist_dir: env::var("WEB_DIST_DIR").ok(),
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

/// 从绝对 URL 中提取 authority（`host[:port]`）。
///
/// 规则与理由：
/// - 去掉 scheme（任意 `xxx://`）与 path / query / fragment；
/// - 去掉 userinfo（`user:pass@`），凭据不应出现在脚本头部注释里；
/// - **保留显式端口**：同域名下 `:8787` / `:8788` 多实例是常见调试形态，
///   省掉端口会让它们重新不可区分；
/// - IPv6 字面量保留方括号（`[::1]:8787`），host 与 port 的边界才无歧义。
///
/// 无 scheme 的输入按纯 authority 处理，因此 `127.0.0.1:8787/x` 也能得到
/// `127.0.0.1:8787`。这是纯函数、不读环境变量，因此可直接单测
/// （`Config::from_env()` 在缺环境变量时会 panic，不适合做测试入口）。
///
/// 这里刻意手写而不引入 `url` crate。注意理由**不是**「省一个依赖」：
/// `url` 已经是 `reqwest` 的传递依赖，声明它不会新增任何编译单元。真实理由是
/// 两者的失败模式不同——`Url::parse` 对无 scheme 输入直接报错，而调用方只把
/// `PUBLIC_BASE_URL` 当字符串拼接使用、从不校验，所以这里需要的是「尽力提取」
/// 而不是「严格解析 + 额外降级分支」。若将来要严格校验该配置，应改用
/// `url::Url::parse` 并在 `Config::from_env()` 里一次性报错，而不是在这里补。
pub fn authority_of(base_url: &str) -> String {
    let s = base_url.trim();
    // 只认 `scheme://`，避免把 IPv6 或 host:port 里的冒号误当分隔符。
    let s = match s.find("://") {
        Some(idx) => &s[idx + 3..],
        None => s,
    };
    let end = s.find(['/', '?', '#']).unwrap_or(s.len());
    let s = &s[..end];
    // userinfo 里不会出现裸 `@`，取最后一个即可正确切分。
    let s = match s.rfind('@') {
        Some(idx) => &s[idx + 1..],
        None => s,
    };
    s.to_string()
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
        assert_eq!(authority_of("https://kg2lx.example.com"), "kg2lx.example.com");
        assert_eq!(
            authority_of("https://kg2lx.example.com/api/v1?x=1#frag"),
            "kg2lx.example.com"
        );
    }

    /// 无 scheme 的输入按纯 authority 处理。
    #[test]
    fn authority_accepts_scheme_less_input() {
        assert_eq!(authority_of("127.0.0.1:8787"), "127.0.0.1:8787");
        assert_eq!(authority_of("127.0.0.1:8787/x"), "127.0.0.1:8787");
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
}
