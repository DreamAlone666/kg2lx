use std::sync::Arc;

use crate::config::Config;
use crate::domain::source::sanitize_script_meta;
use crate::error::{AppError, ErrorCode};
use crate::repos::account::AccountRepo;
use crate::repos::source::SourceRepo;

/// 脚本契约版本，**刻意与 server 版本解耦**。
///
/// 脚本在架构上等价于客户端，只依赖 `/api/v1/runtime/music-url` 的请求/响应
/// 契约；服务端加功能、重构内部实现都不应让这个版本号跳动。LX 对自定义源
/// 没有自动更新，`@version` 是用户判断「值不值得重新导入」的唯一信号，被
/// 服务端发布节奏带着乱跳会诱导用户做无意义的「删旧 + 重新导入」。
///
/// bump 规则的**锚点是上一次发行的脚本版本**，不是当前工作树里的值：
/// 1. 相对上次发行，`music-url` 请求/响应契约变了 —— 必须 bump，用户不重新导入
///    就无法继续用；
/// 2. 相对上次发行，脚本模板实现变了 —— 应 bump，用户重新导入才有收益；
/// 3. 相对上次发行，脚本没有变（只有服务端内部改动）—— **不 bump**。
///
/// **未发行窗口内不累积**：一连串都还没发出去的改动，合计只对应一个号。这不是
/// 洁癖——中间那些号没有任何用户见过，而 `@version` 的全部作用就是回答「我手上
/// 冻结的那个号与现在这个号等不等」；不等就够了，多出来的中间值零信息量。逐次
/// 累积等于凭空造出一串谁也没拥有过的号，而它们一旦随发行发出去，诱导的效果与
/// 「版本号乱跳」完全一样。
///
/// 当前取值 `0.1.2` 的来历：`v0.1.3` 发行时带的脚本版本是 `0.1.1`。此后至今的
/// 脚本改动（头部元信息重排与 `sources.kg.name` 静态化、转发 LX 提供的歌曲
/// 元数据、自带错误文案字典并自控请求超时与返回地址校验）全部**尚未发行**，
/// 故合计只 +1。
///
/// 任一版都必须靠**删除音源后重新导入**才能在用户侧生效——洛雪音乐从不自动更新
/// 已导入的脚本。服务端无法代劳。
const SCRIPT_VERSION: &str = "0.1.2";

/// 脚本自研错误的前缀，与字典一起构成用户在播放条上能看到的那一行。
///
/// **Why:** 洛雪音乐的播放条只渲染一行省略号文本，且在最常见的那条失败路径上
/// 会把脚本给的 `err.message` 整个换成它自己写死的「换源失败…」句子
/// （`src/renderer/core/music/utils.ts:315-333` 的 `onToggleSource()` 分支）。
/// 前缀是我们在那一行开头唯一能留下的署名：用户至少能分辨这条消息来自 kg2lx
/// 还是来自别的音源。
const SCRIPT_ERROR_PREFIX: &str = "kg2lx：";

/// 用户可见文案的唯一真源：错误码 → 中文短句。
///
/// **Why 必须放在脚本里：** 洛雪音乐在脚本边界上只把信封里的 `message` 交给
/// 脚本，随后客户端会用一句自己写死的通用句子把它顶掉；我们能控制的只有「短、
/// 把要用户做的那件事前置、能看出是谁说的」。字典若留在服务端，等于让播放条上
/// 的一句话多绕一次网络往返，而它要挤进的那一行只有大约 15~25 个汉字。
///
/// 值里**不带** `SCRIPT_ERROR_PREFIX`：拼接由脚本里的 `fail()` 独占完成，字典
/// 再写一次就会重复。
///
/// `SOURCE_NAME_EMPTY` / `SOURCE_NAME_TOO_LONG` / `LOGIN_SESSION_NOT_FOUND` 是
/// 管理面（后台改名、管理接口）的错误码，运行时链路上不可能出现。它们仍然列在
/// 这里，是因为这张表的价值在于**穷尽**：穷尽性正是
/// `dictionary_covers_every_error_code` 能成立的前提，而任何一个漏掉的码都会让
/// 脚本回退到后端的英文诊断文本——那才是要杜绝的事。
const SCRIPT_ERROR_MESSAGES: &[(ErrorCode, &str)] = &[
    (ErrorCode::AdminUnauthorized, "脚本令牌无效，请重新导入"),
    (ErrorCode::SourceNotFound, "音源不存在，请重新导入音源"),
    (ErrorCode::SourceDisabled, "音源已停用，请在后台启用"),
    (ErrorCode::AccountNotFound, "账号不存在，请重新绑定"),
    (ErrorCode::AccountDisabled, "账号已停用，请在后台启用"),
    (ErrorCode::AccountNotVip, "账号 VIP 已失效，请续费"),
    (ErrorCode::LoginSessionNotFound, "登录已过期，请重新扫码"),
    (ErrorCode::UpstreamRequestFailed, "连接酷狗失败，请稍后重试"),
    (ErrorCode::UpstreamLoginFailed, "登录态失效，请在后台刷新"),
    (
        ErrorCode::UpstreamVipCheckFailed,
        "VIP 校验失败，请稍后重试",
    ),
    (ErrorCode::UpstreamPlayUrlEmpty, "该歌曲暂无可用音质"),
    (ErrorCode::UpstreamRateLimited, "酷狗繁忙，请稍后重试"),
    (ErrorCode::InvalidRequest, "请求参数有误，请重新导入"),
    (ErrorCode::SourceNameEmpty, "音源名称为空，请在后台修改"),
    (ErrorCode::SourceNameTooLong, "音源名称过长，请在后台修改"),
    (ErrorCode::InternalError, "服务内部错误，请稍后重试"),
];

/// 字典里查不到的 `code` 的兜底文案，同样不含前缀。
///
/// 它存在的意义是保证「永远不回退到后端文本」这条规则没有缺口：后端确实可能
/// 返回字典里没有的码（老脚本遇到新码，或信封本身被中间件改写）。
const SCRIPT_UNKNOWN_ERROR: &str = "服务异常，请稍后重试";

/// 前缀 + 文案的字符上限。
///
/// **Why 24：** 播放条上大约能看到 15~25 个汉字，而 `kg2lx：` 要占掉其中 3~4 个。
/// 这个上限故意留得保守——它比「实际能显示」更严格，于是字典里的每一条都
/// **永远不会被截断**。反例就是洛雪音乐自己那句「换源失败，请尝试手动在搜索页指定
/// 其他来源搜索该歌曲播放」：27 个字符（26 汉字 + 1 个全角逗号，2026-09-20 在
/// master `ad95d50` 上实测），超出可视宽度的部分被省略号吃掉，用户看到的是半句话。
const SCRIPT_ERROR_MAX_CHARS: usize = 24;

/// 脚本自己给 `request()` 设的超时，单位毫秒。
///
/// **Why 15 秒：** 宿主自己会在 20 秒时硬超时并把请求以字面量 `Cancel request`
/// 拒绝（`src/main/modules/userApi/rendererEvent/rendererEvent.ts:119,137-139`）。
/// 那句英文既不等于宿主本地化的取消常量，也不经过任何翻译，会直接显示在播放条
/// 上。我们要抢在它前面失败，好让用户看到中文原因，因此留出约 5 秒余量。
const SCRIPT_REQUEST_TIMEOUT_MS: u64 = 15_000;

/// 播放地址的本地护栏，与宿主的判断保持一致。
///
/// 宿主的 `preload.js` 会以字面量 `failed` 拒绝「不是字符串 / 不匹配 `/^https?:/`
/// / 超过 2048 字符」的 `musicUrl`。先在这里拦下，用户看到的才是中文原因而不是
/// 那个 untranslated 的 `failed`。
const SCRIPT_MAX_URL_LEN: usize = 2048;

/// 与宿主逐字相等的重试信号。
///
/// 宿主用 `===` 比较 `'服务器繁忙'`：字面量定义在
/// `src/renderer/utils/message.ts:8`，比较在 `src/renderer/core/player/action.ts:120`。
/// 紧邻的 `:118` 比的是另一个常量（`'取消http请求'`，`message.ts:7`），撞上它会被
/// 当成用户主动取消而静默——两者相差一行，引用时别指错。命中后会等 2~6 秒再重新
/// 发起一次取链；它的好处是这条分支在
/// `src/renderer/core/music/utils.ts:317` 就短路了，能绕开那句会把责任推给用户
/// 的「换源失败」兜底。因此它值得在**真·瞬时错误**上借用，但每次借用都让用户
/// 多等 2~6 秒，必须有次数上限。
const SCRIPT_BUSY_MESSAGE: &str = "服务器繁忙";

/// 借用宿主重试信号的次数上限：同一音源连续失败到第三次之后就不再借。
const SCRIPT_BUSY_RETRY_LIMIT: u8 = 3;

/// 用尽上限后的冷却窗口，单位毫秒。
///
/// **Why 需要它：** 计数会在一次非瞬时失败（例如某首歌取不到链接）后被清零，
/// 于是紧接着的限流又会重新借到重试信号。冷却窗口是第二道闸：窗口内无论计数
/// 是多少，都直接给用户可读消息，不再让宿主静默重试。
const SCRIPT_BUSY_COOLDOWN_MS: u64 = 60_000;

/// 把字典渲染成一段 JS 对象字面量。
///
/// 键取 `ErrorCode::as_str()`（服务端线上字符串，即信封里的 `code`），而不是
/// Rust 变体名——脚本查的是信封的字段，写成变体名的话这张表在运行时永远命中
/// 不了，所有错误都会掉进兜底文案。键与值都走 `js_string_literal`，与脚本里其他
/// 插值保持同一套转义规则。
fn render_error_messages() -> String {
    let entries: Vec<String> = SCRIPT_ERROR_MESSAGES
        .iter()
        .map(|(code, message)| {
            format!(
                "  {}: {},",
                js_string_literal(code.as_str()),
                js_string_literal(message)
            )
        })
        .collect();
    format!("{{\n{}\n}}", entries.join("\n"))
}

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
    //
    // 用户可见文案同理必须由服务端渲染进脚本，不能指望运行时再问一次后端：
    // 宿主的失败渲染路径会把我们的文本换掉，我们唯一能控制的就是「在脚本里
    // 就写好、短到能装进播放条一行」的那句话（见 `SCRIPT_ERROR_MESSAGES`）。
    // 字典的键是线上 code，所以服务端加了新码、脚本不重新导入时，用户看到的是
    // 兜底文案而不是英文诊断文本。
    let api_literal = js_string_literal(&format!("{public_base_url}/api/v1/runtime/music-url"));
    let token_literal = js_string_literal(runtime_token);
    let platform_literal = js_string_literal(platform_name);
    let error_prefix_literal = js_string_literal(SCRIPT_ERROR_PREFIX);
    let error_messages = render_error_messages();
    let unknown_error_literal = js_string_literal(SCRIPT_UNKNOWN_ERROR);
    let busy_message_literal = js_string_literal(SCRIPT_BUSY_MESSAGE);

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
// 错误码 -> 用户文案。键是后端信封里的 code（SCREAMING_SNAKE_CASE），由服务端
// 从 ErrorCode::as_str() 生成，两侧不会各写一份字符串。
const ERROR_PREFIX = {error_prefix_literal}
const ERROR_MESSAGES = {error_messages}
const UNKNOWN_ERROR = {unknown_error_literal}
const MAX_ERROR_CHARS = {SCRIPT_ERROR_MAX_CHARS}
const MAX_URL_LEN = {SCRIPT_MAX_URL_LEN}
const REQUEST_TIMEOUT_MS = {SCRIPT_REQUEST_TIMEOUT_MS}
const BUSY_MESSAGE = {busy_message_literal}
const BUSY_RETRY_LIMIT = {SCRIPT_BUSY_RETRY_LIMIT}
const BUSY_COOLDOWN_MS = {SCRIPT_BUSY_COOLDOWN_MS}
// 自研错误的唯一构造入口：前缀不会被漏掉，长度上限也不会被绕过。
const fail = (m) => new Error(ERROR_PREFIX + String(m).slice(0, MAX_ERROR_CHARS))
// 响应体解析：任何失败都返回 null，绝不把引擎文案抛出去。反向代理在 502 时会
// 回一整个 HTML 页，JSON.parse 抛出的 V8 SyntaxError 会被宿主原样显示——对用户
// 那是一句没有意义的英文。
const parseBody = (body) => {{
  if (body && typeof body === 'object') return body
  if (typeof body !== 'string') return null
  try {{
    const parsed = JSON.parse(body)
    return parsed && typeof parsed === 'object' ? parsed : null
  }} catch (e) {{
    return null
  }}
}}
// 播放地址护栏，与宿主 preload.js 的判断一致（非字符串 / 非 http(s) / 超过
// 2048 字符都会被它用字面量 failed 拒绝）。先在这里拦下，用户看到的中文原因
// 才不会被那句 failed 顶掉。
const isPlayableUrl = (u) =>
  typeof u === 'string' && u.length <= MAX_URL_LEN && /^https?:/.test(u)
const httpPost = (url, body) => new Promise((resolve, reject) => {{
  request(url, {{
    method: 'POST',
    headers: {{
      'content-type': 'application/json',
      authorization: `Bearer ${{TOKEN}}`,
    }},
    body: body,
    // 单位是毫秒。宿主自己会在 20 秒时硬超时并抛出英文的 'Cancel request'，
    // 我们要抢在它前面用自己的消息失败。
    timeout: REQUEST_TIMEOUT_MS,
  }}, (err, resp) => {{
    // 传输层失败（连不上服务端 / 被宿主的超时打断）：只给固定短句，绝不透出
    // err.message——那里是 ECONNREFUSED、证书错误一类的引擎文本。
    if (err) return reject(fail(UNKNOWN_ERROR))
    // 这个回调抛出的异常不会被 promise 捕获（它已经离开了 executor），会让
    // promise 永不 settle、最后退化成宿主的英文超时。所以响应对象一律先判空。
    const statusCode = resp && resp.statusCode
    const data = parseBody(resp && resp.body)
    if (!data) return reject(fail(UNKNOWN_ERROR))
    if (statusCode && statusCode >= 400) {{
      // 只用 code 查本地字典：后端的 message 是写给服务端日志的诊断散文，
      // 不是给用户看的文案，绝不能显示。
      const e = fail(ERROR_MESSAGES[data.code] || UNKNOWN_ERROR)
      // 该不该重试由服务端判定（脚本导入后不会自动更新，判定写在脚本里就无法
      // 随服务端演进），这里只是把标记原样传给上层的 catch。
      e.retryable = data.retryable === true
      return reject(e)
    }}
    if (!isPlayableUrl(data.url)) return reject(fail(ERROR_MESSAGES.UPSTREAM_PLAY_URL_EMPTY))
    resolve(data.url)
  }})
}})
// busyStreak / busyCooldownUntil 是模块级状态，因此在音源被导入后的整个生命周期
// 内**跨歌曲**共享：换一首歌不会清零。这是刻意的权衡——按 hash 建一张 map 能让
// 冷却更精准，但那要引入一个会持续增长的表和它的淘汰规则，而这里要挡的只是
// 「宿主把一次失败放大成无界重试」，一份全局计数已经够用。代价是同一音源在窗口期内
// 播另一首歌时，也会直接拿到可读消息而不是先重试一次。
let busyStreak = 0
let busyCooldownUntil = 0
on(EVENT_NAMES.request, ({{ source, action, info }}) => {{
  if (source !== 'kg' || action !== 'musicUrl') return Promise.reject(fail(ERROR_MESSAGES.INVALID_REQUEST))
  const musicInfo = info.musicInfo || {{}}
  const quality = qualityMap[info.type]
  if (!quality) return Promise.reject(fail(ERROR_MESSAGES.INVALID_REQUEST))
  return httpPost(API, {{
    hash: musicInfo.hash,
    album_audio_id: musicInfo.albumAudioId || null,
    quality,
    track_title: musicInfo.name || null,
    artist_name: musicInfo.singer || null,
    album_name: musicInfo.albumName || null,
  }}).catch((err) => {{
    // 服务端没有说这是瞬时错误：清零计数（一次失败不代表后面都会失败）并原样抛出。
    if (!err || err.retryable !== true) {{
      busyStreak = 0
      throw err
    }}
    // 只有服务端声明的瞬时错误才借宿主的重试信号，而且有上限。宿主收到信号后会等
    // 2~6 秒再发起一次请求（player/action.ts 的 delayRetry），每一次借用都意味着
    // 用户多等这么久，无限借用就成了一条永远不结束的重试环。
    // 冷却窗口是第二道闸：计数可能被上面那次非瞬时失败清零，窗口内仍然不借。
    if (Date.now() < busyCooldownUntil || busyStreak >= BUSY_RETRY_LIMIT) {{
      busyStreak = BUSY_RETRY_LIMIT
      busyCooldownUntil = Date.now() + BUSY_COOLDOWN_MS
      throw fail(ERROR_MESSAGES.UPSTREAM_RATE_LIMITED)
    }}
    busyStreak += 1
    // 逐字相等是必须的：宿主用 === 比较这个字符串，带上前缀就不再是它认得的那个
    // 信号，会掉回「换源失败」那条误导路径。
    // 这里刻意不清零计数：宿主收到信号后会静默重试，清零等于每轮都重新发放额度。
    throw new Error(BUSY_MESSAGE)
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

    /// 字典必须**恰好**覆盖每一个错误码。
    ///
    /// 少一个：该码在运行时回退成后端的英文诊断文本，而这正是本轮要消灭的东西。
    /// 多一个：表里有一行永远不会被命中的文案，读代码的人会以为那个码还会从
    /// 运行时冒出来。
    #[test]
    fn dictionary_covers_every_error_code() {
        assert_eq!(
            SCRIPT_ERROR_MESSAGES.len(),
            ErrorCode::ALL.len(),
            "字典条数与 ErrorCode::ALL 不一致"
        );
        for code in ErrorCode::ALL {
            assert!(
                SCRIPT_ERROR_MESSAGES.iter().any(|(c, _)| *c == code),
                "{code:?} 在字典里没有对应文案"
            );
        }
        // 键必须是线上字符串（SCREAMING_SNAKE_CASE）。写成 Rust 变体名的话，
        // 这张表在运行时一次都命中不了，所有错误都会掉进兜底文案。
        let script = sample_script();
        assert!(
            script.contains(&format!(
                "{}:",
                js_string_literal(ErrorCode::UpstreamRateLimited.as_str())
            )),
            "字典的键必须是信封里的线上 code"
        );
    }

    /// 前缀 + 文案必须装得下播放条那一行。
    ///
    /// Rust 的 `chars()` 数的是 Unicode 标量值，JS 的 `slice` 数的是 UTF-16 码元；
    /// 对中文与全角标点，两者逐个相等，所以这条断言同时钉住了脚本侧 `slice` 的
    /// 截断语义——只要它成立，字典里没有任何一条会被截断。
    #[test]
    fn dictionary_messages_fit_the_play_bar() {
        for (code, message) in SCRIPT_ERROR_MESSAGES {
            let rendered = format!("{SCRIPT_ERROR_PREFIX}{message}");
            assert!(
                rendered.chars().count() <= SCRIPT_ERROR_MAX_CHARS,
                "{code:?} 的文案「{rendered}」有 {} 字符，超过上限 {}",
                rendered.chars().count(),
                SCRIPT_ERROR_MAX_CHARS
            );
        }
    }

    /// 字典文案不得包含宿主用 `===` 比较的那两个魔术字符串。
    ///
    /// `'服务器繁忙'` 会被宿主当成「重试」信号，`'取消http请求'` 会被当成「用户
    /// 主动取消」。任何一条普通文案只要含有它们，就会在不该重试、不该静默的场合
    /// 被宿主的字符串比较命中——那不是我们能从服务端观测到的行为。
    #[test]
    fn dictionary_never_collides_with_host_magic_strings() {
        for (code, message) in SCRIPT_ERROR_MESSAGES {
            assert!(!message.contains("服务器繁忙"), "{code:?} 撞上了重试信号");
            assert!(!message.contains("取消http请求"), "{code:?} 撞上了取消信号");
        }
    }

    /// 借道宿主重试时抛出的必须是**裸**文案：宿主用 `===` 比较，带前缀即失效。
    #[test]
    fn busy_rejection_is_bare_and_exact() {
        let script = sample_script();
        assert!(
            script.contains("const BUSY_MESSAGE = \"服务器繁忙\""),
            "BUSY_MESSAGE 必须逐字等于宿主的重试信号"
        );
        assert!(
            script.contains("new Error(BUSY_MESSAGE)"),
            "重试路径必须原样抛出 BUSY_MESSAGE"
        );
        assert!(
            !script.contains("kg2lx：服务器繁忙"),
            "前缀会让重试信号不再被宿主识别"
        );
    }

    /// 「有界」不能只是注释里的一句话：上限与冷却窗口都要真的出现在守卫里。
    #[test]
    fn busy_retry_is_bounded() {
        let script = sample_script();
        const MARKER: &str = ".catch((err) => {";
        let start = script
            .find(MARKER)
            .expect("the request handler must own the busy-retry decision");
        let guard = &script[start..];
        assert!(guard.contains("BUSY_RETRY_LIMIT"), "缺少次数上限");
        assert!(guard.contains("BUSY_COOLDOWN_MS"), "缺少冷却窗口");
    }

    /// 自己的超时必须早于宿主的 20 秒硬超时，否则用户看到的是那句未翻译的
    /// `Cancel request`。
    #[test]
    fn request_timeout_beats_the_host_deadline() {
        // 两个都是编译期常量，放进 const block 让「余量确实存在」这件事在编译期
        // 就被证明一次，而不是等这条测试跑起来。
        const { assert!(SCRIPT_REQUEST_TIMEOUT_MS < 20_000) };
        assert!(
            sample_script().contains("timeout: REQUEST_TIMEOUT_MS"),
            "request() 必须真的收到这个超时值"
        );
    }

    /// 返回地址前后两侧的护栏：宿主的 preload.js 会用字面量 `failed` 拒绝
    /// 非 http(s) 或超长的地址，我们必须先给出中文原因。
    #[test]
    fn returned_url_is_guarded() {
        let script = sample_script();
        assert!(script.contains("/^https?:/"), "缺少 scheme 校验");
        assert!(script.contains("MAX_URL_LEN"), "缺少长度校验");
    }

    /// 后端的 `message` 是写给服务端日志的诊断散文，一个字都不该到用户眼前。
    #[test]
    fn backend_message_is_never_displayed() {
        assert!(
            !sample_script().contains("data.message"),
            "后端诊断文本不得进入脚本"
        );
    }

    /// 字典里没有的 code 必须落到本地化兜底，而不是后端的英文 text。
    #[test]
    fn unknown_code_has_a_localized_fallback() {
        let script = sample_script();
        assert!(
            script.contains("ERROR_MESSAGES[data.code] || UNKNOWN_ERROR"),
            "未命中的 code 必须回退到 UNKNOWN_ERROR"
        );
        assert_ne!(SCRIPT_UNKNOWN_ERROR, SCRIPT_BUSY_MESSAGE);
        assert!(!SCRIPT_UNKNOWN_ERROR.contains("取消http请求"));
    }
}
