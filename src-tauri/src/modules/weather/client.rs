use std::{io::Read, time::Duration};

/// 单次响应体最大字节限制（256 KB），防止恶意或巨型响应撑爆内存
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// 错误信息里回显的响应体长度上限（按字符计）：错误要看得懂，
/// 但不能把整页网关 HTML（和风对不合法 URL 会返回 Tomcat 的 400 页面）塞进界面。
const ERROR_SNIPPET_CHARS: usize = 160;

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
            return Err(format!("HTTP 状态码错误 {status}: {}", snippet(&body)));
        }

        Ok(body)
    }
}

/// 对 URL 查询参数做百分号编码（RFC 3986）：保留 `A-Za-z0-9-._~`，其余按 UTF-8 字节转 `%XX`。
///
/// ⚠️ 必须编码：中文城市名（如「杭州」）直接拼进 URL 会被网关判成 400 Bad Request
/// （实测和风 GeoAPI 的城市检索就是这个表现），带空格 / `&` / `+` 的 Key 同理。
pub fn encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~') {
            encoded.push(ch);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// 拼接完整请求 URL：`base` + `path` + 编码后的查询串（值一律先过 [`encode_query_component`]）。
pub fn build_url(base: &str, path: &str, params: &[(&str, &str)]) -> String {
    let query = params
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                encode_query_component(key),
                encode_query_component(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}{path}?{query}")
}

/// 把出错的响应体压成一行并截断（见 [`ERROR_SNIPPET_CHARS`]），供错误信息回显。
fn snippet(body: &str) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= ERROR_SNIPPET_CHARS {
        return flat;
    }
    let mut out: String = flat.chars().take(ERROR_SNIPPET_CHARS).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_cjk_and_url_reserved_characters() {
        assert_eq!(encode_query_component("杭州"), "%E6%9D%AD%E5%B7%9E");
        assert_eq!(encode_query_component("116.40,39.90"), "116.40%2C39.90");
        // 未保留字符原样输出，编码后仍然可读。
        assert_eq!(encode_query_component("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn builds_url_with_encoded_query() {
        assert_eq!(
            build_url(
                "https://geoapi.qweather.com",
                "/v2/city/lookup",
                &[("location", "海淀"), ("key", "a b")],
            ),
            "https://geoapi.qweather.com/v2/city/lookup?location=%E6%B5%B7%E6%B7%80&key=a%20b"
        );
    }

    #[test]
    fn error_snippet_is_single_line_and_truncated() {
        let body = format!("HTTP Status 400 \n  Bad Request {}", "x".repeat(400));
        let text = snippet(&body);
        assert!(!text.contains('\n'));
        assert_eq!(text.chars().count(), ERROR_SNIPPET_CHARS + 1);
        assert!(text.ends_with('…'));
    }
}
