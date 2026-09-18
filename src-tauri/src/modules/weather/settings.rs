use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}
