//! 待办模块的配置解释者（与前端 `src/modules/todo/settings.ts` 同口径）。
//!
//! 字段名 / 默认值 / 取值范围必须与前端逐字一致，并用同构的单测锁住 —— 两边不一致
//! 就会出现「界面显示 30 分钟、实际按 15 分钟拉」这种看不见的错。

use serde_json::Value;

use crate::floating::module_config::FloatingModuleConfig;

/// 本组件在外壳配置里的键（`module_settings` 的键，同时也是前端注册表里的模块 id）。
pub const MODULE_ID: &str = "todo";

/// 自动刷新周期的下限（分钟）：太短会白白打滴答接口。
const MIN_REFRESH_INTERVAL_MIN: u32 = 5;
/// 自动刷新周期的上限（分钟）。
const MAX_REFRESH_INTERVAL_MIN: u32 = 60;

/// 待办组件在 floating-modules.json 的配置项（凭据在 OS 凭据管理器里，不在这里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSettings {
    /// 是否已保存 API Token（纯界面状态；真实校验用凭据管理器里的值，读不到就是没配）
    pub has_token: bool,
    /// 只统计哪个清单：空串 = 全部清单；`inbox` = 收件箱；其余 = 滴答的清单 id
    pub project_id: String,
    /// 上述清单的展示名（仅界面回显用，后端不解释）
    pub project_name: String,
    /// 自动刷新周期（分钟）：5..=60
    pub refresh_interval_min: u32,
}

impl Default for TodoSettings {
    /// 缺省配置：未配 Token、全部清单、15 分钟刷新一次。
    fn default() -> Self {
        Self {
            has_token: false,
            project_id: String::new(),
            project_name: String::new(),
            refresh_interval_min: 15,
        }
    }
}

impl TodoSettings {
    /// 从整份外壳配置里取出本组件的块（缺块 = 全默认）。
    pub fn from_config(config: &FloatingModuleConfig) -> Self {
        Self::from_block(config.module_settings.get(MODULE_ID))
    }

    /// 逐字段宽松取值 + 夹取：某个字段写错只影响它自己，不让整块配置失效。
    pub fn from_block(block: Option<&Value>) -> Self {
        let default = Self::default();
        let Some(block) = block else {
            return default;
        };

        let has_token = block
            .get("has_token")
            .and_then(Value::as_bool)
            .unwrap_or(default.has_token);

        let project_id = block
            .get("project_id")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .unwrap_or(default.project_id);

        let project_name = block
            .get("project_name")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .unwrap_or(default.project_name);

        let refresh_interval_min = block
            .get("refresh_interval_min")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(MIN_REFRESH_INTERVAL_MIN as u64, MAX_REFRESH_INTERVAL_MIN as u64) as u32)
            .unwrap_or(default.refresh_interval_min);

        Self {
            has_token,
            project_id,
            project_name,
            refresh_interval_min,
        }
    }

    /// 把「清单筛选」翻译成接口要的清单 id 列表（空 = 不限定，统计全部清单）。
    pub fn project_ids(&self) -> Vec<String> {
        if self.project_id.is_empty() {
            Vec::new()
        } else {
            vec![self.project_id.clone()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_block_falls_back_to_defaults() {
        let parsed = TodoSettings::from_config(&FloatingModuleConfig::default());
        assert_eq!(parsed, TodoSettings::default());
        assert!(parsed.project_ids().is_empty());
    }

    #[test]
    fn parses_fields_and_trims_text() {
        let parsed = TodoSettings::from_block(Some(&serde_json::json!({
            "has_token": true,
            "project_id": "  69a850f41c20d2030e148fdf  ",
            "project_name": " 工作 ",
            "refresh_interval_min": 30,
        })));

        assert!(parsed.has_token);
        assert_eq!(parsed.project_id, "69a850f41c20d2030e148fdf");
        assert_eq!(parsed.project_name, "工作");
        assert_eq!(parsed.refresh_interval_min, 30);
        assert_eq!(parsed.project_ids(), vec!["69a850f41c20d2030e148fdf".to_string()]);
    }

    #[test]
    fn refresh_interval_is_clamped() {
        let low = TodoSettings::from_block(Some(&serde_json::json!({ "refresh_interval_min": 1 })));
        assert_eq!(low.refresh_interval_min, MIN_REFRESH_INTERVAL_MIN);

        let high = TodoSettings::from_block(Some(&serde_json::json!({ "refresh_interval_min": 999 })));
        assert_eq!(high.refresh_interval_min, MAX_REFRESH_INTERVAL_MIN);
    }

    #[test]
    fn wrong_field_types_only_break_that_field() {
        let parsed = TodoSettings::from_block(Some(&serde_json::json!({
            "has_token": "yes",
            "project_id": 42,
            "refresh_interval_min": "soon",
        })));

        assert!(!parsed.has_token);
        assert_eq!(parsed.project_id, "");
        assert_eq!(parsed.refresh_interval_min, 15);
    }
}
