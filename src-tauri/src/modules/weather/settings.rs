use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::floating::module_config::ModuleSettingsMap;
use crate::modules::weather::secrets::{get_api_key, set_api_key};

/// 本组件在外壳配置里的键（`module_settings` 的键，同时也是前端注册表里的模块 id）。
pub const MODULE_ID: &str = "weather";

/// 旧版把这把 Key 明文写在配置块里的字段名（`module_settings.weather.api_key`）。
/// 密钥现在只进 OS 凭据管理器，这个字段只剩迁移用途。
const LEGACY_PLAINTEXT_KEY_FIELD: &str = "api_key";

/// 天气模块在 floating-modules.json 的配置项（敏感 key 已迁入 OS 凭据管理器）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeatherSettings {
    /// 是否已配置并保存 API Key（由前端根据凭据管理器状态维护或由后端命令更新）
    pub has_key: bool,
    /// 主机服务类型："dev" 开发版 | "commercial" 商业版 | "custom" 自定义
    pub host_type: String,
    /// 自定义服务域名
    pub custom_host: String,
    /// 定位模式："auto" 自动定位 | "manual" 手动指定城市
    pub location_mode: String,
    /// 手动指定的 LocationID 或经纬度
    pub manual_location: String,
    /// 手动指定的城市展示名称（如 "北京"）
    pub manual_location_name: String,
    /// 自动刷新周期（分钟）：范围 10..=60
    pub refresh_interval_min: u32,
}

impl Default for WeatherSettings {
    /// 获取缺省默认设置
    fn default() -> Self {
        Self {
            has_key: false,
            host_type: "dev".to_string(),
            custom_host: String::new(),
            location_mode: "auto".to_string(),
            manual_location: String::new(),
            manual_location_name: String::new(),
            refresh_interval_min: 15,
        }
    }
}

impl WeatherSettings {
    /// 从通用的 JSON 键值 Map 解析并夹取配置项
    pub fn from_block(block: Option<&BTreeMap<String, Value>>) -> Self {
        let default = Self::default();
        let Some(block) = block else {
            return default;
        };

        let has_key = block
            .get("has_key")
            .and_then(Value::as_bool)
            .unwrap_or(default.has_key);

        let host_type = match block.get("host_type").and_then(Value::as_str) {
            Some("commercial") => "commercial".to_string(),
            Some("custom") => "custom".to_string(),
            _ => "dev".to_string(),
        };

        let custom_host = block
            .get("custom_host")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .unwrap_or(default.custom_host);

        let location_mode = match block.get("location_mode").and_then(Value::as_str) {
            Some("manual") => "manual".to_string(),
            _ => "auto".to_string(),
        };

        let manual_location = block
            .get("manual_location")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .unwrap_or(default.manual_location);

        let manual_location_name = block
            .get("manual_location_name")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .unwrap_or(default.manual_location_name);

        let refresh_interval_min = block
            .get("refresh_interval_min")
            .and_then(Value::as_u64)
            .map(|v| v.clamp(10, 60) as u32)
            .unwrap_or(default.refresh_interval_min);

        Self {
            has_key,
            host_type,
            custom_host,
            location_mode,
            manual_location,
            manual_location_name,
            refresh_interval_min,
        }
    }

    /// 取得用于和风数据 API 的 Host 域名
    pub fn weather_host(&self) -> &str {
        match self.host_type.as_str() {
            "commercial" => "https://api.qweather.com",
            "custom" if !self.custom_host.is_empty() => &self.custom_host,
            _ => "https://devapi.qweather.com",
        }
    }

    /// 取得用于和风城市 GeoAPI 的 Host 域名
    pub fn geo_host(&self) -> &str {
        "https://geoapi.qweather.com"
    }
}

/// 认领旧版「明文 API Key」字段（组合根登记给外壳的迁移钩子）。
///
/// 界面写着「API Key 不进入配置文件」，可历史配置里确实躺着一份明文 —— 既不安全也自相矛盾，
/// 所以启动时清掉它。顺序刻意是「先兜住、再删」，绝不为了清理而丢凭据：
///
/// 1. 凭据管理器里已有 Key → 明文只是冗余副本，直接删；
/// 2. 凭据管理器里没有 Key → 先把明文迁进去，迁成功才删；
/// 3. 凭据管理器读 / 写失败 → 保留明文返回 false，下次启动再试。
///
/// 返回 true 表示认领了内容，外壳会把清理后的配置写回磁盘（不 bump revision、不广播）。
pub fn adopt_legacy_fields(
    settings: &mut ModuleSettingsMap,
    _legacy: &BTreeMap<String, Value>,
) -> bool {
    adopt_plaintext_key(settings, get_api_key, set_api_key)
}

/// [`adopt_legacy_fields`] 的实现体：凭据读写由调用方注入，便于单测（测试不碰真实凭据管理器）。
fn adopt_plaintext_key(
    settings: &mut ModuleSettingsMap,
    read_key: impl Fn() -> Result<Option<String>, String>,
    write_key: impl Fn(&str) -> Result<(), String>,
) -> bool {
    let Some(block) = settings.get_mut(MODULE_ID).and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(plaintext) = block
        .get(LEGACY_PLAINTEXT_KEY_FIELD)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
    else {
        return false;
    };

    match read_key() {
        // 1. 凭据管理器里已经有 Key：删掉明文即可。
        Ok(Some(_)) => {}
        // 2. 凭据管理器为空：先把明文迁进去，迁不动就保留（不能丢用户的 Key）。
        Ok(None) => {
            if let Err(error) = write_key(&plaintext) {
                eprintln!("failed to migrate weather api key into credential store: {error}");
                return false;
            }
        }
        // 3. 读不到凭据管理器：保留明文，下次启动再试。
        Err(error) => {
            eprintln!("failed to read weather credential store: {error}");
            return false;
        }
    }

    block.remove(LEGACY_PLAINTEXT_KEY_FIELD);
    // 有过明文 Key = 用户确实配过密钥：把状态同步成「已保存」，免得界面显示未配置。
    block.insert("has_key".to_string(), Value::Bool(true));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_and_clamping() {
        let mut map = BTreeMap::new();
        map.insert("has_key".to_string(), Value::Bool(true));
        map.insert("refresh_interval_min".to_string(), Value::Number(5.into()));

        let parsed = WeatherSettings::from_block(Some(&map));
        assert!(parsed.has_key);
        assert_eq!(parsed.refresh_interval_min, 10);
        assert_eq!(parsed.host_type, "dev");
        assert_eq!(parsed.weather_host(), "https://devapi.qweather.com");
    }

    /// 造一份只有本组件配置块的外壳配置（外壳不认识块里的字段，这里只当 JSON 搬运工）。
    fn settings_with(block: Value) -> ModuleSettingsMap {
        let mut settings = ModuleSettingsMap::new();
        settings.insert(MODULE_ID.to_string(), block);
        settings
    }

    /// 取回本组件配置块，供断言「明文还在不在」。
    fn block_of(settings: &ModuleSettingsMap) -> &Value {
        settings.get(MODULE_ID).expect("module block")
    }

    #[test]
    fn plaintext_key_is_migrated_into_the_credential_store() {
        use std::cell::RefCell;

        let mut settings =
            settings_with(serde_json::json!({ "api_key": "secret-key", "has_key": false }));
        let saved = RefCell::new(None);
        let adopted = adopt_plaintext_key(
            &mut settings,
            || Ok(None),
            |key| {
                *saved.borrow_mut() = Some(key.to_string());
                Ok(())
            },
        );

        assert!(adopted);
        assert_eq!(saved.into_inner().as_deref(), Some("secret-key"));
        // 明文从配置块里消失，状态同步成「已保存」。
        assert_eq!(block_of(&settings).get("api_key"), None);
        assert_eq!(
            block_of(&settings).get("has_key"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn plaintext_key_is_dropped_when_the_store_already_has_one() {
        use std::cell::Cell;

        let mut settings = settings_with(serde_json::json!({ "api_key": "stale-copy" }));
        let wrote = Cell::new(false);
        let adopted = adopt_plaintext_key(
            &mut settings,
            || Ok(Some("live-key".to_string())),
            |_| {
                wrote.set(true);
                Ok(())
            },
        );

        assert!(adopted);
        assert!(!wrote.get(), "凭据已有 Key 时不该再写一次");
        assert_eq!(block_of(&settings).get("api_key"), None);
    }

    #[test]
    fn plaintext_key_survives_when_the_credential_store_is_unavailable() {
        let mut settings = settings_with(serde_json::json!({ "api_key": "secret-key" }));

        assert!(!adopt_plaintext_key(
            &mut settings,
            || Err("keyring unavailable".to_string()),
            |_| Ok(())
        ));
        // 宁可不清理，也不能丢用户的 Key。
        assert_eq!(
            block_of(&settings).get("api_key"),
            Some(&serde_json::json!("secret-key"))
        );
    }

    #[test]
    fn plaintext_key_survives_when_migrating_it_fails() {
        let mut settings = settings_with(serde_json::json!({ "api_key": "secret-key" }));

        assert!(!adopt_plaintext_key(
            &mut settings,
            || Ok(None),
            |_| { Err("write failed".to_string()) }
        ));
        assert_eq!(
            block_of(&settings).get("api_key"),
            Some(&serde_json::json!("secret-key"))
        );
    }

    #[test]
    fn nothing_to_adopt_without_a_plaintext_key() {
        let mut configured = settings_with(serde_json::json!({ "has_key": true }));
        assert!(!adopt_plaintext_key(
            &mut configured,
            || Ok(None),
            |_| Ok(())
        ));
        assert_eq!(
            block_of(&configured).get("has_key"),
            Some(&serde_json::json!(true))
        );

        // 配置里根本没有本组件的块时也要安全返回 false。
        let mut empty = ModuleSettingsMap::new();
        assert!(!adopt_plaintext_key(&mut empty, || Ok(None), |_| Ok(())));
    }
}
