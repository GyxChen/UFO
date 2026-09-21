use serde::{Deserialize, Serialize};

/// 天气状况快照（前端消费的标准模型）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeatherSnapshot {
    /// 城市显示名称（如 "北京市海淀区" 或 "杭州"）
    pub location_name: String,
    /// 城市 LocationID 或经纬度标识
    pub location_id: String,
    /// 当前温度（摄氏度）
    pub temp: i32,
    /// 体感温度（摄氏度）
    pub feels_like: i32,
    /// 天气状况文本（如 "晴"、"多云"、"雷阵雨"）
    pub text: String,
    /// 和风天气图标代码（如 "100"、"101"、"305"）
    pub icon_code: String,
    /// 风向描述（如 "东北风"）
    pub wind_dir: String,
    /// 风力等级
    pub wind_scale: String,
    /// 相对湿度百分比（0-100）
    pub humidity: u8,
    /// 大气压强（hPa）
    pub pressure: i32,
    /// 能见度（公里）
    pub vis: i32,
    /// 数据更新时间戳（毫秒）
    pub updated_at_ms: i64,
    /// 未来 24 小时简明预报列表
    pub hourly_forecast: Vec<HourlyWeatherItem>,
}

/// 逐小时简明天气数据
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HourlyWeatherItem {
    /// 预报时间点（如 "14:00"）
    pub time: String,
    /// 预报温度（摄氏度）
    pub temp: i32,
    /// 天气图标代码
    pub icon_code: String,
    /// 天气状况文本
    pub text: String,
}

/// 城市检索命中项（用于设置面板手动选择城市）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CityLookupItem {
    /// 城市 Location ID（如 "101010100"）
    pub id: String,
    /// 城市/区县名称（如 "海淀"）
    pub name: String,
    /// 所属上级行政区划/省份（如 "北京市"）
    pub adm1: String,
    /// 所属二级行政区（如 "北京"）
    pub adm2: String,
    /// 国家
    pub country: String,
}

/// 和风天气 API - /v7/weather/now 原始响应结构
#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherNowResponse {
    pub code: String,
    pub now: Option<QWeatherNowData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherNowData {
    pub temp: String,
    #[serde(rename = "feelsLike", default)]
    pub feels_like: String,
    pub icon: String,
    pub text: String,
    #[serde(rename = "windDir", default)]
    pub wind_dir: String,
    #[serde(rename = "windScale", default)]
    pub wind_scale: String,
    #[serde(default)]
    pub humidity: String,
    #[serde(default)]
    pub pressure: String,
    #[serde(default)]
    pub vis: String,
}

/// 和风天气 API - /v7/weather/24h 原始响应结构
#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherHourlyResponse {
    pub code: String,
    #[serde(default)]
    pub hourly: Vec<QWeatherHourlyData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherHourlyData {
    #[serde(rename = "fxTime")]
    pub fx_time: String,
    pub temp: String,
    pub icon: String,
    pub text: String,
}

/// 和风 GeoAPI - /v2/city/lookup 原始响应结构
#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherCityLookupResponse {
    pub code: String,
    #[serde(default)]
    pub location: Vec<QWeatherLocationData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QWeatherLocationData {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub adm1: String,
    #[serde(default)]
    pub adm2: String,
    #[serde(default)]
    pub country: String,
}

/// ip-api.com 响应结构
#[derive(Debug, Clone, Deserialize)]
pub struct IpApiResponse {
    pub status: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub region_name: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_now_response_success() {
        let json_data = r#"{
            "code": "200",
            "updateTime": "2026-03-24T14:30+08:00",
            "now": {
                "obsTime": "2026-03-24T14:00+08:00",
                "temp": "23",
                "feelsLike": "21",
                "icon": "100",
                "text": "晴",
                "wind360": "45",
                "windDir": "东北风",
                "windScale": "3",
                "windSpeed": "15",
                "humidity": "40",
                "precip": "0.0",
                "pressure": "1012",
                "vis": "16"
            }
        }"#;

        let parsed: QWeatherNowResponse = serde_json::from_str(json_data).expect("解析实时天气 JSON 失败");
        assert_eq!(parsed.code, "200");
        let now = parsed.now.expect("缺失 now 数据字段");
        assert_eq!(now.temp, "23");
        assert_eq!(now.feels_like, "21");
        assert_eq!(now.text, "晴");
        assert_eq!(now.icon, "100");
    }

    #[test]
    fn parse_hourly_response_success() {
        let json_data = r#"{
            "code": "200",
            "hourly": [
                {
                    "fxTime": "2026-03-24T15:00+08:00",
                    "temp": "24",
                    "icon": "100",
                    "text": "晴"
                },
                {
                    "fxTime": "2026-03-24T16:00+08:00",
                    "temp": "22",
                    "icon": "101",
                    "text": "多云"
                }
            ]
        }"#;

        let parsed: QWeatherHourlyResponse = serde_json::from_str(json_data).expect("解析逐小时天气 JSON 失败");
        assert_eq!(parsed.code, "200");
        assert_eq!(parsed.hourly.len(), 2);
        assert_eq!(parsed.hourly[0].temp, "24");
    }
}
