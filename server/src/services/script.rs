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
/// 元数据、自带错误文案字典并自控请求超时与返回地址校验、以及本轮新增的
/// 「有新版脚本」提醒）全部**尚未发行**，故合计只 +1。
///
/// 本轮新增的提醒机制有一个必须写下来的后果：**它通知不了任何人**，因为通知
/// 本身也得由脚本发出，而已经导入的脚本里没有这段代码（见 `SCRIPT_RELEASE_NOTES`
/// 与 `check_script_update`）。它只对**此后**导入的版本生效。
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
    (ErrorCode::LoginSessionNotFound, "登录已过期，请重新扫码"),
    (ErrorCode::UpstreamRequestFailed, "连接酷狗失败，请稍后重试"),
    (ErrorCode::UpstreamLoginFailed, "登录态失效，请在后台刷新"),
    (
        ErrorCode::UpstreamVipCheckFailed,
        "VIP 校验失败，请稍后重试",
    ),
    // 「未取到播放地址」刻意不写成「暂无可用音质」：拆分后它的确切含义是
    // 「没拿到 url、且上游没有表达拒绝」，上游明确拒绝某一档是下面那条。
    // 两句读起来像同一件事，用户就无法从播放条上分辨该不该去改什么。
    (ErrorCode::UpstreamPlayUrlEmpty, "未取到播放地址"),
    (ErrorCode::UpstreamQualityRefused, "该音质不可用"),
    (ErrorCode::UpstreamQualityExhausted, "所有音质均不可用"),
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

/// 更新检查那次请求自己的超时，单位毫秒。
///
/// **Why 与 `SCRIPT_REQUEST_TIMEOUT_MS` 分开写两个常量：** 两者约束的是两件不同的
/// 事。取链是用户正在等的一个动作，慢一点也值得等；更新检查是脚本启动时的静默
/// 探测，用户根本不知道它在跑，等得久只是白占一次连接、并让用户的机器多做一次
/// 无用功。共用一个值会逼着两者一起改，而它们没有任何一起改的理由。
///
/// **Why 必须 < 20 秒：** 宿主对每个脚本请求都会起一个 20 秒硬超时计时器，到点
/// 以字面量 `Cancel request` 拒绝（见 `SCRIPT_REQUEST_TIMEOUT_MS`）；这个英文串
/// 会被显示在播放条上。5 秒给进程内调度留了充分余量，同时短到用户察觉不到。
const SCRIPT_UPDATE_TIMEOUT_MS: u64 = 5_000;

/// 脚本发行说明：`(版本号, 该版的说明条目)`，**新 → 旧**有序；每一条自成一行。
///
/// 它只回答一个问题：两个版本之间发生了什么。服务端的 `check_script_update` 会把
/// 用户手上那份脚本的版本与 `SCRIPT_VERSION` 之间的**全部**条目按这里的次序摘出，
/// 因为用户可能一次跳好几版（他上一次导入与这一次导入之间，服务端可能已经发了
/// 好几版）——只给最新一版等于让他在一无所知的情况下做「删掉旧音源、重新导入」
/// 这个不可撤销的动作。
///
/// **为什么是条目列表而不是一整句话：** 宿主把 `log` 渲染成 `white-space: pre-line`
/// 的纯文本，而那个对话框的内容盒**只有 `min-width`、没有 `max-width`**——它的宽度
/// 跟着消息里最长的那一行走，只被窗口宽度封顶。一句话写两百字会让最长行远宽于窗口，
/// 于是弹窗被撑满整个窗口、正文横贯全屏；拆成每行二三十字的短条目，弹窗才会收成
/// 一个窄块，也才扫得动。**换行是这个弹窗里唯一可用的排版手段**，所以这里存的是
/// 行，而不是段落。条目长度由
/// `SCRIPT_UPDATE_NOTE_ITEM_MAX_CHARS` 兜住。
///
/// **只写能当作事实陈述的条目。** 表里没有 `0.1.1`：那一版随 `v0.1.3` 发行，
/// 但它究竟改了什么已经无从查证，凭印象补一句就是发明事实。区间查询对空缺是
/// 容忍的——用户在 `(client, 最新]` 内拿到几条就是几条，缺条目只会少一段背景，
/// 而导入地址与动作指令在任何情况下都在。
///
/// **顺序是语义，不是格式**：`check_script_update` 直接按这里的次序过滤输出，
/// 不做排序。排序会把「表里放错位置」这件事掩盖掉，而用户读这段文字时正是按
/// 新 → 旧理解的。
const SCRIPT_RELEASE_NOTES: &[(&str, &[&str])] = &[(
    "0.1.2",
    &[
        "错误提示改为脚本自带的中文短句，播放条上不再出现英文诊断",
        "取链失败分得更细：音质不可用 / 全部不可用 / 没取到地址",
        "请求超时由脚本自己控制在 15 秒",
        "请求体带上歌名 / 歌手 / 专辑，日志能看出是哪首歌",
        "新增本弹窗",
    ],
)];

/// 单条发行说明的长度上限（字符数）。
///
/// **Why 有这条上限：** 对话框的宽度由消息里最长那一行决定（见
/// `SCRIPT_RELEASE_NOTES`），所以一条写长的说明会把整个弹窗撑宽。40 字符在 14px 下
/// 约 560px，加上左右各 15px 内边距仍在窗口最小预设（828px）之内——留出这个余量，
/// 「最长行」才轮得到那行导入地址去决定弹窗宽窄，而地址的长度我们控制不了。
///
/// 它是**维护时的断言**，不是运行时截断：超限由测试挡下，不会把用户看到的字砍掉。
///
/// `allow(dead_code)`：唯一的消费方是 `#[cfg(test)]` 的测试模块，非测试的 bin target
/// 看不到它，于是会报「从未被使用」。不要因为这条警告把它删掉——删了这条护栏就没了。
#[allow(dead_code)]
const SCRIPT_UPDATE_NOTE_ITEM_MAX_CHARS: usize = 40;

/// `log` 的长度目标：整段内容应当落在这个字符数以内，超出时从**版本说明**开始牺牲。
///
/// **Why 700：** 宿主在 1024 字符处从尾部截断（contract.md 的
/// `EVENT_NAMES.updateAlert` 一节），但在那个数字之前就该收手——弹窗里的正文是
/// 一段可增长的纯文本，而它没有 `max-height`，写长了是把对话框顶出窗口而不是
/// 在内部滚动。700 是「够讲清一件事」与「一眼能扫到末尾」之间的取值，且它是我们
/// 自己定的，不随宿主变。
const SCRIPT_UPDATE_LOG_TARGET_CHARS: usize = 700;

/// `log` 的硬上限：无论如何都不允许超过这个字符数。
///
/// 比宿主的 1024 还留出余量，因为**它的 1024 是截断点而不是拒绝点**：超出的部分
/// 会被它补一个 `...` 之后直接丢掉，丢掉的是尾部——而尾部是版本说明。我们自己
/// 先截，至少能保证截断发生在有意的位置。
const SCRIPT_UPDATE_LOG_MAX_CHARS: usize = 1000;

/// 版本说明被截断时补的收束语。
///
/// 没有它，用户会以为版本说明就到这里为止——一段被砍掉的话不加任何标记，读起来
/// 与「这版就只改了这一件事」完全一样。用**常量**而不是「另有 N 条未显示」这种
/// 带数字的说法，是为了让下面那段预算能精确预留它的长度（见 `render_update_log`）；
/// 截断只发生在这个弹窗最不缺信息的时候，换来一条永不超过目标的硬保证是划算的。
const SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL: &str = "（说明较长，以下有省略）";

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

/// 脚本版本的三元组（major, minor, patch）。
///
/// `SCRIPT_VERSION` 与客户端上报的 `@version` 都解析成它再比较。用元组而不是
/// 字符串比较：字符串比较会让 `0.1.10` 排在 `0.1.9` 之前，而那个错误只会在
/// 十位进位的那一版上出现一次——恰好是最难在测试里复现的时机。
type ScriptVersion = (u64, u64, u64);

/// 只认 `x.y.z` 三段十进制，前后可有空白。任何偏差都返回 `None`。
///
/// **Why 收得这么紧：** 这个函数的调用点只有一处，它的返回值决定**要不要弹窗**。
/// 宽松解析（补位、忽略后缀、把 `1.2` 当成 `1.2.0`）在这里的收益是零——客户端
/// 那一侧的 `@version` 由本文件渲染，形状是确定的；解析不出来只意味着有人手改了
/// 脚本头部。那时的正确回答是「不知道，算没有更新」，凭猜出来的版本去打扰用户
/// 才是错的。
///
/// 注意它同时挡掉了 `0.1.2.3`：`split('.')` 之后还有第四段就直接判不通过，
/// 而不是「读前三段忽略其余」。
fn parse_script_version(raw: &str) -> Option<ScriptVersion> {
    let mut parts = raw.trim().split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// 三元组还原成 `x.y.z`。
///
/// 展示给用户的版本号一律走这里，而不是把客户端上报的原始串直接拼进文案：
/// 走到这里就说明它已经解析成功（只含数字与点），但用三元组重排还能顺手吃掉
/// 前后空白与 `0.01.2` 这类写法，让「当前 X → 最新 Y」两边长得一样。
fn format_script_version(version: ScriptVersion) -> String {
    let (major, minor, patch) = version;
    format!("{major}.{minor}.{patch}")
}

/// 严格比较：`latest` **严格大于** `client` 才算有更新。
///
/// **Why 不是 `!=`：** 相等时弹窗是纯骚扰；而「客户端比服务端新」只可能来自
/// 服务端回滚（把已发行的脚本版本降回去），那是我们的运维事件，不该变成用户
/// 机器上的一个弹窗。
///
/// **Why 任一侧解析失败都返回 false：** 两侧都不确定时唯一安全的答案是「没有
/// 更新」——弹窗是用户无法从我们这侧撤销的动作（他要么忽略，要么去删音源）。
fn is_newer_script_version(latest: &str, client: &str) -> bool {
    match (parse_script_version(latest), parse_script_version(client)) {
        (Some(latest), Some(client)) => latest > client,
        _ => false,
    }
}

/// 摘出 `(client, latest]` 区间内的条目，保持表里的**新 → 旧**次序。
///
/// 左开右闭：`client` 那一版用户已经有了（他手上那份脚本就是它），`latest` 那一版
/// 正是要告诉他的。区间而不是「只给最新一版」，是因为用户上一次导入之后服务端可能
/// 已经发了好几版，而他做的是「删掉旧音源」这个不可撤销的动作——只讲最后一版
/// 等于让他蒙着眼睛删。
///
/// 表由调用方传入而不是直接读 `SCRIPT_RELEASE_NOTES`：真实的那张表当前只有一个
/// 条目（越界的版本无从查证），而「一次跳好几版」要等第二次 bump 才会真的发生；
/// 到那时才发现区间摘错了，用户已经在删音源了。
fn release_notes_between<'a>(
    notes: &'a [(&'a str, &'a [&'a str])],
    client: ScriptVersion,
    latest: ScriptVersion,
) -> Vec<(&'a str, &'a [&'a str])> {
    notes
        .iter()
        .copied()
        .filter(|(version, _)| {
            parse_script_version(version).is_some_and(|v| v > client && v <= latest)
        })
        .collect()
}

/// 把一次「有更新」的答复渲染成用户看到的那段文本。
///
/// **四段的顺序是承重的，不能重排：** ① 版本落差；② 动作指令；③ 导入地址（单独
/// 一行）；④ 版本说明。宿主在 1024 字符处**从尾部**截断 `log`，因此越靠后的内容
/// 越先被牺牲——地址被吃掉，这条通知里唯一能让用户动起来的东西就没了；而说明被
/// 吃掉只是少了一段背景。所以地址必须排在说明之前，且**独占一行**：它与动作指令
/// 挤在同一行时，那一行的长度由地址决定（地址是定长的、我们缩短不了），单独成行
/// 至少让它成为一个干净的复制单位。
///
/// 说明段按**行**排布、按**行**截断（一行一个版本标题、一条说明一行），这是弹窗
/// 里唯一可用的排版手段：宿主那个对话框的宽度跟着最长一行走，不分行就等于把弹窗
/// 撑满整个窗口。截断也只发生在行边界——切出半句话再加收束语，读起来比少一条更糟。
///
/// 预算分两档（目标 `SCRIPT_UPDATE_LOG_TARGET_CHARS` / 硬上限
/// `SCRIPT_UPDATE_LOG_MAX_CHARS`）。三条硬底线都有测试守着：地址那一行永远在；
/// 整段永远不超过硬上限；说明一条都装不下时连「本次更新内容：」这个抬头一起省掉
/// （一个下面空无一物的抬头比没有抬头更怪）。
fn render_update_log(
    public_base_url: &str,
    script_token: &str,
    client: ScriptVersion,
    latest: ScriptVersion,
    notes: &[(&str, &[&str])],
) -> String {
    let mut log = format!(
        "脚本版本更新：当前 {} → 最新 {}\n\
         请先删除旧的音源条目，再导入新脚本：\n\
         {public_base_url}/s/{script_token}.js\n\
         洛雪音乐不会自动更新已导入的脚本，导入后请删掉旧条目。\n",
        format_script_version(client),
        format_script_version(latest),
    );

    if !notes.is_empty() {
        const HEADER: &str = "本次更新内容：\n";
        let tail = SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL;
        // 说明段的总预算。**收束语先占位**：它是否需要出现要到循环末尾才知道，
        // 而预留位置只会让一条刚好装得下的说明被挪走一条——代价是少一段背景，
        // 可接受的。反过来（事后再扣）就会让结果比目标多出收束语那么长。
        let line_budget = SCRIPT_UPDATE_LOG_TARGET_CHARS
            .saturating_sub(log.chars().count())
            .saturating_sub(HEADER.chars().count())
            .saturating_sub(tail.chars().count());
        let mut section = String::from(HEADER);
        let mut used = 0usize;
        let mut included_any = false;
        let mut truncated = false;

        // 摊平成一维的行：一个版本一行标题，每条说明各占一行。摊平而不是按版本块
        // 整体取舍，是为了让预算耗尽的边界落在**行**上——整块取舍在说明颗粒度变细
        // 之后会浪费掉大量预算。
        let lines = notes.iter().flat_map(|(version, items)| {
            std::iter::once(format!("- {version}"))
                .chain(items.iter().map(|item| format!("  · {item}")))
        });

        for line in lines {
            let rendered = format!("{line}\n");
            let width = rendered.chars().count();
            if used + width > line_budget {
                truncated = true;
                break;
            }
            section.push_str(&rendered);
            used += width;
            included_any = true;
        }

        if included_any {
            if truncated {
                section.push_str(tail);
            }
            log.push_str(&section);
        }
    }

    if log.chars().count() > SCRIPT_UPDATE_LOG_MAX_CHARS {
        // 走到这里只可能是说明本身离谱地长。截尾不会碰到地址那一行：它离这个
        // 上限有一大截距离，而说明排在它后面。
        log = log.chars().take(SCRIPT_UPDATE_LOG_MAX_CHARS).collect();
    }

    log
}

/// `POST /api/v1/runtime/script-update` 的响应。
///
/// `log` 与 `update_url` 都带 `skip_serializing_if`，因此「无更新」的信封是且仅是
/// `{"update": false}`——没有 `log: null`，也没有 `update_url: null`。
///
/// **Why 是键消失而不是值为 null：** 客户端那一侧要判断「服务端有没有给我东西」，
/// 而 `null` 与「键不存在」在 JS 里读到的是两件事（`'update_url' in data` 与
/// `data.update_url === null` 不同），多一种状态就多一条要解释的路径。不给 web 产物
/// 时我们确实**没有**这个地址，那就不该在信封里放一个位置。
#[derive(serde::Serialize)]
pub struct ScriptUpdateResponse {
    pub update: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_url: Option<String>,
}

impl ScriptUpdateResponse {
    /// 「没有更新」的唯一构造入口：三个字段里只有一个 `false`。
    fn none() -> Self {
        Self {
            update: false,
            log: None,
            update_url: None,
        }
    }
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

    /// 回答「你手上这份脚本还新吗」，有更新时把版本说明一并给出来。
    ///
    /// 认证与归属同 `music-url`：`Authorization: Bearer <runtime_token>`，查不到
    /// 音源就是 `SOURCE_NOT_FOUND`（空 token 在路由层的 `verify_runtime` 就被挡成
    /// `ADMIN_UNAUTHORIZED`，到不了这里）。除此之外它与取链**刻意有三处不同**：
    ///
    /// 1. **不看 `source.enabled`。** 这条通知讲的是「你手上那份脚本旧了」，与音源
    ///    启不启用无关；停用是暂时的，用户照样会在某天把它启回来，而那时他手上的
    ///    仍然是旧脚本。顺手抄一份 enabled 预检等于让停用期的用户永远收不到通知。
    /// 2. **不查账号。** 版本判定只用脚本版本，账号状态、登录态、权益都不参与——
    ///    它们各自有另外的失败面，掺进来只会让这条通知莫名其妙地消失。
    /// 3. **不写运行时日志。** `RuntimeLog` 记的是取链事件，这条通知不是取链，
    ///    强行落一行会让日志面板里混进一批既没有 hash 也没有音质的行。
    ///
    /// 客户端版本解析不出来时按「无更新」处理：脚本头部被手改属于自找，
    /// 拿一个猜出来的版本去弹窗才是错的。
    pub async fn check_script_update(
        &self,
        runtime_token: &str,
        client_version: &str,
    ) -> Result<ScriptUpdateResponse, AppError> {
        let source = self
            .source_repo
            .find_by_runtime_token(runtime_token)
            .await?
            .ok_or_else(AppError::source_not_found)?;

        if !is_newer_script_version(SCRIPT_VERSION, client_version) {
            // 相等、客户端更新（服务端回滚）、或任一侧解析失败，都落在这个分支。
            return Ok(ScriptUpdateResponse::none());
        }
        // 上面那句已经证明两侧都解析得出来，这里再解析一次拿三元组——说明区间
        // 要按版本比较而不按字符串。刻意不用 `expect`：这是 HTTP 处理器里的路径，
        // 将来若有人动过上面的判定，这里宁可退回「无更新」，也不要 panic。
        let (Some(client), Some(latest)) = (
            parse_script_version(client_version),
            parse_script_version(SCRIPT_VERSION),
        ) else {
            return Ok(ScriptUpdateResponse::none());
        };

        let log = render_update_log(
            &self.config.public_base_url,
            &source.script_token,
            client,
            latest,
            &release_notes_between(SCRIPT_RELEASE_NOTES, client, latest),
        );

        // 后台音源详情页。**只在 web 产物已构建时给**：那个页面由同一个进程托管，
        // 没有产物时它是 404，把用户送去一个 404 比不给按钮更糟。此时整个键不出现。
        let update_url = self.config.web_dist_dir.as_ref().map(|_| {
            format!(
                "{}/sources/{}",
                self.config.public_base_url, source.source_id
            )
        });

        Ok(ScriptUpdateResponse {
            update: true,
            log: Some(log),
            update_url,
        })
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
    let api_base_literal = js_string_literal(public_base_url);
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
// 基址只 bake 一次，两个端点各自拼后缀。分成两条完整地址的话，服务端将来改了
// 其中一个端点就要记得同时改另一条，而它们本来只有一个共同前缀会变。
const API_BASE = {api_base_literal}
const API = API_BASE + '/api/v1/runtime/music-url'
const UPDATE_API = API_BASE + '/api/v1/runtime/script-update'
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
// 更新检查用自己的超时，与取链分开：取链是用户正在等的动作，更新检查是启动时
// 的静默探测，等得久只是白占一次连接。两者都必须小于宿主 20 秒的硬超时。
const UPDATE_TIMEOUT_MS = {SCRIPT_UPDATE_TIMEOUT_MS}
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
  // 这个对象里只有 `hash` / `quality` 是取链的入参，其余全部是**记录**：服务端
  // 把它们落盘供事后诊断，不参与任何判断。`types` 就是宿主扁平 musicInfo 上的
  // 那个字段（LX 的 `meta.qualitys`：这首歌有哪些档位、各自的大小与 hash）。
  // 这里刻意不做 `Array.isArray` 之类的形状断言——它是事实记录而非判断输入，
  // 形状异常也该原样上报，而不是在脚本里被悄悄丢掉。
  return httpPost(API, {{
    hash: musicInfo.hash,
    album_audio_id: musicInfo.albumAudioId || null,
    quality,
    track_title: musicInfo.name || null,
    artist_name: musicInfo.singer || null,
    album_name: musicInfo.albumName || null,
    types: musicInfo.types || null,
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
// 更新检查**必须**排在 inited 之后：在它之前抛出的任何异常都会让导入直接失败
// （宿主会把消息摆进「自定义源初始化失败」弹窗里），而这一段要联网，绝不能挡在
// 导入路径上。这里是顶层的 fire-and-forget，它不参与任何取链流程。
const checkScriptUpdate = () => new Promise((resolve) => {{
  // 版本来自导入时冻结的头部元信息。`currentScriptInfo` 由宿主提供，读不到或为
  // 空就整段跳过——此时唯一正确的动作是什么都不做，绝不能因此抛出去。
  const info = globalThis.lx && globalThis.lx.currentScriptInfo
  const version = info && info.version
  if (typeof version !== 'string' || !version) return resolve(null)
  request(UPDATE_API, {{
    method: 'POST',
    headers: {{
      'content-type': 'application/json',
      authorization: `Bearer ${{TOKEN}}`,
    }},
    body: {{ version: version }},
    timeout: UPDATE_TIMEOUT_MS,
  }}, (err, resp) => {{
    // 这一段刻意既不构造用户可见的错误文案，也不 reject：更新检查的失败必须与
    // 「本来就没有更新」在表现上完全不可区分。若图省事复用取链那条请求路径，一次
    // 网络不通就会给用户的播放条塞一条「kg2lx：…」——而那次失败与他手上的版本毫无
    // 关系，他做不了任何事。因此这个 Promise 永远 resolve，reject 一次都不会出现。
    if (err) return resolve(null)
    const statusCode = resp && resp.statusCode
    if (statusCode && statusCode >= 400) return resolve(null)
    resolve(parseBody(resp && resp.body))
  }})
}})
// 发弹窗前的形状校验。**顺序是承重的**：宿主「本轮只弹一次」的额度在校验 payload
// *之前*就被扣掉了，一次被拒的 send 会白白花掉这次脚本运行里唯一的机会——而脚本
// 每次切回该音源都会重新执行一次，浪费掉的那次要等下一次运行才补得回来。
const alertScriptUpdate = (data) => {{
  if (!data || data.update !== true) return
  if (typeof data.log !== 'string' || !data.log) return
  const payload = {{ log: data.log }}
  // 这两个名字相邻却分属两层，别顺手统一：读的是**我们自己信封**的蛇形键
  // `data.update_url`（`ScriptUpdateResponse` 没有 serde rename，线上就是这个
  // 形状），写进去的却是**宿主** `updateAlert` 参数要求的驼峰 `payload.updateUrl`。
  // 此前读端误用了驼峰，条件恒假，于是地址从来没进过弹窗——一处死代码。
  // updateUrl 只在服务端真的给了非空字符串时才带上：宿主对不带 scheme 的地址会
  // 静默丢弃（点了没反应），我们只送合规的值。
  if (typeof data.update_url === 'string' && data.update_url) payload.updateUrl = data.update_url
  // send 返回的 Promise 会被拒绝（额度已被用掉时是 'The update alert can only be
  // called once.'），必须接住，否则沙箱里会出现一个没人处理的 rejection。
  send(EVENT_NAMES.updateAlert, payload).catch(() => {{}})
}}
checkScriptUpdate().then(alertScriptUpdate).catch(() => {{}})
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

    use std::path::PathBuf;

    use async_trait::async_trait;

    use crate::domain::account::ProviderAccount;
    use crate::domain::provider::ProviderKind;
    use crate::domain::source::Source;

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
    ///
    /// bake 的形态从「一条完整地址」改成了「基址 + 两条派生」之后，这条断言的
    /// 落点跟着从 `const API = ...` 挪到 `const API_BASE = ...`：要守的性质没变
    /// （那个可能含 `'` 的配置值仍然被包在双引号里、转义过），只是它现在出现在
    /// 基址那一行。两条派生地址因此不可能再各自成为第二个注入点。
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
            script.contains(r#"const API_BASE = "http://host/a'b""#),
            "base url must be a double-quoted literal so an apostrophe cannot escape"
        );
        assert!(
            script.contains("const API = API_BASE + '/api/v1/runtime/music-url'"),
            "两条端点地址都从基址派生，不再各自 bake 一个完整地址"
        );
        assert!(
            script.contains("const UPDATE_API = API_BASE + '/api/v1/runtime/script-update'"),
            "更新检查的端点同样从基址派生"
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
                "album_name: musicInfo.albumName || null, ",
                "types: musicInfo.types || null,"
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

    /// 渲染出的脚本是**冻结产物**：用户导入后，洛雪音乐永不自动更新它，因此任何
    /// 写进脚本的策略都会在用户手上那一份里永久固定下来。
    ///
    /// 「上游拒绝了这一档该怎么办」这件事必须只活在服务端（后台的降级开关 +
    /// 取链循环）：只有这样，改一次就对所有人生效。这条断言挡的是有人图方便把
    /// 策略写回模板——那会逼每个用户删掉音源重新导入才能受益，正是 `@version`
    /// 一节反复说明要避免的东西。检查的是**渲染结果**，服务端自己的策略代码
    /// 不受影响。
    #[test]
    fn script_carries_no_quality_policy() {
        let script = sample_script();
        assert!(
            !script.to_lowercase().contains("fallback"),
            "脚本不得含任何降级策略：策略只应活在服务端"
        );
        assert!(
            !script.contains("降级"),
            "脚本不得含任何降级策略：策略只应活在服务端"
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

    // ---- 更新检查：渲染结果侧 ----
    //
    // 下面这几条断言的对象是**渲染出来的脚本正文**，不是常量表——脚本是冻结产物，
    // 它落到用户机器上之后的形状只能靠这一层守住。每一段检查都先切出自己那一小段
    // 再断言，而不是对整个脚本做 `contains`：整篇查什么都查不出来，因为 `fail(`、
    // `httpPost(`、`timeout:` 这些东西本来就在文件里别的地方。

    /// 从 `const checkScriptUpdate` 起、到脚本末尾的那一段（静默请求 + 弹窗 + 发起）。
    fn update_check_path(script: &str) -> &str {
        let start = script
            .find("const checkScriptUpdate = ")
            .expect("脚本必须包含更新检查那一整段");
        &script[start..]
    }

    /// 只含静默请求 helper 的那一段，到弹窗函数为止。
    fn update_request_helper(script: &str) -> &str {
        let path = update_check_path(script);
        let end = path
            .find("\nconst alertScriptUpdate = ")
            .expect("静默 helper 必须与弹窗函数分开，否则这两条断言的范围就不成立了");
        &path[..end]
    }

    /// 用固定的基址与 script_token 渲染一段 `log`，让断言只关心组装规则本身。
    fn update_log(
        client: ScriptVersion,
        latest: ScriptVersion,
        notes: &[(&str, &[&str])],
    ) -> String {
        render_update_log("http://127.0.0.1:8787", "st_update", client, latest, notes)
    }

    /// 更新检查必须发起在 `send(EVENT_NAMES.inited` **之后**。
    ///
    /// 位置断言，不是「有没有发」的断言：宿主对 `inited` 之前抛出的任何异常都会让
    /// **导入失败**（并把消息摆进「自定义源初始化失败」弹窗），而这段检查要联网。
    /// 内容一字不改地挪到 `inited` 前面，功能描述上仍然「正确」，后果却是用户装不上
    /// 音源——这条挡的正是那种挪动。
    #[test]
    fn update_check_call_sits_after_inited() {
        let script = sample_script();
        let inited = script
            .find("send(EVENT_NAMES.inited")
            .expect("脚本必须发 inited");
        let check = script
            .find("checkScriptUpdate()")
            .expect("脚本必须发起更新检查");
        assert!(
            inited < check,
            "更新检查必须在 inited 之后发起，否则一次网络不通就会让导入直接失败"
        );
    }

    /// 更新检查那条路径里不得出现 `fail(`，也不得出现 `httpPost(`。
    ///
    /// `httpPost` 的失败会走 `fail()`，也就是播放条上一句 `kg2lx：…`，还带
    /// `retryable` 语义；而更新检查的失败必须与「本来就没有更新」在表现上完全
    /// 不可区分——那次失败与用户手上的版本毫无关系，他做不了任何事，也不该被
    /// 打扰。断言范围收在这一段函数体内：整篇 `contains` 恒为真，什么也挡不住。
    #[test]
    fn update_check_path_never_routes_through_fail_or_http_post() {
        let script = sample_script();
        let path = update_check_path(&script);
        assert!(
            !path.contains("fail("),
            "更新检查不得构造用户可见的错误文案"
        );
        assert!(
            !path.contains("httpPost("),
            "更新检查不得复用取链的请求路径：那条路的失败会显示在播放条上"
        );
        assert!(
            path.contains("request(UPDATE_API"),
            "更新检查必须真的用自己的请求"
        );
    }

    /// 更新请求必须带自己的显式超时，且那个超时小于宿主的 20 秒硬超时。
    ///
    /// 前者漏了就会走宿主的 60 秒默认值——比宿主自己的 20 秒还长，于是永远轮不到
    /// 我们自己的超时先失败。后者必须有：宿主到点是拿字面量 `Cancel request` 拒绝的，
    /// 那个英文串会显示在播放条上。
    #[test]
    fn update_check_uses_its_own_timeout_below_the_host_deadline() {
        // 编译期常量，放进 const block 让「余量确实存在」在编译期就被证明一次。
        const { assert!(SCRIPT_UPDATE_TIMEOUT_MS < 20_000) };

        let script = sample_script();
        assert!(
            update_request_helper(&script).contains("timeout: UPDATE_TIMEOUT_MS"),
            "更新请求必须显式设超时"
        );
        assert!(
            script.contains(&format!(
                "const UPDATE_TIMEOUT_MS = {SCRIPT_UPDATE_TIMEOUT_MS}"
            )),
            "这个常量必须真的渲染进脚本"
        );
        assert_ne!(
            SCRIPT_UPDATE_TIMEOUT_MS, SCRIPT_REQUEST_TIMEOUT_MS,
            "取链 15 秒与更新检查 5 秒是两件事：合并成一个值会逼着它们一起改"
        );
    }

    /// 发弹窗前必须先校验 payload 形状，且 `send` 返回的 Promise 必须被接住。
    ///
    /// 两件事都因为宿主的同一个实现细节而承重：`updateAlert` 的「本轮只弹一次」
    /// 额度是在**校验 payload 之前**扣掉的。所以一次带着空 `log` 的 send 不但什么
    /// 都不显示，还会花掉这次脚本运行里唯一的机会；而脚本每次切回该音源都会重新
    /// 执行一遍，浪费掉的那次要等下一轮才补得回来。那个 send 的返回值又是个会被
    /// 拒绝的 Promise，不接住就是沙箱里一个没人处理的 rejection。
    #[test]
    fn update_alert_validates_the_payload_before_sending() {
        let script = sample_script();
        let path = update_check_path(&script);

        let guard = path
            .find("typeof data.log !== 'string' || !data.log")
            .expect("必须校验 log 是非空字符串");
        let send = path
            .find("send(EVENT_NAMES.updateAlert")
            .expect("必须真的发出 updateAlert");
        assert!(
            guard < send,
            "形状校验必须排在 send 之前：额度在校验 payload 之前就被扣掉了"
        );
        assert!(
            path.contains("data.update !== true"),
            "只有在服务端明确说 update === true 时才弹"
        );
        assert!(
            path.contains("send(EVENT_NAMES.updateAlert, payload).catch("),
            "send 的返回值是会被拒绝的 Promise，必须接住"
        );
    }

    /// 静默 helper 不得有任何把错误抛出去的路径。
    ///
    /// 更新检查的失败必须与「本来就没有更新」完全不可区分，因此这里连 `err.message`
    /// 都不许读——读了就说明有人正打算把它交给某个会显示它的地方。`reject(` 同样
    /// 不许出现：这个 Promise 永远 resolve，executor 连 `reject` 参数都没有。
    #[test]
    fn silent_update_helper_never_rethrows() {
        let script = sample_script();
        let helper = update_request_helper(&script);

        assert!(
            !helper.contains("err.message"),
            "helper 不得读取 err.message：任何失败都只是「没有更新」"
        );
        assert!(
            !helper.contains("reject("),
            "helper 的 Promise 必须永不 reject：reject 就是一次未处理的 rejection"
        );
        assert!(
            helper.contains("if (err) return resolve(null)"),
            "传输层失败必须安静地当作没有更新"
        );
        assert!(
            helper.contains("globalThis.lx.currentScriptInfo"),
            "版本来源是导入时冻结的头部元信息"
        );
        assert!(
            helper.contains("if (typeof version !== 'string' || !version) return resolve(null)"),
            "取不到版本就整段跳过，绝不能抛"
        );
    }

    /// 更新检查的**接线**：脚本读的信封键必须与 DTO 真的序列化出的键是同一个。
    ///
    /// 与 `request_body_maps_lx_music_info_fields` 同一个成因——两侧的名字没有任何
    /// 编译器能对齐。DTO 改了名而这里照旧读老名字时，编译不响、上面那几条只盯
    /// `log` 守卫与 `send(...).catch(` 的断言也都不响，功能却整段变成死代码：条件
    /// 恒假，`update_url` 永远进不了弹窗。所以键名**从 DTO 自己推**，不抄字面量——
    /// 把 `script.contains("data.update_url")` 写死，DTO 改名时它照样是绿的，等于
    /// 没守。
    ///
    /// 两个方向都钉：信封侧改名要让这条报错；有人把宿主侧也「顺手统一」成蛇形，
    /// 同样要让这条报错——`payload.updateUrl` 是宿主 `updateAlert` 的参数名，与
    /// 我们信封的字段名分属两层，不是一回事。
    #[test]
    fn update_alert_reads_the_key_the_dto_actually_serializes() {
        let resp = ScriptUpdateResponse {
            update: true,
            log: Some("有新版".into()),
            update_url: Some("http://127.0.0.1:8787/sources/src_update".into()),
        };
        let value = serde_json::to_value(&resp).expect("响应必须能序列化");
        let wire_key = value
            .as_object()
            .expect("信封是 JSON 对象")
            .keys()
            .find(|k| *k != "update" && *k != "log")
            .expect("信封里除 update / log 之外还应当有一个地址键");

        let script = sample_script();
        assert!(
            script.contains(&format!("data.{wire_key}")),
            "脚本读的信封键必须与 DTO 序列化出的 `{wire_key}` 一致，否则 update_url 是死代码"
        );
        assert!(
            script.contains("payload.updateUrl"),
            "送进宿主的键必须保持宿主 updateAlert 的驼峰参数名 updateUrl"
        );
    }

    // ---- 更新检查：服务端逻辑侧 ----
    //
    // 这一组不需要网络、也不需要账号：`check_script_update` 刻意不读账号，账号仓库
    // 在这里是个只会返回空值的桩。

    struct FakeSourceRepo {
        source: Option<Source>,
    }

    #[async_trait]
    impl SourceRepo for FakeSourceRepo {
        async fn find_by_id(&self, _source_id: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_script_token(&self, _token: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn find_by_runtime_token(&self, token: &str) -> Result<Option<Source>, AppError> {
            Ok(self.source.clone().filter(|s| s.runtime_token == token))
        }

        async fn find_by_account_id(&self, _account_id: &str) -> Result<Option<Source>, AppError> {
            Ok(None)
        }

        async fn upsert(&self, _source: &Source) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_all(&self) -> Result<Vec<Source>, AppError> {
            Ok(Vec::new())
        }
    }

    /// 更新检查**不该**碰账号仓库。全部方法返回空值：若哪天有人给版本判定加上账号
    /// 条件，这里返回的 `None` 会让那条路径的行为露出破口，而不是安静地通过。
    struct FakeAccountRepo;

    #[async_trait]
    impl AccountRepo for FakeAccountRepo {
        async fn find_by_id(&self, _account_id: &str) -> Result<Option<ProviderAccount>, AppError> {
            Ok(None)
        }

        async fn find_by_provider_userid(
            &self,
            _provider: ProviderKind,
            _upstream_userid: &str,
        ) -> Result<Option<ProviderAccount>, AppError> {
            Ok(None)
        }

        async fn upsert(&self, _account: &ProviderAccount) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn update_config(web_dist_dir: Option<PathBuf>) -> Arc<Config> {
        Arc::new(Config {
            listen_addr: "127.0.0.1:0".into(),
            public_base_url: "http://127.0.0.1:8787".into(),
            admin_token: "test-admin-token".into(),
            kugou_api_base_url: "http://127.0.0.1:1".into(),
            data_dir: "./data".into(),
            upstream_timeout_ms: 1_000,
            refresh_interval_secs: 6 * 3600,
            web_dist_dir,
        })
    }

    fn update_source(runtime_token: &str, enabled: bool) -> Source {
        Source {
            source_id: "src_update".into(),
            account_id: "acc_update".into(),
            provider: ProviderKind::KugouLite,
            name: "测试音源".into(),
            enabled,
            script_token: "st_update".into(),
            runtime_token: runtime_token.into(),
            created_at: 0,
            updated_at: 0,
            quality_fallback: false,
        }
    }

    fn update_service(web_dist_dir: Option<PathBuf>, source: Option<Source>) -> ScriptService {
        ScriptService::new(
            update_config(web_dist_dir),
            Arc::new(FakeSourceRepo { source }),
            Arc::new(FakeAccountRepo),
        )
    }

    /// 响应的形状只能从序列化结果上断言：`skip_serializing_if` 决定的正是**信封里
    /// 有没有这个键**，而字段是 `None` 只说明我们没打算给它值——两者不是一回事。
    fn response_json(resp: &ScriptUpdateResponse) -> serde_json::Value {
        serde_json::to_value(resp).expect("响应必须能序列化")
    }

    /// 版本比较的三条边界：客户端更旧才有更新，相等没有，客户端**更新**也没有。
    ///
    /// 最后一条（服务端回滚）不是假想：把脚本版本降回去是修一版坏脚本的正常手段，
    /// 而那时所有已经导入更新版的用户手上都是一个「比服务端新」的版本。若判定写成
    /// `!=`，我们就会给他们每人弹一次窗，请他们去「更新」到一个更旧的脚本。
    #[test]
    fn version_comparison_is_strictly_greater() {
        assert!(
            is_newer_script_version("0.1.4", "0.1.2"),
            "客户端更旧 → 有更新"
        );
        assert!(
            !is_newer_script_version("0.1.2", "0.1.2"),
            "相等 → 没有更新"
        );
        assert!(
            !is_newer_script_version("0.1.2", "0.1.4"),
            "客户端更新（服务端回滚）→ 没有更新"
        );
        // 十位进位：按字符串比较会在这里给出相反答案。
        assert!(is_newer_script_version("0.1.10", "0.1.9"));
        assert!(!is_newer_script_version("0.1.9", "0.1.10"));
        assert!(is_newer_script_version("0.2.0", "0.1.99"));
    }

    /// 解析不出来的版本一律按「没有更新」处理，两侧都一样。
    ///
    /// 客户端那一侧落在这条上意味着有人手改了脚本头部；服务端那一侧意味着常量被写
    /// 坏了。两种情形都不能拿一个猜出来的版本去打扰用户——尤其不能 panic：这条路径
    /// 在一个 HTTP 处理器里。
    #[test]
    fn malformed_versions_never_trigger_an_update() {
        for bad in [
            "",
            "   ",
            "0.1",
            "0.1.2.3",
            "v0.1.2",
            "0.1.x",
            "abc",
            "0.1.-2",
            "０.１.２",
        ] {
            assert!(
                !is_newer_script_version("0.1.4", bad),
                "客户端版本 {bad:?} 解析不出来时必须按「无更新」处理"
            );
            assert!(
                !is_newer_script_version(bad, "0.1.0"),
                "服务端版本 {bad:?} 解析不出来时同样退回「无更新」，不能 panic"
            );
        }
    }

    /// 最新一版**必须**有自己的说明，且整张表是新 → 旧、版本号都解析得出来。
    ///
    /// 「最新一版有说明」是这张表存在的理由：跳版通知的全部价值就是把区间里的条目
    /// 摘给用户。若 bump 版本号的人忘了加条目，弹窗就会只说「你旧了」却不说「旧在
    /// 哪」——而那种状态能通过这里除了本条之外的所有测试。
    #[test]
    fn every_released_script_version_is_documented() {
        assert!(!SCRIPT_RELEASE_NOTES.is_empty(), "说明表不能是空的");
        assert!(
            parse_script_version(SCRIPT_VERSION).is_some(),
            "SCRIPT_VERSION 必须是能解析的 x.y.z"
        );

        let mut previous: Option<ScriptVersion> = None;
        for (version, items) in SCRIPT_RELEASE_NOTES {
            let parsed = parse_script_version(version)
                .unwrap_or_else(|| panic!("表里的版本号 {version:?} 解析不出来"));
            assert!(!items.is_empty(), "{version} 至少要有一条说明");
            for item in *items {
                let item = item.trim();
                assert!(!item.is_empty(), "{version} 的说明条目不能为空");
                assert!(
                    item.chars().count() <= SCRIPT_UPDATE_NOTE_ITEM_MAX_CHARS,
                    "{version} 的这条说明有 {} 字符，超过 {SCRIPT_UPDATE_NOTE_ITEM_MAX_CHARS}：\
                     {item:?}——弹窗宽度跟着最长一行走，写长了会把整个弹窗撑宽",
                    item.chars().count()
                );
            }
            if let Some(previous) = previous {
                assert!(
                    previous > parsed,
                    "表必须按新 → 旧排列：{version} 排在 {previous:?} 之后"
                );
            }
            previous = Some(parsed);
        }

        assert_eq!(
            SCRIPT_RELEASE_NOTES[0].0, SCRIPT_VERSION,
            "最新一版必须有自己的说明，否则 bump 之后只能弹出一句「你旧了」"
        );
    }

    /// 区间说明覆盖 `(客户端, 最新]` 的**每一条**，且保持新 → 旧。
    ///
    /// 用一张合成的表：真实的 `SCRIPT_RELEASE_NOTES` 当前只有一个条目（越界的版本
    /// 无从查证），而「用户一次跳好几版」要等第二次 bump 才会在真实表上发生——那时
    /// 再发现区间摘错了，用户已经在删音源了。
    #[test]
    fn release_notes_cover_the_whole_span_newest_first() {
        // 条目数刻意不等（0.1.5 有两条），这样「摘出来的是一整组条目」也被断言到，
        // 而不只是「首条说明没丢」。
        let table: [(&str, &[&str]); 4] = [
            ("0.1.5", &["五之一", "五之二"]),
            ("0.1.4", &["四"]),
            ("0.1.3", &["三"]),
            ("0.1.2", &["二"]),
        ];

        assert_eq!(
            release_notes_between(&table, (0, 1, 2), (0, 1, 5)),
            vec![
                ("0.1.5", &["五之一", "五之二"][..]),
                ("0.1.4", &["四"][..]),
                ("0.1.3", &["三"][..])
            ],
            "跨三版时必须把中间的都说出来，且新在前"
        );
        assert_eq!(
            release_notes_between(&table, (0, 1, 4), (0, 1, 5)),
            vec![("0.1.5", &["五之一", "五之二"][..])],
            "只差一版时只含那一版"
        );
        assert!(
            release_notes_between(&table, (0, 1, 5), (0, 1, 5)).is_empty(),
            "客户端已是最新：区间必须为空"
        );
        assert!(
            release_notes_between(&table, (0, 1, 1), (0, 1, 2))
                .iter()
                .all(|(v, _)| *v == "0.1.2"),
            "区间左开：客户端自己那一版不在说明里"
        );
    }

    /// `log` 的四段顺序：版本落差 → 动作指令 → 导入地址 → 版本说明，且地址独占一行。
    ///
    /// 顺序不是排版偏好：宿主在 1024 字符处**从尾部**截断，排在后面的先消失。地址
    /// 若排在说明后面，一份写得长的说明就能把地址挤出弹窗——而地址是这条通知里
    /// 唯一能让用户动起来的东西。独占一行则是为了让它成为一个干净的复制单位：
    /// 与动作指令挤在同一行时，那一行的长度由地址（定长、我们缩短不了）决定。
    #[test]
    fn update_log_puts_the_import_url_before_the_notes() {
        let log = update_log((0, 1, 1), (0, 1, 2), &[("0.1.2", &["这一版的说明"][..])]);

        let gap = log
            .find("当前 0.1.1 → 最新 0.1.2")
            .expect("必须给出两边的版本落差");
        let url = log
            .find("http://127.0.0.1:8787/s/st_update.js")
            .expect("必须给出可直接导入的脚本地址");
        let notes = log.find("这一版的说明").expect("必须给出版本说明");

        assert!(gap < url, "版本落差排在最前");
        assert!(url < notes, "导入地址必须排在版本说明之前");
        assert!(
            log.lines()
                .any(|line| line == "http://127.0.0.1:8787/s/st_update.js"),
            "导入地址必须独占一行，否则它不是一个能整体选中的复制单位"
        );
        assert!(
            log.contains("请先删除旧的音源条目"),
            "重新导入会多出一个条目，必须说清要先删旧的"
        );
    }

    /// 说明段的形状：抬头 → 版本标题行 → 每条说明各占一行（带 `·` 前缀）。
    ///
    /// 断言形状而不是只断言「文字在不在」，因为形状正是这个弹窗的可读性所在：宿主
    /// 那个对话框的宽度跟着**最长一行**走，把几条说明并成一行等于把弹窗撑满整个窗口。
    #[test]
    fn update_log_lists_each_note_on_its_own_line() {
        let log = update_log(
            (0, 1, 1),
            (0, 1, 2),
            &[("0.1.2", &["第一条", "第二条"][..])],
        );

        assert!(log.contains("本次更新内容："));
        assert_eq!(
            log.lines().filter(|line| *line == "- 0.1.2").count(),
            1,
            "版本号必须独占一行"
        );
        assert_eq!(
            log.lines().filter(|line| *line == "  · 第一条").count(),
            1,
            "每条说明必须独占一行"
        );
        assert_eq!(log.lines().filter(|line| *line == "  · 第二条").count(), 1);
        assert!(
            !log.contains("第一条 第二条"),
            "两条说明不得并成同一行：那正是把弹窗撑宽的做法"
        );
        assert!(
            !log.contains(SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL),
            "没有截断就不该有收束语"
        );
    }

    /// 说明装不下时：地址那一行必须活下来，说明**整行**截断并补收束语。
    ///
    /// 这是预算那一整套逻辑的验收点——它唯一的产出就是这个取舍。除了「地址还在」，
    /// 还钉住「不切句子」：每一行要么是完整的版本标题、要么是完整的条目，半句话加
    /// 收束语读起来比少一条更糟，它看起来像写坏了。
    #[test]
    fn long_notes_are_truncated_but_the_import_url_survives() {
        let items: Vec<String> = (0..40)
            .map(|i| format!("第 {i} 条说明，写得还算长一点点"))
            .collect();
        let items: Vec<&str> = items.iter().map(String::as_str).collect();
        let log = update_log((0, 1, 1), (0, 1, 2), &[("0.1.2", &items[..])]);

        assert!(
            log.contains("http://127.0.0.1:8787/s/st_update.js"),
            "说明再长，导入地址那一行也必须完整保留"
        );
        assert!(
            log.contains(SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL),
            "截断必须补收束语，否则用户会以为说明就到这里为止"
        );

        // 说明段的每一行：要么是版本标题，要么是完整条目；最后一行是收束语。
        let section: Vec<&str> = log
            .lines()
            .skip_while(|line| *line != "本次更新内容：")
            .skip(1)
            .collect();
        assert!(section.len() > 1, "说明段至少要有标题行与一条说明");
        assert_eq!(
            section.last().copied(),
            Some(SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL),
            "被截断时最后一行必须是收束语"
        );
        for line in &section[..section.len() - 1] {
            assert!(
                *line == "- 0.1.2" || line.starts_with("  · "),
                "说明段只应有版本标题行与条目行，实际：{line:?}"
            );
        }
        assert_eq!(section[0], "- 0.1.2", "版本标题行必须紧跟在抬头之后");
        let mut listed = 0usize;
        for line in &section[1..section.len() - 1] {
            let text = line
                .strip_prefix("  · ")
                .unwrap_or_else(|| panic!("抬头与收束语之间只应有条目行，实际：{line:?}"));
            assert!(
                items.contains(&text),
                "条目不得被切成半句，实际出现：{text:?}"
            );
            listed += 1;
        }
        assert!(listed > 0, "截断不该把条目全丢掉");
        assert!(listed < items.len(), "这条用例本该触发截断");

        assert!(
            log.chars().count() <= SCRIPT_UPDATE_LOG_TARGET_CHARS,
            "截断之后整段应当收在目标预算内，实际 {} 字符",
            log.chars().count()
        );
        assert!(
            log.chars().count() <= SCRIPT_UPDATE_LOG_MAX_CHARS,
            "整段不得超过硬上限"
        );
    }

    /// 说明超过硬上限时按硬上限截断，且地址仍然在。
    ///
    /// 目标预算是「应该」，硬上限是「必须」：只要有一处算错，宿主的 1024 截断就会
    /// 替我们动手，而它是从尾部砍的。
    ///
    /// 注意**正常输入下这条兜底是碰不到的**：说明那一步（见上一条测试）已经把整段
    /// 收在目标预算内了。它唯一会被触发的地方是前面两段自己就超长——离奇到不现实的
    /// `PUBLIC_BASE_URL`。因此这里分两半：一半用真实形态的输入钉住不变量，另一半
    /// 用那个病态输入把兜底**真的走一遍**（否则这段代码是死代码，写错了也没人知道）。
    #[test]
    fn update_log_never_exceeds_the_hard_cap() {
        // 宿主的截断点是 1024。把关系放进 const block 让它在编译期被证明一次。
        const { assert!(SCRIPT_UPDATE_LOG_MAX_CHARS < 1024) };

        // 一条离谱长的说明：按行截断意味着它整条装不下，于是说明段只剩版本标题行
        // 加收束语。地址与动作指令必须留下——它们是这条通知里唯一能让用户动起来的东西。
        let huge_note = "很长的说明".repeat(10_000);
        let log = update_log(
            (0, 1, 1),
            (0, 1, 2),
            &[("0.1.2", &[huge_note.as_str()][..])],
        );
        assert!(
            log.contains("http://127.0.0.1:8787/s/st_update.js"),
            "说明整条装不下时，地址那一行仍必须完整保留"
        );
        assert!(
            !log.contains(&huge_note),
            "装不下的说明整条丢掉，不得切成半句塞进去"
        );
        assert!(
            log.contains(SCRIPT_UPDATE_NOTES_TRUNCATED_TAIL),
            "丢掉说明就说清有省略，不能让用户以为这一版只改了一件事"
        );
        assert!(
            log.chars().count() <= SCRIPT_UPDATE_LOG_MAX_CHARS,
            "实际 {} 字符",
            log.chars().count()
        );

        // 病态输入：连版本标题行都放不进预算时，说明段连抬头一起消失（下面空无一物
        // 的抬头比没有抬头更怪）。`PUBLIC_BASE_URL` 长到这个地步才会走到这里。
        let absurd_base = "a".repeat(3_000);
        let clamped = render_update_log(
            &absurd_base,
            "st_update",
            (0, 1, 1),
            (0, 1, 2),
            &[("0.1.2", &["说明"][..])],
        );
        assert!(
            !clamped.contains("本次更新内容："),
            "一条都放不下时不该留一个下面空无一物的抬头"
        );
        assert!(
            clamped.chars().count() <= SCRIPT_UPDATE_LOG_MAX_CHARS,
            "兜底截断必须真的生效，实际 {} 字符",
            clamped.chars().count()
        );
    }

    /// 无更新时的信封是且仅是 `{"update": false}`。
    #[tokio::test]
    async fn up_to_date_script_gets_a_bare_no_update_envelope() {
        let service = update_service(None, Some(update_source("tok_current", true)));

        let resp = service
            .check_script_update("tok_current", SCRIPT_VERSION)
            .await
            .expect("版本相同应当正常返回");
        assert!(!resp.update);
        let value = response_json(&resp);
        assert_eq!(value["update"], false);
        assert!(
            value.get("log").is_none(),
            "没有更新就不该有 log 键，实际 {:?}",
            value.get("log")
        );
        assert!(
            value.get("update_url").is_none(),
            "没有更新就不该有 update_url 键"
        );
    }

    /// 有更新、且后台没有 web 产物时：`update_url` 这个键**整个消失**。
    ///
    /// 不给 `null` 是刻意的：客户端要区分的是「服务端有没有给我地址」，两种表述会
    /// 变成两条要解释的路径，而我们确实没有这个地址。
    #[tokio::test]
    async fn update_without_web_dist_omits_the_update_url_key() {
        let service = update_service(None, Some(update_source("tok_old", true)));

        let resp = service
            .check_script_update("tok_old", "0.1.1")
            .await
            .expect("旧版本应当正常返回");
        assert!(resp.update);

        let value = response_json(&resp);
        assert_eq!(value["update"], true);
        let log = value["log"].as_str().expect("有更新就必须给 log");
        assert!(
            log.contains("http://127.0.0.1:8787/s/st_update.js"),
            "log 里必须带上可直接导入的地址，实际：{log}"
        );
        assert!(
            value.get("update_url").is_none(),
            "没有 web 产物时这个键必须整个消失，而不是给一个 null"
        );
    }

    /// 有 web 产物时给出后台音源详情页的地址——那里有可复制的脚本地址。
    #[tokio::test]
    async fn update_with_web_dist_includes_the_source_page() {
        let service = update_service(
            Some(PathBuf::from("./web/build")),
            Some(update_source("tok_old", true)),
        );

        let resp = service
            .check_script_update("tok_old", "0.1.1")
            .await
            .expect("旧版本应当正常返回");
        assert_eq!(
            resp.update_url.as_deref(),
            Some("http://127.0.0.1:8787/sources/src_update")
        );
        assert_eq!(
            response_json(&resp)["update_url"],
            "http://127.0.0.1:8787/sources/src_update"
        );
    }

    /// **被停用的音源照样收到通知。**
    ///
    /// 这条通知讲的是「你手上那份脚本旧了」，与音源启不启用无关：停用是暂时的，
    /// 用户会在某天把它启回来，而那时他手上的仍然是旧脚本。顺手抄一份 `music-url`
    /// 的 enabled 预检，会让停用期的用户永远收不到通知，而这恰恰是没有任何别的
    /// 渠道能补救的（洛雪音乐从不自动更新已导入的脚本）。
    #[tokio::test]
    async fn disabled_source_still_gets_the_update_notice() {
        let service = update_service(None, Some(update_source("tok_disabled", false)));

        let resp = service
            .check_script_update("tok_disabled", "0.1.1")
            .await
            .expect("停用不该成为这条通知的前置条件");
        assert!(resp.update, "停用的音源也必须被告知脚本已经旧了");
    }

    /// 未知 runtime token：与 `music-url` 一致，`SOURCE_NOT_FOUND`。
    #[tokio::test]
    async fn unknown_runtime_token_is_source_not_found() {
        let service = update_service(None, None);

        // `ScriptUpdateResponse` 没有 `Debug`（它只是个出网用的 DTO），用不了
        // `expect_err`；照 `services/runtime.rs` 的老做法显式匹配。
        let err = match service.check_script_update("tok_unknown", "0.1.1").await {
            Ok(resp) => panic!("未知 token 必须失败，实际返回 update={}", resp.update),
            Err(err) => err,
        };
        assert_eq!(err.code, ErrorCode::SourceNotFound);
    }

    /// 畸形客户端版本即使在音源查得到时也不弹窗。
    ///
    /// 与 `malformed_versions_never_trigger_an_update` 的区别是它走的是**整条服务
    /// 方法**：那条测的是比较函数，这条证明的是「无更新」这个答案真的能构造出来
    /// （而不是在某个分支上 panic 或返回一个半成品信封）。
    #[tokio::test]
    async fn malformed_client_version_yields_a_no_update_envelope() {
        let service = update_service(None, Some(update_source("tok_bad", true)));

        let resp = service
            .check_script_update("tok_bad", "手改过的版本号")
            .await
            .expect("畸形版本不是错误，只是「没有更新」");
        assert!(!resp.update);
        assert_eq!(response_json(&resp)["update"], false);
    }
}
