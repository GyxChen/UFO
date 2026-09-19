use crate::modules::weather::{
    client::{build_url, WeatherHttpClient},
    models::{CityLookupItem, IpApiResponse, QWeatherCityLookupResponse},
};

/// 获取自动 IP 定位的坐标或城市名
pub fn auto_ip_location() -> Result<String, String> {
    // 请求公共免费 ip-api 接口获取外网 IP 所在地理位置
    let body = WeatherHttpClient::get("http://ip-api.com/json/?lang=zh-CN", 6000)
        .map_err(|e| format!("自动IP定位失败: {e}"))?;

    let parsed: IpApiResponse = serde_json::from_str(&body)
        .map_err(|e| format!("解析IP定位结果失败: {e}"))?;

    if parsed.status != "success" {
        return Err("IP 定位服务返回非成功状态".to_string());
    }

    if let (Some(lon), Some(lat)) = (parsed.lon, parsed.lat) {
        // 优先返回经纬度形式："116.40,39.90"
        Ok(format!("{lon:.2},{lat:.2}"))
    } else if !parsed.city.is_empty() {
        Ok(parsed.city)
    } else {
        Err("无法从 IP 获取到经纬度或城市名".to_string())
    }
}

/// 调用和风 GeoAPI 反查或根据关键字搜索城市列表
pub fn lookup_city(
    geo_host: &str,
    api_key: &str,
    location_query: &str,
) -> Result<Vec<CityLookupItem>, String> {
    if api_key.trim().is_empty() {
        return Err("和风天气 API Key 为空，请先在主窗口设置中配置".to_string());
    }

    // 关键字与 Key 都要编码：中文城市名不编码会被网关判成 400。
    let url = build_url(
        geo_host,
        "/v2/city/lookup",
        &[("location", location_query), ("key", api_key)],
    );

    let body = WeatherHttpClient::get(&url, 8000)
        .map_err(|e| format!("搜索城市失败: {e}"))?;

    let parsed: QWeatherCityLookupResponse = serde_json::from_str(&body)
        .map_err(|e| format!("解析城市搜索结果失败: {e}"))?;

    if parsed.code != "200" {
        return Err(format!("和风城市查询返回状态码: {}", parsed.code));
    }

    let items = parsed
        .location
        .into_iter()
        .map(|item| CityLookupItem {
            id: item.id,
            name: item.name,
            adm1: item.adm1,
            adm2: item.adm2,
            country: item.country,
        })
        .collect();

    Ok(items)
}
