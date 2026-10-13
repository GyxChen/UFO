//! 窗口 devtools 的开关：由环境变量 `UFO_DEVTOOLS` 控制。
//!
//! 原先调试构建下三个窗口（main / floating / tooltip）一律自动打开 devtools，
//! 想关掉只能去 `lib.rs` 把那几行注释掉，来回改代码很别扭。现在改成显式环境变量：
//! **默认全关**，需要调试哪个窗口就点名哪个。
//!
//! 取值（逗号或空格分隔，不区分大小写）：
//! - 空 / `0` / `false` / `off` / `none` → 一个都不开（默认）
//! - `all` / `1` / `true` / `on`        → 所有窗口全开（含运行时才建出来的独立面板窗）
//! - 其它按窗口 label 处理，如 `floating,tooltip` → 只开这两个
//!
//! 注意这里管的是 **webview 的 devtools**；前端的 Vue devtools 是另一套开关（`VITE_DEVTOOLS`）。

/// 控制 devtools 的环境变量名。
pub const ENV_KEY: &str = "UFO_DEVTOOLS";

/// 本次启动要打开 devtools 的窗口集合。
///
/// 变体不写成 enum 是因为「全部窗口」与「若干个具名窗口」在用法上同构，
/// 判定的只有 [`DevtoolsTargets::wants`] 一处。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DevtoolsTargets {
    /// 是否为「全部窗口」；为 true 时忽略 `labels`。
    all: bool,
    /// 需要打开的窗口 label（小写化后存放），仅在 `all == false` 时有意义。
    labels: Vec<String>,
}

impl DevtoolsTargets {
    /// 从环境变量读出本次的目标窗口集合；未设置或取值无效时返回空集合（全关）。
    ///
    /// 取值有两处来源，按优先级：
    /// 1. 进程环境变量 —— 临时 `UFO_DEVTOOLS=floating pnpm tauri dev` 立刻生效；
    /// 2. 编译期常量 —— `src-tauri/build.rs` 从仓库根 `.env` 读出后注入，
    ///    方便把配置写进 `.env` 一劳永逸（代价是改完要重新构建）。
    pub fn from_env() -> Self {
        let raw = std::env::var(ENV_KEY)
            .ok()
            .or_else(|| option_env!("UFO_DEVTOOLS").map(str::to_string))
            .unwrap_or_default();
        Self::parse(&raw)
    }

    /// 解析环境变量取值文本，得到目标窗口集合。
    ///
    /// 逗号与空白都当分隔符，因此 `main, tooltip` 与 `main tooltip` 等价。
    /// 无法识别的 token 一律按窗口 label 收下：窗口 label 是开放的（将来可能加窗口），
    /// 这里不必也不该内置一份 label 白名单，判定交给 [`DevtoolsTargets::wants`]。
    pub fn parse(raw: &str) -> Self {
        let mut targets = Self::default();
        for token in raw.split([',', ' ', '\t', '\r', '\n']) {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            match token.to_ascii_lowercase().as_str() {
                // 全开
                "all" | "1" | "true" | "on" | "yes" => targets.all = true,
                // 显式关闭：与默认值一致，只是让 `UFO_DEVTOOLS=false` 这类写法不被当成窗口名。
                "0" | "false" | "off" | "no" | "none" => {}
                other => targets.labels.push(other.to_string()),
            }
        }
        targets
    }

    /// 判断 `label` 这个窗口是否需要打开 devtools。
    pub fn wants(&self, label: &str) -> bool {
        self.all
            || self
                .labels
                .iter()
                .any(|item| item.eq_ignore_ascii_case(label))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言三个窗口一个都不开 —— 用于「默认全关」这类用例。
    fn assert_opens_nothing(targets: &DevtoolsTargets, raw: &str) {
        for label in ["main", "floating", "tooltip"] {
            assert!(!targets.wants(label), "{raw:?} should not open {label}");
        }
    }

    /// 未设置 / 空值时全关 —— 这是默认行为，必须锁住（否则 devtools 又会自己弹出来）。
    #[test]
    fn empty_value_opens_nothing() {
        assert_opens_nothing(&DevtoolsTargets::parse(""), "");
    }

    /// `false` / `0` 等显式关闭写法等同于不设置，不能被当成窗口名。
    #[test]
    fn off_tokens_open_nothing() {
        for raw in ["false", "0", "off", "OFF", " none "] {
            assert_opens_nothing(&DevtoolsTargets::parse(raw), raw);
        }
    }

    /// 全开写法：三个窗口都命中。
    #[test]
    fn on_tokens_open_every_window() {
        for raw in ["1", "true", "all", "ON"] {
            let targets = DevtoolsTargets::parse(raw);
            assert!(targets.wants("main"), "{raw} should open main");
            assert!(targets.wants("floating"), "{raw} should open floating");
            assert!(targets.wants("tooltip"), "{raw} should open tooltip");
        }
    }

    /// 只点名一个窗口时，其余窗口不受影响。
    #[test]
    fn single_label_opens_only_that_window() {
        let targets = DevtoolsTargets::parse("floating");
        assert!(targets.wants("floating"));
        assert!(!targets.wants("main"));
        assert!(!targets.wants("tooltip"));
    }

    /// 多个 label 允许逗号 / 空格混用，且大小写不敏感。
    #[test]
    fn labels_accept_spaces_and_case_insensitively() {
        let targets = DevtoolsTargets::parse(" MAIN, Tooltip ");
        assert!(targets.wants("main"));
        assert!(targets.wants("tooltip"));
        assert!(!targets.wants("floating"));
    }
}
