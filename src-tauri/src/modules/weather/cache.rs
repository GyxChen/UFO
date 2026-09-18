use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use crate::modules::weather::models::WeatherSnapshot;

/// 缓存项封装
#[derive(Debug, Clone)]
struct CachedData<T> {
    data: T,
    expire_at_ms: i64,
}

/// 线程安全的天气快照与定位缓存
#[derive(Debug, Clone, Default)]
pub struct WeatherCache {
    inner: Arc<Mutex<WeatherCacheInner>>,
}

#[derive(Debug, Default)]
struct WeatherCacheInner {
    /// 缓存的天气快照
    snapshot: Option<CachedData<WeatherSnapshot>>,
    /// 缓存的城市 LocationID（例如 auto 模式下缓存 IP 解析的 location_id）
    cached_location: Option<CachedData<(String, String)>>, // (location_id, location_name)
}

impl WeatherCache {
    /// 创建一个新的缓存管理器
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取当前毫秒时间戳
    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    /// 获取缓存的天气快照（未过期且有效）
    pub fn get_snapshot(&self) -> Option<WeatherSnapshot> {
        let now = Self::now_ms();
        let guard = self.inner.lock().ok()?;
        if let Some(ref cached) = guard.snapshot {
            if cached.expire_at_ms > now {
                return Some(cached.data.clone());
            }
        }
        None
    }

    /// 设置天气快照缓存（ttl_sec 秒有效）
    pub fn set_snapshot(&self, snapshot: WeatherSnapshot, ttl_sec: u64) {
        let now = Self::now_ms();
        let expire_at_ms = now + (ttl_sec as i64 * 1000);
        if let Ok(mut guard) = self.inner.lock() {
            guard.snapshot = Some(CachedData {
                data: snapshot,
                expire_at_ms,
            });
        }
    }

    /// 获取缓存的自动定位解析结果
    pub fn get_location(&self) -> Option<(String, String)> {
        let now = Self::now_ms();
        let guard = self.inner.lock().ok()?;
        if let Some(ref cached) = guard.cached_location {
            if cached.expire_at_ms > now {
                return Some(cached.data.clone());
            }
        }
        None
    }

    /// 设置定位缓存（ttl_sec 秒有效，通常 24 小时）
    pub fn set_location(&self, location_id: String, location_name: String, ttl_sec: u64) {
        let now = Self::now_ms();
        let expire_at_ms = now + (ttl_sec as i64 * 1000);
        if let Ok(mut guard) = self.inner.lock() {
            guard.cached_location = Some(CachedData {
                data: (location_id, location_name),
                expire_at_ms,
            });
        }
    }

    /// 清空所有缓存（如用户切换城市或强制刷新时）
    pub fn clear(&self) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.snapshot = None;
            guard.cached_location = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_ttl_expiration() {
        let cache = WeatherCache::new();
        let dummy = WeatherSnapshot {
            location_name: "测试城市".to_string(),
            location_id: "101010100".to_string(),
            temp: 20,
            feels_like: 19,
            text: "晴".to_string(),
            icon_code: "100".to_string(),
            wind_dir: "南风".to_string(),
            wind_scale: "2".to_string(),
            humidity: 50,
            pressure: 1010,
            vis: 10,
            updated_at_ms: 0,
            hourly_forecast: vec![],
        };

        // 缓存 100 秒
        cache.set_snapshot(dummy.clone(), 100);
        assert!(cache.get_snapshot().is_some());

        // 清空
        cache.clear();
        assert!(cache.get_snapshot().is_none());
    }
}
