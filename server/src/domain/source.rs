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
/// **永不拒绝、也不截断**，否则一个超长的 `Source.name` 会让脚本
/// URL 直接不可用。
pub const SCRIPT_NAME_MAX_BYTES: usize = 256;

/// 音源默认标题的**实例级**前缀。
///
/// 与 [`ProviderKind::display_name`] 的**平台级**展示名是两个层级，两者当前
/// 恰好同为「酷狗概念版」，但必须彼此独立、不可互相复用——理由见
/// `src/domain/provider.rs`：一份脚本可以声明多个平台源，共用同一字符串会让
/// 「接入第二个平台」立刻变成 bug。
///
/// 为什么不直接用账号的 `userid`：账号记录可能缺失，此时 `userid` 是空串，
/// 拼出的标题也是空串。写入路径拒绝空标题，「恢复默认名」会因此失败。常量
/// 保证默认标题非空，是这条恢复路径的兜底。
///
/// 它曾经是可配置的 `SOURCE_NAME_PREFIX`。改成常量，一是全局配置决定每个
/// 实例的标题属于层级错配，二是每次读取都从环境变量重算会让 `default_name`
/// 随部署漂移：已存在的音源标题会与其「默认值」对不上。
pub const INSTANCE_NAME_PREFIX: &str = "酷狗概念版";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Source {
    pub source_id: String,
    pub account_id: String,
    pub provider: ProviderKind,
    pub name: String,
    pub script_token: String,
    pub runtime_token: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// 音质自动降级开关，**默认关闭**。
    ///
    /// 开着时，上游明确拒绝（`status:2`）用户请求的档位后，服务端会沿 LX 的档位
    /// 阶梯（`flac24bit → flac → 320k → 128k`）向下改请求，尽量让用户听到东西，
    /// 而不是整首播不了。关着则维持原有行为：把拒绝原样抛回去。
    ///
    /// **为什么默认关**：这是「宁可失败也不给低档」与「宁可给低档也别播不了」之间的
    /// 取舍，属于用户偏好，不是显然更优的默认值。保守默认意味着「不开就与改动前完全
    /// 同行为」。反过来则不然：LX 客户端在播放链路里不渲染音质，默认打开会让静默降级
    /// 大面积生效而无人察觉。
    ///
    /// **下载同样生效，且这一点无法规避**：客户端脚本拿到的 `info` 只有 `type` 与
    /// `musicInfo`，**无法区分**本次是播放还是下载，因此服务端不可能只对播放降级。
    /// 用户开启这个开关，就等于一并接受「下载会拿到比所选档位更低的文件」。客户端
    /// 仍按**请求时**的档位记录与缓存该地址。
    ///
    /// `#[serde(default)]` 是必需的，不是风格问题：音源以「一源一 JSON 文件」落盘，
    /// 而 `FsSourceRepo` 对解析失败的行是 `Err(_) => continue`——少了它，本次改动
    /// **之前**写下的每一份音源文件都会整份读不出来，runtime token 解析不到、脚本
    /// 端点 404，且几乎没有日志。字段读出的 `false` 此时精确对应「用户没开过这个
    /// 开关」，与显式写入的 `false` 同义，无需区分。
    #[serde(default)]
    pub quality_fallback: bool,
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

/// 生成音源的默认标题：`<instance prefix> <userid>`。
///
/// 由「创建音源」与「恢复默认名」共用，避免两处 `format!` 各自演化出
/// 不一致的格式。`userid` 先 trim，空则只留前缀，因此结果不带尾随空格——
/// 后台会原样展示该字符串，`"酷狗概念版 "` 这类文案会显得像 bug。
///
/// 前缀是常量 [`INSTANCE_NAME_PREFIX`]，非空，因此结果只可能是
/// 「前缀」或「前缀 + 空格 + userid」，不存在空串分支。
pub fn default_name(userid: &str) -> String {
    let userid = userid.trim();
    if userid.is_empty() {
        INSTANCE_NAME_PREFIX.to_string()
    } else {
        format!("{INSTANCE_NAME_PREFIX} {userid}")
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

    /// 正常情形：实例前缀与 userid 以单个空格连接。
    #[test]
    fn default_name_joins_prefix_and_userid() {
        assert_eq!(default_name("12345678"), "酷狗概念版 12345678");
        assert_eq!(default_name("  12345678  "), "酷狗概念版 12345678");
    }

    /// userid 为空时不能产生「酷狗概念版 」这类带尾随空格的文案。
    #[test]
    fn default_name_has_no_trailing_space_when_userid_missing() {
        assert_eq!(default_name(""), "酷狗概念版");
        assert_eq!(default_name("  "), "酷狗概念版");
    }

    /// 本次改动**之前**落盘的音源 JSON 没有 `quality_fallback` 这个键。
    ///
    /// fixture 里那句 `"enabled": true` 是**刻意留着的**：`Source.enabled` 已整体删除，
    /// 而每一个历史音源文件里都写着这个键。它留在这里，是为了让「删掉的键不会让旧文件
    /// 读不出来」这件事每次跑测试都被真实验证一遍，而不是靠「serde 默认忽略未知字段」
    /// 这句话本身。
    ///
    /// 缺了 `#[serde(default)]`，它会整份反序列化失败，而 `FsSourceRepo` 用
    /// `Err(_) => continue` 吞掉错误——后果是**现存音源全部消失**：runtime token
    /// 解析不到、脚本端点 404，且几乎没有日志。这条断言钉住向后兼容，
    /// fixture 的键集照落盘形态构造，取值一律是合成占位。
    #[test]
    fn reads_source_files_written_before_quality_fallback_existed() {
        let old_source = r#"{
  "source_id": "src_00000000-0000-4000-8000-000000000001",
  "account_id": "acc_00000000-0000-4000-8000-000000000000",
  "provider": "kugou_lite",
  "name": "kg2lx demo",
  "enabled": true,
  "script_token": "st_00000000-0000-0000-0000-000000000000",
  "runtime_token": "rt_00000000-0000-0000-0000-000000000000",
  "created_at": 1600000000,
  "updated_at": 1700100000
}"#;

        // 断言整份 struct 都能读出来，而不是只挑新字段看：任何一个字段出问题，
        // 真实的 `FsSourceRepo` 都会把这一份文件整个跳过。
        let source: Source =
            serde_json::from_str(old_source).expect("本次改动之前落盘的音源文件必须仍能整份读入");
        assert!(
            !source.quality_fallback,
            "老音源读出 false，语义是「用户没开过这个开关」，不是「开关被显式关掉」"
        );
        assert_eq!(source.source_id, "src_00000000-0000-4000-8000-000000000001");
        assert_eq!(source.provider, ProviderKind::KugouLite, "其余字段不受影响");
        assert_eq!(source.name, "kg2lx demo");
        assert_eq!(source.created_at, 1600000000);
        assert_eq!(source.updated_at, 1700100000);
    }

    /// 新值能原样写回读出，且序列化文本里确实带这个键。
    #[test]
    fn round_trips_quality_fallback() {
        let source = Source {
            source_id: "src_x".into(),
            account_id: "acc_x".into(),
            provider: ProviderKind::KugouLite,
            name: "测试音源".into(),
            script_token: "st_x".into(),
            runtime_token: "rt_x".into(),
            created_at: 1_776_414_582,
            updated_at: 1_789_832_921,
            quality_fallback: true,
        };

        let json = serde_json::to_string(&source).expect("serialize");
        assert!(
            json.contains(r#""quality_fallback":true"#),
            "开关必须真正落盘，否则重启后悄悄退回默认关：{json}"
        );

        let back: Source = serde_json::from_str(&json).expect("deserialize");
        assert!(back.quality_fallback);
        assert_eq!(back.source_id, "src_x");
    }
}
