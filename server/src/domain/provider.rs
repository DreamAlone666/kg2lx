use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    KugouLite,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::KugouLite => write!(f, "kugou_lite"),
        }
    }
}

impl ProviderKind {
    /// 面向用户的平台展示名，用于脚本 `sources[].name`。
    ///
    /// 这是**平台级**归属（与官方示例里的「酷我音乐」「本地音乐」同一层级），
    /// 而非某个具体实例的标题——后者由用户在后台逐源编辑并持久化。两者默认值
    /// 同为「酷狗概念版」纯属巧合，不可互相替代：一份脚本可以声明多个平台源，
    /// 但头部 `@name` 只有一个，让平台名跟着某个实例的标题走，
    /// 会让「接入第二个平台」立刻变成 bug。
    ///
    /// 也不同于 [`fmt::Display`] 输出的 `kugou_lite`——后者是持久化/协议用的
    /// 稳定标识，不能拿来展示。
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderKind::KugouLite => "酷狗概念版",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 平台展示名是固定值，不得由实例标题驱动；同时与 Display 的协议标识区分开。
    #[test]
    fn display_name_is_platform_level_and_stable() {
        assert_eq!(ProviderKind::KugouLite.display_name(), "酷狗概念版");
        assert_eq!(ProviderKind::KugouLite.to_string(), "kugou_lite");
        assert_ne!(
            ProviderKind::KugouLite.display_name(),
            ProviderKind::KugouLite.to_string(),
            "platform display name must not be reused as the protocol identifier"
        );
    }
}
