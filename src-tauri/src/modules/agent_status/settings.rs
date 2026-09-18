//! agent-status 组件自己的配置：解析 / 认领旧字段 / 应用到运行时。
//!
//! 悬浮窗外壳（[`crate::floating::module_config`]）只认「组件 id + 一块 JSON」，本文件是那块 JSON
//! 的唯一解释者 —— 字段名、默认值、取值范围、旧版扁平字段的迁移都写在这里。
//! 因此外壳不需要认识状态灯，将来新增组件也不必动外壳。

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::floating::module_config::{FloatingModuleConfig, ModuleConfigHook, ModuleSettingsMap};
use crate::modules::agent_status::proxy::NONE_RESOLVER;
use crate::modules::agent_status::status::StatusState;

/// 本组件在外壳配置里的键（`module_settings` 的键，同时也是前端注册表里的模块 id）。
pub const MODULE_ID: &str = "agent-status";

/// 完成灯保留默认 30 秒（与加入该设置前一致）。
const DEFAULT_RETENTION_MS: u64 = 30_000;
const MIN_RETENTION_MS: u64 = 1_000;
const MAX_RETENTION_MS: u64 = 24 * 60 * 60 * 1000;
/// Running 无新 hook 时的默认超时：60 秒（与 Error 下限同档，用户可在主窗口拉长）。
const DEFAULT_RUNNING_TIMEOUT_MS: u64 = 60 * 1000;
/// Running 超时下限：1 分钟（不低于旧的 Error 级 60s 门槛）。
const MIN_RUNNING_TIMEOUT_MS: u64 = 60 * 1000;
/// Running 超时上限：60 分钟。
const MAX_RUNNING_TIMEOUT_MS: u64 = 60 * 60 * 1000;

/// 旧版外壳把本组件字段平铺在配置顶层时的键名 → 本组件配置块里的键名。
/// 迁移期只读一次，之后配置里只留 `module_settings` 那一份。
const LEGACY_FLAT_FIELDS: &[(&str, &str)] = &[
    ("completed_retention_ms", "completed_retention_ms"),
    ("running_timeout_ms", "running_timeout_ms"),
    ("token_unit", "token_unit"),
    ("proxy_resolver", "proxy_resolver"),
];

/// agent-status 组件自己解释的配置（时间是毫秒，与 Go / Rust 侧状态监测一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatusSettings {
    /// 一轮执行完成后绿灯保留多久（毫秒，1s ..= 24h）。
    pub completed_retention_ms: u64,
    /// Running 阶段无新事件时的超时（毫秒，1min ..= 60min）；超时后移除绿灯。
    pub running_timeout_ms: u64,
    /// 代理解析器 id：`none` = 不解析。缺字段 / 空串都回落 `none`。
    pub proxy_resolver: String,
}

impl Default for AgentStatusSettings {
    fn default() -> Self {
        Self {
            completed_retention_ms: DEFAULT_RETENTION_MS,
            running_timeout_ms: DEFAULT_RUNNING_TIMEOUT_MS,
            proxy_resolver: NONE_RESOLVER.to_string(),
        }
    }
}

impl AgentStatusSettings {
    /// 从整份外壳配置里取出本组件的配置块（缺块 = 全默认）。
    pub fn from_config(config: &FloatingModuleConfig) -> Self {
        Self::from_block(config.module_settings.get(MODULE_ID))
    }

    /// 解析本组件的配置块，并夹取各字段到合法范围。
    ///
    /// 刻意逐字段宽松取值（而不是用 `serde` 整块反序列化）：手改配置把某个值写错，
    /// 只该回落那一个字段，不能让整份配置加载失败、连带丢掉用户其它设置。
    pub fn from_block(block: Option<&Value>) -> Self {
        let defaults = Self::default();
        let Some(block) = block else {
            return defaults;
        };
        Self {
            completed_retention_ms: read_u64(
                block,
                "completed_retention_ms",
                defaults.completed_retention_ms,
            )
            .clamp(MIN_RETENTION_MS, MAX_RETENTION_MS),
            running_timeout_ms: read_u64(block, "running_timeout_ms", defaults.running_timeout_ms)
                .clamp(MIN_RUNNING_TIMEOUT_MS, MAX_RUNNING_TIMEOUT_MS),
            proxy_resolver: read_string(block, "proxy_resolver")
                .unwrap_or(defaults.proxy_resolver),
        }
    }
}

/// 取一个毫秒数；字段缺失或类型不对（如被引号包起来）都回落默认值。
fn read_u64(block: &Value, key: &str, fallback: u64) -> u64 {
    block.get(key).and_then(Value::as_u64).unwrap_or(fallback)
}

/// 取一个非空字符串（自动 trim）；字段缺失 / 空串 / 类型不对返回 `None`。
fn read_string(block: &Value, key: &str) -> Option<String> {
    let value = block.get(key)?.as_str()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 认领旧版扁平字段（组合根登记给外壳的迁移钩子）。
///
/// 只在文件里还没有本组件的配置块时把旧值搬进新块；已经有块就只认领不覆盖 ——
/// 老字段残留不该顶掉用户新改的设置。返回 true 时外壳会把清理后的配置写回磁盘。
pub fn adopt_legacy_fields(
    settings: &mut ModuleSettingsMap,
    legacy: &BTreeMap<String, Value>,
) -> bool {
    let present: Vec<(&str, &Value)> = LEGACY_FLAT_FIELDS
        .iter()
        .filter_map(|(legacy_key, key)| {
            legacy
                .get(*legacy_key)
                .map(|value| (*key, value))
        })
        .collect();
    if present.is_empty() {
        return false;
    }
    if !settings.contains_key(MODULE_ID) {
        let mut block = Map::new();
        for (key, value) in present {
            block.insert(key.to_string(), value.clone());
        }
        settings.insert(MODULE_ID.to_string(), Value::Object(block));
    }
    true
}

/// 配置变更钩子（组合根登记）：把新配置推给状态监测，改完即时生效、无需重启。
pub fn config_hook() -> ModuleConfigHook {
    Box::new(|app: &AppHandle, config: &FloatingModuleConfig| {
        let settings = AgentStatusSettings::from_config(config);
        // StatusState 还没建好（启动早期的写盘）时什么都不用做：启动路径会自己取初值。
        let Some(status_state) = app.try_state::<StatusState>() else {
            return;
        };
        if let Ok(mut retention_ms) = status_state.retention_ms.lock() {
            *retention_ms = settings.completed_retention_ms;
        }
        if let Ok(mut running_timeout_ms) = status_state.running_timeout_ms.lock() {
            *running_timeout_ms = settings.running_timeout_ms;
        }
        // 代理解析器选择也即时同步，切换后无需重启即可生效。
        status_state.proxy.set_selected(&settings.proxy_resolver);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_previous_flat_defaults() {
        let settings = AgentStatusSettings::default();
        assert_eq!(settings.completed_retention_ms, 30_000);
        assert_eq!(settings.running_timeout_ms, 60_000);
        assert_eq!(settings.proxy_resolver, NONE_RESOLVER);
    }

    #[test]
    fn missing_block_and_missing_fields_fall_back() {
        let settings = AgentStatusSettings::from_block(None);
        assert_eq!(settings, AgentStatusSettings::default());

        let settings = AgentStatusSettings::from_block(Some(&json!({})));
        assert_eq!(settings, AgentStatusSettings::default());
    }

    #[test]
    fn wrong_field_type_falls_back_without_discarding_the_rest() {
        // 关键回归：一个字段写错（这里是类型不对 / 非法单位）不能牵连其它字段。
        let settings = AgentStatusSettings::from_block(Some(&json!({
            "completed_retention_ms": 45_000,
            "running_timeout_ms": "120000",
            "proxy_resolver": "  "
        })));
        assert_eq!(settings.completed_retention_ms, 45_000);
        assert_eq!(settings.running_timeout_ms, 60_000);
        assert_eq!(settings.proxy_resolver, NONE_RESOLVER);
    }

    #[test]
    fn reads_values_and_clamps_them() {
        let settings = AgentStatusSettings::from_block(Some(&json!({
            "completed_retention_ms": 0,
            "running_timeout_ms": u64::MAX,
            "proxy_resolver": "  cc-switch  "
        })));
        assert_eq!(settings.completed_retention_ms, MIN_RETENTION_MS);
        assert_eq!(settings.running_timeout_ms, MAX_RUNNING_TIMEOUT_MS);
        assert_eq!(settings.proxy_resolver, "cc-switch");

        let settings = AgentStatusSettings::from_block(Some(&json!({
            "completed_retention_ms": u64::MAX,
            "running_timeout_ms": 0
        })));
        assert_eq!(settings.completed_retention_ms, MAX_RETENTION_MS);
        assert_eq!(settings.running_timeout_ms, MIN_RUNNING_TIMEOUT_MS);
    }

    #[test]
    fn adopts_legacy_flat_fields_into_its_own_block() {
        let mut settings = ModuleSettingsMap::new();
        let mut legacy = BTreeMap::new();
        legacy.insert("completed_retention_ms".to_string(), json!(45_000));
        legacy.insert("running_timeout_ms".to_string(), json!(120_000));
        legacy.insert("token_unit".to_string(), json!("m_tokens"));
        legacy.insert("proxy_resolver".to_string(), json!("cc-switch"));
        // 外壳顶层还有别的组件 / 别的东西留下的字段，不该被本组件顺手带走。
        legacy.insert("stub_value".to_string(), json!("other-module"));

        assert!(adopt_legacy_fields(&mut settings, &legacy));
        let block = settings.get(MODULE_ID).expect("block adopted");
        assert_eq!(block.get("completed_retention_ms"), Some(&json!(45_000)));
        assert_eq!(block.get("token_unit"), Some(&json!("m_tokens")));
        assert_eq!(block.get("stub_value"), None);
        let parsed = AgentStatusSettings::from_block(Some(block));
        assert_eq!(parsed.completed_retention_ms, 45_000);
        assert_eq!(parsed.proxy_resolver, "cc-switch");
    }

    #[test]
    fn adoption_never_overwrites_an_existing_block() {
        // 已经有新配置块时，旧字段只被认领（好让外壳清理掉），不覆盖用户当前取值。
        let mut settings = ModuleSettingsMap::new();
        settings.insert(MODULE_ID.to_string(), json!({"completed_retention_ms": 9_000}));
        let mut legacy = BTreeMap::new();
        legacy.insert("completed_retention_ms".to_string(), json!(30_000));

        assert!(adopt_legacy_fields(&mut settings, &legacy));
        assert_eq!(
            AgentStatusSettings::from_config(&FloatingModuleConfig {
                module_settings: settings,
                ..FloatingModuleConfig::default()
            })
            .completed_retention_ms,
            9_000
        );
    }

    #[test]
    fn loading_a_legacy_file_migrates_the_real_shell_config() {
        // 端到端：真外壳配置 + 真迁移钩子 —— 旧版扁平字段必须落进本组件的配置块且写回文件。
        let dir = std::env::temp_dir().join("ufo-test-agent-status-migration");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("floating-modules.json");
        std::fs::write(
            &path,
            r#"{
                "schema_version": 1,
                "revision": 9,
                "modules": [{"id":"agent-status","enabled":true,"order":0}],
                "completed_retention_ms": 45000,
                "running_timeout_ms": 120000,
                "token_unit": "m_tokens",
                "proxy_resolver": "cc-switch",
                "max_cells_by_layout": {"square":8,"corner":8,"horizontal":8,"vertical":8}
            }"#,
        )
        .unwrap();

        let state = crate::floating::module_config::ModuleState::load(path.clone(), &[adopt_legacy_fields]);
        let config = state.snapshot();
        let settings = AgentStatusSettings::from_config(&config);
        assert_eq!(settings.completed_retention_ms, 45_000);
        assert_eq!(settings.running_timeout_ms, 120_000);
        assert_eq!(settings.proxy_resolver, "cc-switch");
        // token 单位由前端解释，这里只保证它被原样搬进配置块。
        assert_eq!(
            config.module_settings.get(MODULE_ID).and_then(|block| block.get("token_unit")),
            Some(&json!("m_tokens"))
        );

        // 写回后旧字段不再挂在顶层（值本身还在，只是搬进了 module_settings）。
        let rewritten = std::fs::read_to_string(&path).unwrap();
        let reloaded: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
        assert!(reloaded.get("completed_retention_ms").is_none());
        assert!(reloaded.get("module_settings").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adoption_is_a_no_op_without_legacy_fields() {
        let mut settings = ModuleSettingsMap::new();
        assert!(!adopt_legacy_fields(&mut settings, &BTreeMap::new()));
        assert!(settings.is_empty());
    }
}
