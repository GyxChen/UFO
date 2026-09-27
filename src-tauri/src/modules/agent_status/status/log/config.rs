//! 转灯日志自己的配置：`module_settings['agent-status'].log` 子块。
//!
//! 照 [`crate::modules::agent_status::push::config::PushConfig`] 的**嵌套子块**范式：
//! 外壳与 `settings.rs` 都不解释这些字段，本模块自己从整块 JSON 里取 `log` 再解析。
//! 逐字段宽松取值（不用 serde 整块反序列化）：手改配置写坏一个值只该回落那一个字段。
//!
//! ⚠️ 前端有一份同口径的默认值与取值范围（`src/modules/agent-status/settings.ts`），
//! 两边必须一致 —— 否则「面板显示 14 天、实际留 30 天」。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 记录到多细。**默认 `Off`**：这套日志是主动排障工具，不是常开的黑匣子 ——
/// 明细级会落提问摘录，绝不允许「用户没碰开关就往盘上写用户文本」。
/// 设置页的那一个 switch 就是这一档的开关：开 = `Verbose` + hook 侧同记，关 = `Off`。
///
/// 档位是「包含」关系：`Verbose` 涵盖 `Normal` 涵盖 `Minimal`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// 完全关闭（连灯变都不记）。
    Off,
    /// 只记灯态变化与清理：体积最小，够回答「这盏灯什么时候变成什么颜色」。
    Minimal,
    /// 裁决级：外加每一次「为什么没转灯」的拒绝理由（含 hook 侧的静默忽略）。
    Normal,
    /// 明细级：外加 rollout 逐行记账，以及提问 / 审批理由等**用户文本的截断摘录**。
    ///
    /// ⚠️ 开启前必须在 UI 上写明风险：提问可能含密钥或私有代码，落到日志后
    /// 保留期是按天算的（不像 inbox 那样 500ms 就被消费掉），且会随诊断包一起被导出。
    Verbose,
}

impl LogLevel {
    /// 解析配置里的字符串；认不出的一律回落 [`LogLevel::Normal`]（写错档位不该变成写错开关）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => Some(Self::Off),
            "minimal" | "brief" => Some(Self::Minimal),
            "normal" | "decision" => Some(Self::Normal),
            "verbose" | "detail" | "full" => Some(Self::Verbose),
            _ => None,
        }
    }

    /// 该档位是否包含 `needed` 这一档要记的东西。
    pub fn covers(self, needed: Self) -> bool {
        self >= needed
    }
}

/// 整份日志配置。
///
/// ⚠️ 没有独立的总开关：`level` 里的 `Off` 就是关。原先另有一个 `enabled`，与
/// 「关闭」档位是同一条件的两个入口（效果完全一致、界面上还互相遮蔽），
/// 留着一个只会让人猜「这两个哪个说了算」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogConfig {
    /// 详细档位；`Off` = 彻底不写。
    pub level: LogLevel,
    /// hook 侧单独开关：极端场景（在意那一次 open/write）可以只关 hook，保留主进程的账。
    pub hook_enabled: bool,
    /// 旧日志保留天数（按**整天文件**回收，当天的永不删）。
    pub retention_days: u32,
    /// 单文件体积上限：写满就往后开一个新序号文件。
    pub max_file_bytes: u64,
    /// 目录总量上限：超了从最旧的整天开始删。
    pub max_total_bytes: u64,
    /// 启动时从活动文件末尾读多少字节做「历史播种」。
    pub live_tail_bytes: u64,
    /// 内存环形缓冲容量：面板与命令只看这份，不做全量驻留。
    pub ring_capacity: u32,
}

// 默认关：设置页只有**一个 switch**（关 = 完全不记，开 = 明细级 + hook 侧也记）。
// ⚠️ 这两个默认值必须一起是「关」，否则全新安装时 switch 读起来是「关」而档位其实是 normal，
// 用户以为没在记、盘上却在写（或反过来）。明细级会落提问摘录，不能让它在没人拨开关的情况下发生。
const DEFAULT_LEVEL: LogLevel = LogLevel::Off;
// 认不出的档位值回落「裁决级」而不是「关」：能写出非法 `level` 的只有手改配置的人，
// 他的意图明显是「要记」—— 静默变成不记比静默变成少记一档更难查（写错的字段只影响它自己）。
const FALLBACK_LEVEL: LogLevel = LogLevel::Normal;
const DEFAULT_HOOK_ENABLED: bool = false;
const DEFAULT_RETENTION_DAYS: u32 = 14;
const MIN_RETENTION_DAYS: u32 = 1;
const MAX_RETENTION_DAYS: u32 = 90;
const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;
const DEFAULT_MAX_FILE_BYTES: u64 = 8 * MIB;
const MIN_MAX_FILE_BYTES: u64 = 64 * KIB;
const MAX_MAX_FILE_BYTES: u64 = 64 * MIB;
const DEFAULT_MAX_TOTAL_BYTES: u64 = 96 * MIB;
const MIN_MAX_TOTAL_BYTES: u64 = MIB;
const MAX_MAX_TOTAL_BYTES: u64 = 512 * MIB;
const DEFAULT_LIVE_TAIL_BYTES: u64 = 256 * KIB;
const MIN_LIVE_TAIL_BYTES: u64 = 64 * KIB;
const MAX_LIVE_TAIL_BYTES: u64 = 4 * MIB;
const DEFAULT_RING_CAPACITY: u64 = 5_000;
const MIN_RING_CAPACITY: u64 = 500;
const MAX_RING_CAPACITY: u64 = 50_000;

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: DEFAULT_LEVEL,
            hook_enabled: DEFAULT_HOOK_ENABLED,
            retention_days: DEFAULT_RETENTION_DAYS,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            live_tail_bytes: DEFAULT_LIVE_TAIL_BYTES,
            ring_capacity: DEFAULT_RING_CAPACITY as u32,
        }
    }
}

impl LogConfig {
    /// 从 `module_settings['agent-status']` 里取 `log` 子块并解析（缺块 = 全默认）。
    pub fn from_block(module_block: Option<&Value>) -> Self {
        let Some(raw) = module_block.and_then(|block| block.get("log")) else {
            return Self::default();
        };
        let defaults = Self::default();
        Self {
            // ⚠️ 两个「回落」是分开的：没有 log 块 = 没人打开过开关 = Off；
            // 块里写了个认不出的 level = 有人想记但写错了 → 落到裁决级（不含用户文本），
            // 而不是静默变成「什么都没记」。
            level: read_string(raw.get("level"))
                .and_then(|value| LogLevel::parse(&value))
                .unwrap_or(FALLBACK_LEVEL),
            hook_enabled: read_bool(raw.get("hook_enabled"), defaults.hook_enabled),
            retention_days: read_u64(raw.get("retention_days"), u64::from(defaults.retention_days))
                .clamp(u64::from(MIN_RETENTION_DAYS), u64::from(MAX_RETENTION_DAYS)) as u32,
            max_file_bytes: read_u64(raw.get("max_file_bytes"), defaults.max_file_bytes)
                .clamp(MIN_MAX_FILE_BYTES, MAX_MAX_FILE_BYTES),
            max_total_bytes: read_u64(raw.get("max_total_bytes"), defaults.max_total_bytes)
                .clamp(MIN_MAX_TOTAL_BYTES, MAX_MAX_TOTAL_BYTES),
            live_tail_bytes: read_u64(raw.get("live_tail_bytes"), defaults.live_tail_bytes)
                .clamp(MIN_LIVE_TAIL_BYTES, MAX_LIVE_TAIL_BYTES),
            ring_capacity: read_u64(raw.get("ring_capacity"), u64::from(defaults.ring_capacity))
                .clamp(MIN_RING_CAPACITY, MAX_RING_CAPACITY) as u32,
        }
    }

    /// 现在到底写不写：`level = off` 是唯一的关（没有第二个开关）。
    pub fn writes(&self) -> bool {
        self.level != LogLevel::Off
    }

    /// 是否记 `needed` 这一档要记的内容（含「关到 off」的短路）。
    pub fn covers(&self, needed: LogLevel) -> bool {
        self.writes() && self.level.covers(needed)
    }

    /// hook 侧写不写：在档位之上再过一道单独开关（那是最在意耗时的路径）。
    pub fn hook_writes(&self) -> bool {
        self.writes() && self.hook_enabled
    }

    /// 是否换成了一份新配置（用来决定要不要写一条 `ConfigReloaded`）。
    pub fn differs(&self, previous: &Self) -> bool {
        self != previous
    }
}

/// 只认真正的布尔值；写错类型（如字符串 `"false"`）回落默认，不做隐式转换。
fn read_bool(value: Option<&Value>, fallback: bool) -> bool {
    value.and_then(Value::as_bool).unwrap_or(fallback)
}

/// 取一个整数；缺失或类型不对回落默认（后续统一 clamp）。
fn read_u64(value: Option<&Value>, fallback: u64) -> u64 {
    value.and_then(Value::as_u64).unwrap_or(fallback)
}

/// 取一个非空字符串（自动 trim）。
fn read_string(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_block_falls_back_to_every_default() {
        assert_eq!(LogConfig::from_block(None), LogConfig::default());
        assert_eq!(
            LogConfig::from_block(Some(&json!({ "proxy_resolver": "cc-switch" }))),
            LogConfig::default(),
            "同级的其它字段不该被当成日志配置"
        );
    }

    #[test]
    fn nothing_is_recorded_until_the_switch_is_turned_on() {
        // 默认关的两个理由：① 设置页那一个 switch 的「打开」= 明细级 + hook 同记，
        // 默认值若不是 Off，switch 读数与真实档位就对不上；② 明细级会落提问摘录，
        // 不能让「没人碰过开关」的机器上就出现用户文本。
        // 代价是明知故犯：不先打开就没有现场 —— 这是主动排障工具，不是黑匣子。
        let config = LogConfig::default();
        assert!(!config.writes(), "默认就是不写");
        assert!(!config.hook_writes(), "hook 侧也默认不写");
        assert_eq!(config.level, LogLevel::Off);
        assert!(!config.covers(LogLevel::Minimal), "off 时连灯变都不记");

        // 开关「打开」写的就是这一对值（前端 switchToLogBlock 的唯一产出）。
        let on = LogConfig::from_block(Some(&json!({
            "log": { "level": "verbose", "hook_enabled": true }
        })));
        assert!(on.writes() && on.hook_writes());
        assert!(on.covers(LogLevel::Verbose), "打开即明细级");
    }

    #[test]
    fn an_unreadable_level_falls_back_to_recording_not_to_silence() {
        // 写了 log 块 = 有人想记；`level` 值认不出时不能顺着 DEFAULT_LEVEL 变成「不记」。
        let config = LogConfig::from_block(Some(&json!({ "log": { "level": "chatty" } })));
        assert_eq!(config.level, LogLevel::Normal);
        assert!(config.writes());
        assert!(!config.covers(LogLevel::Verbose), "回落档不含用户文本");
    }

    #[test]
    fn each_field_falls_back_independently_when_its_type_is_wrong() {
        // ⚠️ 入参是**组件的整块**配置（`module_settings['agent-status']`），日志字段在它的
        // `log` 子块里 —— 少包一层会让整块读成默认值，测试就变成空断言。
        let config = LogConfig::from_block(Some(&json!({
            "proxy_resolver": "cc-switch",
            "log": {
                "hook_enabled": false,
                "level": 12,
                "retention_days": "30",
                "max_file_bytes": "8000000",
                "ring_capacity": null,
            },
        })));
        assert!(!config.hook_enabled, "写对的字段保住");
        assert_eq!(config.level, LogLevel::Normal, "档位类型不对只回落档位");
        assert_eq!(config.retention_days, DEFAULT_RETENTION_DAYS);
        assert_eq!(config.max_file_bytes, DEFAULT_MAX_FILE_BYTES);
        assert_eq!(config.ring_capacity, DEFAULT_RING_CAPACITY as u32);
    }

    /**
     * 关只有一处：`level = off`。
     *
     * 这里锁的是「不要再长出第二个开关」—— 曾经 `enabled` 与 `off` 档并存、效果完全一致，
     * 而界面上 `off` 藏在「开关开着」才可见的卡里，等于一个选不到、一个说了不算。
     */
    #[test]
    fn off_is_the_only_way_to_stop_writing() {
        let mut config = LogConfig::default();
        config.level = LogLevel::Off;
        assert!(!config.writes(), "off 就是不写");
        assert!(!config.hook_writes(), "off 也关掉 hook 那一侧");
        assert!(!config.covers(LogLevel::Minimal), "off 时任何档都不记");
        config.level = LogLevel::Minimal;
        assert!(config.writes() && config.covers(LogLevel::Minimal));
        assert!(
            !config.covers(LogLevel::Normal),
            "minimal 只到灯变与清理，不含拒绝理由"
        );
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        let config = LogConfig::from_block(Some(&json!({
            "log": {
                "retention_days": 0,
                "max_file_bytes": 1,
                "max_total_bytes": u64::MAX,
                "live_tail_bytes": 0,
                "ring_capacity": 10_000_000,
            }
        })));
        assert_eq!(config.retention_days, MIN_RETENTION_DAYS);
        assert_eq!(config.max_file_bytes, MIN_MAX_FILE_BYTES);
        assert_eq!(config.max_total_bytes, MAX_MAX_TOTAL_BYTES);
        assert_eq!(config.live_tail_bytes, MIN_LIVE_TAIL_BYTES);
        assert_eq!(config.ring_capacity, MAX_RING_CAPACITY as u32);
    }

    #[test]
    fn level_aliases_and_unknown_names_resolve_safely() {
        assert_eq!(LogLevel::parse("  VERBOSE "), Some(LogLevel::Verbose));
        assert_eq!(LogLevel::parse("off"), Some(LogLevel::Off));
        assert_eq!(LogLevel::parse("chatty"), None, "认不出交给调用方回落");
    }

    #[test]
    fn hook_switch_only_narrows_the_level_never_widens_it() {
        let mut config = LogConfig::default();
        config.level = LogLevel::Verbose;
        assert!(config.covers(LogLevel::Normal), "高档包含低档");
        config.hook_enabled = false;
        assert!(config.writes(), "主进程照常写");
        assert!(!config.hook_writes(), "hook 侧单独关掉");
        // hook 开关只减不增：档位是 off 时，就算 hook_enabled 开着也不能写。
        config.level = LogLevel::Off;
        config.hook_enabled = true;
        assert!(!config.hook_writes(), "关就是关，hook 侧不许绕过");
    }

    #[test]
    fn differs_detects_a_real_change_only() {
        let previous = LogConfig::default();
        assert!(!previous.differs(&LogConfig::default()));
        assert!(previous.differs(&LogConfig {
            retention_days: 3,
            ..LogConfig::default()
        }));
    }
}
