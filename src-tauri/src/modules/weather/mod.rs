pub mod cache;
pub mod client;
pub mod geo;
pub mod models;
pub mod secrets;
pub mod settings;

use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};

use crate::floating::module_config::ModuleState;
use crate::modules::weather::{
    cache::WeatherCache,
    client::WeatherHttpClient,
    geo::{auto_ip_location, lookup_city},
    models::{
        CityLookupItem, HourlyWeatherItem, QWeatherHourlyResponse, QWeatherNowResponse,
        WeatherSnapshot,
    },
    secrets::{delete_api_key, get_api_key, set_api_key},
    settings::WeatherSettings,
};

/// 后端持有的天气模块共享状态
#[derive(Clone)]
pub struct WeatherState {
    pub cache: WeatherCache,
}

impl Default for WeatherState {
    fn default() -> Self {
        Self {
            cache: WeatherCache::new(),
        }
    }
}

/// 毫秒时间戳辅助函数
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 核心内部函数：拉取天气并组装快照
fn fetch_weather_internal(
    api_key: &str,
    settings: &WeatherSettings,
    cache: &WeatherCache,
) -> Result<WeatherSnapshot, String> {
    if api_key.trim().is_empty() {
        return Err("和风天气 API Key 为空，请在设置中配置".to_string());
    }

    // 1. 确定城市 Location ID 与名称
    let (location_id, location_name) = if settings.location_mode == "manual"
        && !settings.manual_location.is_empty()
    {
        (
            settings.manual_location.clone(),
            if settings.manual_location_name.is_empty() {
                settings.manual_location.clone()
            } else {
                settings.manual_location_name.clone()
            },
        )
    } else {
        // auto 模式：先查缓存
        if let Some(cached_loc) = cache.get_location() {
            cached_loc
        } else {
            // 通过 IP 定位获取经纬度或城市
            let ip_loc = auto_ip_location()?;
            let cities = lookup_city(settings.geo_host(), api_key, &ip_loc)?;
            let first = cities
                .first()
                .ok_or_else(|| "未能根据自动定位找到对应城市".to_string())?;
            let loc_name = format!("{}-{}", first.adm1, first.name);
            cache.set_location(first.id.clone(), loc_name.clone(), 86400); // 缓存 24 小时
            (first.id.clone(), loc_name)
        }
    };

    // 2. 请求实时天气
    let now_url = format!(
        "{}/v7/weather/now?location={}&key={}",
        settings.weather_host(),
        location_id,
        api_key
    );
    let now_body = WeatherHttpClient::get(&now_url, 8000)
        .map_err(|e| format!("获取实时天气失败: {e}"))?;
    let now_resp: QWeatherNowResponse = serde_json::from_str(&now_body)
        .map_err(|e| format!("解析实时天气响应失败: {e}"))?;

    if now_resp.code != "200" {
        return Err(format!("和风实时天气返回状态码: {}", now_resp.code));
    }
    let now_data = now_resp
        .now
        .ok_or_else(|| "实时天气数据缺失".to_string())?;

    // 3. 请求 24 小时预报
    let hourly_url = format!(
        "{}/v7/weather/24h?location={}&key={}",
        settings.weather_host(),
        location_id,
        api_key
    );
    let mut hourly_items = Vec::new();
    if let Ok(hourly_body) = WeatherHttpClient::get(&hourly_url, 8000) {
        if let Ok(hourly_resp) = serde_json::from_str::<QWeatherHourlyResponse>(&hourly_body) {
            if hourly_resp.code == "200" {
                hourly_items = hourly_resp
                    .hourly
                    .into_iter()
                    .take(8)
                    .map(|h| {
                        let time = if h.fx_time.len() >= 16 {
                            h.fx_time[11..16].to_string()
                        } else {
                            h.fx_time
                        };
                        HourlyWeatherItem {
                            time,
                            temp: h.temp.parse().unwrap_or(0),
                            icon_code: h.icon,
                            text: h.text,
                        }
                    })
                    .collect();
            }
        }
    }

    let snapshot = WeatherSnapshot {
        location_name,
        location_id,
        temp: now_data.temp.parse().unwrap_or(0),
        feels_like: now_data.feels_like.parse().unwrap_or(0),
        text: now_data.text,
        icon_code: now_data.icon,
        wind_dir: now_data.wind_dir,
        wind_scale: now_data.wind_scale,
        humidity: now_data.humidity.parse().unwrap_or(0),
        pressure: now_data.pressure.parse().unwrap_or(0),
        vis: now_data.vis.parse().unwrap_or(0),
        updated_at_ms: now_ms(),
        hourly_forecast: hourly_items,
    };

    Ok(snapshot)
}

/// Tauri Command: 获取天气快照（前端悬浮窗及 Tooltip 使用）
#[tauri::command]
pub async fn get_weather_snapshot(
    app: AppHandle,
    modules_state: State<'_, ModuleState>,
    weather_state: State<'_, WeatherState>,
    force_refresh: Option<bool>,
) -> Result<WeatherSnapshot, String> {
    let force = force_refresh.unwrap_or(false);

    // 优先读取缓存
    if !force {
        if let Some(cached) = weather_state.cache.get_snapshot() {
            let _ = app.emit("weather-snapshot", &cached);
            return Ok(cached);
        }
    }

    let config = modules_state.snapshot();
    let block = config.module_settings.get("weather").and_then(|v| v.as_object()).map(|obj| {
        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    });
    let settings = WeatherSettings::from_block(block.as_ref());
    let cache = weather_state.cache.clone();

    // 在阻塞线程池中完成 OS 凭据读取与网络请求
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let api_key = get_api_key()?
            .ok_or_else(|| "未配置和风天气 API Key".to_string())?;

        let snapshot = fetch_weather_internal(&api_key, &settings, &cache)?;
        // 缓存 10 分钟（600 秒）
        cache.set_snapshot(snapshot.clone(), 600);
        Ok::<WeatherSnapshot, String>(snapshot)
    })
    .await
    .map_err(|e| format!("执行天气请求任务失败: {e}"))??;

    // 广播最新天气快照给所有窗口（悬浮窗、主窗、提示窗）
    let _ = app.emit("weather-snapshot", &snapshot);
    Ok(snapshot)
}

/// Tauri Command: 关键字城市搜索（设置面板使用）
#[tauri::command]
pub async fn search_weather_city(
    query: String,
    host_type: String,
    override_api_key: Option<String>,
) -> Result<Vec<CityLookupItem>, String> {
    let settings = WeatherSettings {
        host_type,
        ..Default::default()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let key = match override_api_key.filter(|k| !k.trim().is_empty()) {
            Some(k) => k,
            None => get_api_key()?.ok_or_else(|| "未配置和风天气 API Key".to_string())?,
        };
        lookup_city(settings.geo_host(), &key, &query)
    })
    .await
    .map_err(|e| format!("城市检索任务失败: {e}"))?
}

/// Tauri Command: 测试天气连接与定位（设置面板使用）
#[tauri::command]
pub async fn test_weather_connection(
    host_type: String,
    location_mode: String,
    manual_location: String,
    manual_location_name: String,
    override_api_key: Option<String>,
) -> Result<WeatherSnapshot, String> {
    let settings = WeatherSettings {
        host_type,
        location_mode,
        manual_location,
        manual_location_name,
        ..Default::default()
    };
    let temp_cache = WeatherCache::new();

    tauri::async_runtime::spawn_blocking(move || {
        let key = match override_api_key.filter(|k| !k.trim().is_empty()) {
            Some(k) => k,
            None => get_api_key()?.ok_or_else(|| "未配置和风天气 API Key".to_string())?,
        };
        fetch_weather_internal(&key, &settings, &temp_cache)
    })
    .await
    .map_err(|e| format!("连通性测试任务失败: {e}"))?
}

/// Tauri Command: 设置和风 API Key 到 OS 凭据管理器（仅 main 窗口有权调用）
#[tauri::command]
pub async fn set_weather_secret(api_key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_api_key(&api_key)
    })
    .await
    .map_err(|e| format!("保存密钥任务失败: {e}"))?
}

/// Tauri Command: 从 OS 凭据管理器删除和风 API Key（仅 main 窗口有权调用）
#[tauri::command]
pub async fn delete_weather_secret() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        delete_api_key()
    })
    .await
    .map_err(|e| format!("删除密钥任务失败: {e}"))?
}

/// Tauri Command: 查询当前是否已存储 API Key（仅返回布尔值，不回显明文）
#[tauri::command]
pub async fn get_weather_secret_status() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(get_api_key()?.is_some())
    })
    .await
    .map_err(|e| format!("读取密钥状态失败: {e}"))?
}
