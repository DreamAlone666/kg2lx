use std::time::{SystemTime, UNIX_EPOCH};

use super::provider::ProviderKind;

/// 音源标题的建议长度上限（字符数）。
///
/// 官方原文：「建议不要过长，24 个字符以内」。注意这是**建议**而非约束：
/// 超长不会导致导入失败，因此该常量只用于后台计数与提示，
/// 绝不参与任何拒绝写入的逻辑——否则一个语义完整但略长的名字
/// 会被无理由拒绝。
pub const SCRIPT_NAME_ADVISORY_CHARS: usize = 24;

/// 音源描述的建议长度上限（字符数）。
///
/// 官方原文：「建议不要过长，36 个字符以内」。语义同
/// [`SCRIPT_NAME_ADVISORY_CHARS`]，同样只是建议。
///
/// 与标题不同，描述由服务端独占生成、用户改不了，因此当前没有任何消费方
/// （后台没有可供用户计数的输入框）。这里保留它，只是因为它是官方约束的
/// 单一事实来源，删掉会让这个数字散落在注释里。刻意**不**把它挂到 API
/// 响应上——那会给前端一个永远不会有消费者的字段，白白变成长期兼容包袱。
#[allow(dead_code)]
pub const SCRIPT_DESC_ADVISORY_CHARS: usize = 36;

/// 标题的技术护栏（字节数上限），与 UX 规范无关。
///
/// `name` 会出现在**每一次** `/s/{token}.js` 响应里，极端超长的输入会明显
/// 撑大脚本体积。该上限只在写入路径（PATCH）生效并返回 400；渲染路径
/// **永不拒绝、也不截断**，否则一个超长的 `SOURCE_NAME_PREFIX` 会让脚本
/// URL 直接不可用。
pub const SCRIPT_NAME_MAX_BYTES: usize = 256;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub source_id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub name: String,
    pub enabled: bool,
    pub script_token: String,
    pub runtime_token: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// 清洗即将写入脚本头部注释的元信息文本。
///
/// 两条硬性要求，且**顺序不可颠倒**：
/// 1. 过滤控制字符（保留空白类）并折叠连续空白：不可见字符会污染脚本与日志，
///    换行则会破坏注释结构与元信息解析（`@name` 必须是单行）。
/// 2. 消除 `*/`：它会**提前闭合注释块**，导致 LX 导入即语法错误。
///
/// 必须先过滤控制字符、再消除 `*/`。反过来做会漏：`"*<DEL>/"` 里并不存在
/// `*/`，先删 `*/` 会原样放行，随后的控制字符过滤才把两侧拼到一起，输出仍是
/// `*/`。先过滤则 `"*<DEL>/"` 先变成 `"*/"`，再被正确消除。
///
/// 输出因此**既不含控制字符、也不含 `*/`**，函数是幂等的：写入侧清洗过的值
/// 在渲染侧再洗一次结果不变。这一点是刚需——否则存进 JSON 的标题与脚本里
/// 实际渲染的标题会不一致（曾出现存储 `*/`、脚本却渲染成空标题）。
pub fn sanitize_script_meta(input: &str) -> String {
    // 第一步：过滤控制字符，保留空白类交给第三步折叠。
    let printable: String = input
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect();

    // 第二步：消除 `*/`。`/` 紧跟在一个已写出的 `*` 后面时，两个字符一起丢掉。
    // 这等价于「反复删除直到不动点」，能处理 `**//` 删完又拼出一个 `*/` 的情况，
    // 且是单遍 O(n)——反复 `replace` 在 `***///` 这类输入上会退化成 O(n^2)。
    let mut out = String::with_capacity(printable.len());
    for c in printable.chars() {
        if c == '/' && out.ends_with('*') {
            out.pop();
        } else {
            out.push(c);
        }
    }

    // 第三步：折叠连续空白。只把空白收成单个空格，不会让两个非空白字符相邻，
    // 因此不可能在这一步重新拼出 `*/`。
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 生成音源的默认标题：`<prefix> <userid>`。
///
/// 由「创建音源」与「恢复默认名」共用，避免两处 `format!` 各自演化出
/// 不一致的格式。两侧都会 trim，因此 `userid` 为空时结果不带尾随空格——
/// 后台会原样展示该字符串，`"酷狗概念版 "` 这类文案会显得像 bug。
pub fn default_name(prefix: &str, userid: &str) -> String {
    let prefix = prefix.trim();
    let userid = userid.trim();
    match (prefix.is_empty(), userid.is_empty()) {
        (true, true) => String::new(),
        (true, false) => userid.to_string(),
        (false, true) => prefix.to_string(),
        (false, false) => format!("{prefix} {userid}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 防「删完又拼出」：`**//` 删掉中间那组 `*/` 后，首尾的 `*` 与 `/`
    /// 会新拼出一个 `*/`，注释仍会被提前闭合。必须循环删到干净为止。
    #[test]
    fn sanitize_removes_star_slash_even_when_reformed() {
        for input in ["**//", "*/", "*/*/", "a*/b", "**/**//"] {
            let out = sanitize_script_meta(input);
            assert!(
                !out.contains("*/"),
                "sanitized output must never contain `*/` (input {input:?}, got {out:?})"
            );
        }
    }

    /// `@name` 必须单行。\t \n \r 属空白，应被折叠为单个空格而不是删除，
    /// 否则 `a\tb` 会静默变成 `ab`。
    #[test]
    fn sanitize_collapses_whitespace_to_single_spaces() {
        assert_eq!(sanitize_script_meta("a\tb"), "a b");
        assert_eq!(sanitize_script_meta("a\n\nb"), "a b");
        assert_eq!(sanitize_script_meta("  a  \r\n  b  "), "a b");
        assert_eq!(sanitize_script_meta("a\u{7}b"), "ab");
    }

    /// 纯空白输入清洗后为空串，写入侧据此判定「非空但清洗后为空」并返回 400。
    #[test]
    fn sanitize_blank_input_yields_empty() {
        assert_eq!(sanitize_script_meta(""), "");
        assert_eq!(sanitize_script_meta("   \t\n  "), "");
        assert_eq!(sanitize_script_meta("*/"), "");
    }

    /// 控制字符过滤必须排在 `*/` 消除**之前**。`"*<DEL>/"` 里本来不存在 `*/`，
    /// 若先删 `*/` 就会原样放行，随后的控制字符过滤才把两者拼成 `*/`，
    /// 注释依然会被提前闭合。这是曾经真实存在于本函数里的顺序 bug。
    #[test]
    fn sanitize_removes_control_chars_before_star_slash() {
        for ctrl in ['\u{7}', '\u{1b}', '\u{7f}', '\u{0}'] {
            let input = format!("*{ctrl}/");
            let out = sanitize_script_meta(&input);
            assert!(
                !out.contains("*/"),
                "a control char must not be able to splice a `*/` \
                 (input {input:?}, got {out:?})"
            );
            assert_eq!(out, "", "input {input:?} must sanitize to empty");
        }
    }

    /// 幂等：写入侧清洗过的值，渲染侧再洗一次结果必须不变。否则存进 JSON 的标题
    /// 与脚本里实际渲染的标题会分叉（曾出现存储 `*/`、脚本却渲染成空标题）。
    #[test]
    fn sanitize_is_idempotent() {
        for input in [
            "**//",
            "*\u{7}/",
            "a*/b",
            "  x  y  ",
            "*/",
            "* /",
            "正常标题 12345678",
        ] {
            let once = sanitize_script_meta(input);
            let twice = sanitize_script_meta(&once);
            assert_eq!(once, twice, "sanitize must be idempotent for {input:?}");
        }
    }

    /// 空白折叠不能成为新的拼接途径：`*` 与 `/` 之间隔着的空白必须保留为
    /// 单个空格，而不是被吃掉后拼出 `*/`。
    #[test]
    fn sanitize_never_splices_across_collapsed_whitespace() {
        for input in ["* /", "*   /", "*\t/", "*\n/"] {
            let out = sanitize_script_meta(input);
            assert!(
                !out.contains("*/"),
                "whitespace collapse must not splice `*/` (input {input:?}, got {out:?})"
            );
            assert_eq!(out, "* /", "input {input:?} must keep the separating space");
        }
    }

    /// userid 为空时不能产生「酷狗概念版 」这类带尾随空格的文案。
    #[test]
    fn default_name_has_no_trailing_space_when_userid_missing() {
        assert_eq!(default_name("酷狗概念版", "12345678"), "酷狗概念版 12345678");
        assert_eq!(default_name("酷狗概念版", ""), "酷狗概念版");
        assert_eq!(default_name("", "12345678"), "12345678");
        assert_eq!(default_name("", ""), "");
        assert_eq!(default_name("酷狗概念版", "  "), "酷狗概念版");
    }
}
