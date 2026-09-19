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
/// 本轮按规则 2：脚本开始转发 LX 提供的歌曲元数据（见 `render_script` 里
/// 以「歌曲元数据的字段名来自…」开头的那段注释），用户重新导入后日志面板才会
/// 出现歌名，故 `0.1.2` -> `0.1.3`。此前那一轮的 `0.1.1` -> `0.1.2` 是头部
/// 元信息重排与 `sources.kg.name` 改为静态平台名。
const SCRIPT_VERSION: &str = "0.1.3";

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
/// `Source.name` 会让脚本 URL 直接不可用。
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
    // 插进 JS 代码里的值一律走 JSON 字符串字面量，而不是手写单引号拼接：
    // JSON 的字符串语法是 JS 字符串字面量的子集，`serde_json` 会正确转义
    // 引号、反斜杠与控制字符。手写 `'...'` 时，只要 `PUBLIC_BASE_URL` 里出现
    // 一个 `'`（或 token 里出现 `"`），整段脚本就是语法错误、导入即失败。
    // 注意上面两个 `@name`/`@description` 不需要转义：它们在注释里，且已被
    // `sanitize_script_meta` 去掉唯一能闭合注释的 `*/`。
    //
    // 歌曲元数据的字段名来自 LX 在脚本边界上的**扁平**音乐对象，不是 LX 内部的
    // `MusicInfo`（那个是嵌套的 `meta.albumName`）。映射链是
    // `core/music/utils.ts` 的 `getMusicUrl(toOldMusicInfo(musicInfo), quality)`
    // → `common/utils/tools.ts` 的 `toOldMusicInfo`，它产出
    // `{ name, singer, albumName, hash, albumAudioId, ... }`：歌名/歌手/专辑是
    // 顶层 `name`/`singer`/`albumName`，酷狗标识是 `hash` 与 **camelCase** 的
    // `albumAudioId`。
    //
    // 这几个字段此前一个都没发，后端的 `track_title` / `artist_name` /
    // `album_name` 于是从未落过盘（实测 20 行窗口跨 2026-05 ~ 2026-09 全是 null），
    // 日志面板只能退化成拿 hash 当行身份。信息是 LX 免费给的，不该由我们反查上游。
    //
    // 取值一律 `|| null`：LX 对缺失字段可能给 `undefined` 或空串，而空串会让后端
    // 落盘成「有元数据但是空标题」，前端 `{#if log.track_title}` 的分支判断与
    // 「这一行没有元数据」就再也分不开了。统一归一化成 `null`。
    let api_literal = js_string_literal(&format!("{public_base_url}/api/v1/runtime/music-url"));
    let token_literal = js_string_literal(runtime_token);
    let platform_literal = js_string_literal(platform_name);

    format!(
        r#"/**
 * @name {name}
 * @description {description}
 * @version {SCRIPT_VERSION}
 */
// kg2lx 专属酷狗概念版音源脚本，仅供通过 kg2lx 解析歌曲链接
const {{ EVENT_NAMES, request, on, send }} = globalThis.lx
const API = {api_literal}
const TOKEN = {token_literal}
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
    album_audio_id: musicInfo.albumAudioId || null,
    quality,
    track_title: musicInfo.name || null,
    artist_name: musicInfo.singer || null,
    album_name: musicInfo.albumName || null,
  }})
}})
send(EVENT_NAMES.inited, {{
  openDevTools: false,
  sources: {{
    kg: {{
      name: {platform_literal},
      type: 'music',
      actions: ['musicUrl'],
      qualitys: ['128k', '320k', 'flac', 'flac24bit'],
    }},
  }},
}})
"#
    )
}

/// 把一个值渲染成可直接嵌入 JS 源码的字符串字面量（双引号，内容已转义）。
///
/// JSON 字符串语法是 JS 字符串字面量的子集，因此 `serde_json` 的输出可以直接
/// 当 JS 字面量用。对 `&str` 而言序列化不会失败。
fn js_string_literal(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a &str to JSON cannot fail")
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
                " * @version 0.1.3",
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
            script.contains("      name: \"酷狗概念版\","),
            "sources.kg.name must be the fixed platform name"
        );
        assert!(
            !script.contains("name: \"本地调试 12345678\""),
            "instance title must never leak into sources[].name"
        );
    }

    /// 插入 JS 代码的值必须是转义过的字符串字面量。`PUBLIC_BASE_URL` 里出现
    /// `'`、或 token 里出现 `"` / `\` 时，手写单引号拼接会让整段脚本变成
    /// 语法错误（导入即失败），而不是只影响显示。
    #[test]
    fn interpolated_values_are_escaped_js_string_literals() {
        let script = render_script(
            "http://host/a'b",
            "rt_\"x\\y",
            "标题",
            "host",
            "1",
            "酷狗概念版",
        );
        assert!(
            script.contains(r#"const API = "http://host/a'b/api/v1/runtime/music-url""#),
            "base url must be a double-quoted literal so an apostrophe cannot escape"
        );
        assert!(
            script.contains(r#"const TOKEN = "rt_\"x\\y""#),
            "quote and backslash in the runtime token must be escaped"
        );
        assert!(
            script.contains("      name: \"酷狗概念版\","),
            "platform name must be a string literal too"
        );
    }

    /// 免责声明已从 @description 下沉到脚本体注释，不能占掉描述的建议预算。
    #[test]
    fn disclaimer_moved_into_body() {
        let script = sample_script();
        assert!(script.contains("// kg2lx 专属酷狗概念版音源脚本"));
    }

    /// `httpPost(API, { ... })` 的请求体，压平空白后返回。
    ///
    /// 只取这一段而不是对整个脚本做 `contains`：后者无法发现「字段接错线」——
    /// 把 `artist_name` 也接到 `musicInfo.name` 上时，两个 `contains` 都成立。
    fn request_body(script: &str) -> String {
        const MARKER: &str = "return httpPost(API, {";
        let start = script
            .find(MARKER)
            .expect("script must build the music-url request body inline");
        let rest = &script[start + MARKER.len()..];
        let end = rest
            .find("})")
            .expect("the request body object must be closed");
        rest[..end].split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// 请求体的字段映射必须逐字钉死。
    ///
    /// 这不是格式洁癖：LX 在脚本边界给出的音乐对象是**扁平**的
    /// （`toOldMusicInfo` 产出 `name` / `singer` / `albumName`），而契约字段是
    /// 蛇形的 `track_title` / `artist_name` / `album_name`。两侧的名字没有任何
    /// 编译器能对齐，唯一的保障就是这条断言——映射被改动时它必须报错，让人
    /// 停下来确认是故意改的。
    #[test]
    fn request_body_maps_lx_music_info_fields() {
        let script = sample_script();
        assert_eq!(
            request_body(&script),
            concat!(
                "hash: musicInfo.hash, ",
                "album_audio_id: musicInfo.albumAudioId || null, ",
                "quality, ",
                "track_title: musicInfo.name || null, ",
                "artist_name: musicInfo.singer || null, ",
                "album_name: musicInfo.albumName || null,"
            ),
            "request body mapping changed; update this test deliberately"
        );
    }

    /// `album_audio_id` 只认 LX 的驼峰 `albumAudioId`。
    ///
    /// 此前写的是 `musicInfo.album_audio_id || musicInfo.albumAudioId`，而
    /// `toOldMusicInfo` 里根本没有蛇形那一个——前半个分支恒为假，是一段永远
    /// 不会命中的兜底。留着它会让下一个人以为两种命名都可能出现。
    #[test]
    fn album_audio_id_has_no_snake_case_alias() {
        let script = sample_script();
        assert!(
            !script.contains("musicInfo.album_audio_id"),
            "LX never sends an `album_audio_id` field on the script boundary"
        );
    }
}
