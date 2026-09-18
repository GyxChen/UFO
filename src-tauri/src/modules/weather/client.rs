use std::{io::Read, time::Duration};

/// 单次响应体最大字节限制（256 KB），防止恶意或巨型响应撑爆内存
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// 封装基于 ureq 的同步 HTTP 客户端
pub struct WeatherHttpClient;

impl WeatherHttpClient {
    /// 发起一个 GET 请求，返回字符串内容（带超时、重定向控制与响应上限截断）
    pub fn get(url: &str, timeout_ms: u64) -> Result<String, String> {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_millis(timeout_ms)))
            .http_status_as_error(false)
            .max_redirects(3)
            .build();
        let agent = ureq::Agent::new_with_config(config);

        let response = agent
            .get(url)
            .header("User-Agent", "UFO-Floating-Weather/1.0")
            .header("Accept", "application/json")
            .call()
            .map_err(|err| format!("HTTP 请求失败: {err}"))?;

        let status = response.status().as_u16();
        let mut reader = response.into_body().into_reader().take(MAX_RESPONSE_BYTES as u64);
        let mut body = String::new();
        reader
            .read_to_string(&mut body)
            .map_err(|err| format!("读取响应失败: {err}"))?;

        if status >= 400 {
            return Err(format!("HTTP 状态码错误 {status}: {body}"));
        }

        Ok(body)
    }
}
