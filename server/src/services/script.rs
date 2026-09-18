use std::sync::Arc;

use crate::config::Config;
use crate::domain::source::sanitize_script_meta;
use crate::error::AppError;
use crate::repos::account::AccountRepo;
use crate::repos::source::SourceRepo;

/// 脚本契约版本，**刻意与 server 版本解耦**。
///
/// 脚本在架构上等价于客户端，只依赖 `/api/v1/runtime/music-url` 的请求/响应
/// 契约；服务端加功能、重构内部实现都不应让这个版本号跳动。LX 对自定义源
/// 没有自动更新，`@version` 是用户判断「值不值得重新导入」的唯一信号，被
/// 服务端发布节奏带着乱跳会诱导用户做无意义的「删旧 + 重新导入」。
///
/// bump 规则只有三档：
/// 1. `music-url` 请求/响应契约变化 —— 必须 bump，用户不重新导入就无法继续用；
/// 2. 脚本模板实现变化 —— 应 bump，用户重新导入才有收益；
/// 3. 服务端内部变化（路由、存储、日志等） —— **不 bump**。
///
/// 本轮按规则 2：头部元信息重排为 `@name`/`@description`/`@version` 三行，
/// 且 `sources.kg.name` 改为静态平台名，故 `0.1.1` -> `0.1.2`。
const SCRIPT_VERSION: &str = "0.1.2";

pub struct ScriptService {
    config: Arc<Config>,
    source_repo: Arc<dyn SourceRepo>,
    account_repo: Arc<dyn AccountRepo>,
}

impl ScriptService {
    pub fn new(
        config: Arc<Config>,
        source_repo: Arc<dyn SourceRepo>,
        account_repo: Arc<dyn AccountRepo>,
    ) -> Self {
        Self {
            config,
            source_repo,
            account_repo,
        }
    }

    pub async fn render_script_by_token(
        &self,
        script_token: &str,
    ) -> Result<(axum::http::HeaderMap, String), AppError> {
        let source = self
            .source_repo
            .find_by_script_token(script_token)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        if !source.enabled {
            return Err(AppError::source_disabled());
        }

        // 严禁从 `Source.name` 反推 userid：`name` 自本轮起可被用户自定义，
        // 该推导既循环又错误，因此这里显式查账号。
        //
        // 元信息整体是「尽力而为」：账号查询失败只降级 `@description`
        // （退化为只写 authority）并记一条 warn，绝不上抛错误——元信息的
        // 任何异常都不该让 `/s/{token}.js` 不可用。
        let userid = match self.account_repo.find_by_id(&source.account_id).await {
            Ok(Some(account)) => account.upstream_userid,
            Ok(None) => {
                tracing::warn!(
                    account_id = %source.account_id,
                    reason = "account record not found",
                    "script metadata degraded: description will omit userid",
                );
                String::new()
            }
            Err(err) => {
                tracing::warn!(
                    account_id = %source.account_id,
                    error = %err,
                    "script metadata degraded: description will omit userid",
                );
                String::new()
            }
        };

        let script = render_script(
            &self.config.public_base_url,
            &source.runtime_token,
            &source.name,
            &self.config.public_authority(),
            &userid,
            source.provider.display_name(),
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            "application/javascript; charset=utf-8".parse().unwrap(),
        );
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            "no-store".parse().unwrap(),
        );

        Ok((headers, script))
    }
}

/// 渲染脚本正文的纯函数：不碰 IO、不读环境变量，因此可以直接单测。
///
/// 渲染路径**只清洗、永不拒绝、不做截断**：否则一个超长的
/// `SOURCE_NAME_PREFIX` 会让脚本 URL 直接不可用。
fn render_script(
    public_base_url: &str,
    runtime_token: &str,
    source_name: &str,
    authority: &str,
    userid: &str,
    platform_name: &str,
) -> String {
    let name = sanitize_script_meta(source_name);
    let description = render_description(authority, userid);

    format!(
        r#"/**
 * @name {name}
 * @description {description}
 * @version {SCRIPT_VERSION}
 */
// kg2lx 专属酷狗概念版音源脚本，仅供通过 kg2lx 解析歌曲链接
const {{ EVENT_NAMES, request, on, send }} = globalThis.lx
const API = '{public_base_url}/api/v1/runtime/music-url'
const TOKEN = '{runtime_token}'
const qualityMap = {{ '128k': '128k', '320k': '320k', flac: 'flac', flac24bit: 'flac24bit' }}
const httpPost = (url, body) => new Promise((resolve, reject) => {{
  request(url, {{
    method: 'POST',
    headers: {{
      'content-type': 'application/json',
      authorization: `Bearer ${{TOKEN}}`,
    }},
    body: body,
  }}, (err, resp) => {{
    if (err) return reject(err)
    try {{
      const data = typeof resp.body === 'string' ? JSON.parse(resp.body) : (resp.body || {{}})
      if (resp.statusCode && resp.statusCode >= 400) return reject(new Error(data.message || `request failed: ${{resp.statusCode}}`))
      if (!data.url) return reject(new Error('empty url'))
      resolve(data.url)
    }} catch (e) {{
      reject(e)
    }}
  }})
}})
on(EVENT_NAMES.request, ({{ source, action, info }}) => {{
  if (source !== 'kg' || action !== 'musicUrl') return Promise.reject(new Error('unsupported action'))
  const musicInfo = info.musicInfo || {{}}
  const quality = qualityMap[info.type]
  if (!quality) return Promise.reject(new Error(`unsupported quality: ${{info.type}}`))
  return httpPost(API, {{
    hash: musicInfo.hash,
    album_audio_id: musicInfo.album_audio_id || musicInfo.albumAudioId || null,
    quality,
  }})
}})
send(EVENT_NAMES.inited, {{
  openDevTools: false,
  sources: {{
    kg: {{
      name: '{platform_name}',
      type: 'music',
      actions: ['musicUrl'],
      qualitys: ['128k', '320k', 'flac', 'flac24bit'],
    }},
  }},
}})
"#
    )
}

/// `@description` 由服务端独占，形如 `<authority> · <userid>`。
///
/// 它是「这条音源来自哪台服务、哪个账号」的兜底事实：用户可以把标题改得
/// 难以分辨，但仍能靠描述确认真实身份。刻意不写「本地」这类环境标签——
/// `127.0.0.1:8787` 本身已蕴含环境语义，重复是浪费 36 字符的建议预算。
///
/// `userid` 缺失时降级为只写 authority，不产生尾随分隔符。
fn render_description(authority: &str, userid: &str) -> String {
    let authority = authority.trim();
    let userid = userid.trim();
    let raw = match (authority.is_empty(), userid.is_empty()) {
        (true, true) => String::new(),
        (true, false) => userid.to_string(),
        (false, true) => authority.to_string(),
        (false, false) => format!("{authority} · {userid}"),
    };
    sanitize_script_meta(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_script() -> String {
        render_script(
            "http://127.0.0.1:8787",
            "rt_test",
            "酷狗概念版 12345678",
            "127.0.0.1:8787",
            "12345678",
            "酷狗概念版",
        )
    }

    fn header_lines(script: &str) -> Vec<&str> {
        script
            .lines()
            .skip(1)
            .take_while(|l| !l.trim_start().starts_with("*/"))
            .collect()
    }

    /// 头部三行元信息必须按 @name / @description / @version 排列，
    /// 且 @name 只有一行——换行会破坏注释结构与元信息解析。
    #[test]
    fn header_has_three_metadata_lines() {
        let script = sample_script();
        let lines = header_lines(&script);
        assert_eq!(
            lines,
            vec![
                " * @name 酷狗概念版 12345678",
                " * @description 127.0.0.1:8787 · 12345678",
                " * @version 0.1.2",
            ],
            "header metadata layout changed; update this test deliberately"
        );
    }

    /// `*/` 会提前闭合注释块，导入即语法错误，因此标题必须被清洗到
    /// 不含它；同时不能因为清洗而让 @name 变成多行。
    #[test]
    fn name_is_sanitized_and_single_line() {
        let script = render_script(
            "http://x",
            "rt",
            "evil*/ name\nsecond",
            "x",
            "1",
            "酷狗概念版",
        );
        let name_lines: Vec<&str> = script
            .lines()
            .filter(|l| l.trim_start().starts_with("* @name"))
            .collect();
        assert_eq!(name_lines.len(), 1, "@name must occupy exactly one line");
        assert!(
            !name_lines[0].contains("*/"),
            "sanitized @name must not close the comment block early"
        );
        assert_eq!(name_lines[0], " * @name evil name second");
    }

    /// 描述是实例兜底事实：必须同时给出 authority 与 userid，显式端口不能被省。
    #[test]
    fn description_contains_authority_and_userid() {
        assert_eq!(
            render_description("127.0.0.1:8787", "12345678"),
            "127.0.0.1:8787 · 12345678"
        );
    }

    /// userid 缺失时降级为只含 authority，不能留下孤零零的分隔符。
    #[test]
    fn description_falls_back_to_authority_only() {
        let script = render_script("http://x", "rt", "n", "127.0.0.1:8787", "", "酷狗概念版");
        assert!(
            script.contains(" * @description 127.0.0.1:8787\n"),
            "missing userid must degrade to authority-only description"
        );
        assert!(!script.contains("·"), "no separator when userid is absent");
    }

    /// `sources.kg.name` 是平台级显示名，必须与可自定义的头部 @name 不同步。
    #[test]
    fn sources_name_is_platform_level_not_instance_title() {
        let script = render_script(
            "http://x",
            "rt",
            "本地调试 12345678",
            "x",
            "1",
            "酷狗概念版",
        );
        assert!(
            script.contains("      name: '酷狗概念版',"),
            "sources.kg.name must be the fixed platform name"
        );
        assert!(
            !script.contains("name: '本地调试 12345678'"),
            "instance title must never leak into sources[].name"
        );
    }

    /// 免责声明已从 @description 下沉到脚本体注释，不能占掉描述的建议预算。
    #[test]
    fn disclaimer_moved_into_body() {
        let script = sample_script();
        assert!(script.contains("// kg2lx 专属酷狗概念版音源脚本"));
    }
}
