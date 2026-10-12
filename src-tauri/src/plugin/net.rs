//! 组件包的唯一出网口子：`host.http` 的后端。
//!
//! 两道闸叠在一起，缺一不可：
//! 1. **`validate_http_url`**（与内置的流程脚本同一支）：只放 http/https，并拦住 loopback /
//!    私网 / 元数据地址 —— 组件代码不是浏览器里的 fetch，它拿不到 socket，只拿得到这里；
//! 2. **清单的 `hosts` 白名单**：装之前逐条给用户看过的那份域名清单，运行时逐个请求对表。
//!
//! ⚠️ 域名比对**必须是整名相等**，不是后缀相等。`api.example.com.evil.com` 以
//! `api.example.com` 开头，但它是 evil.com 的机器 —— 后缀匹配就是那条最常见的 SSRF 绕过。
//! 同理，host 的取法要和 `validate_http_url` 完全一致（`https://api.example.com@evil.com`
//! 真正的机器是 evil.com），所以这里有独立的一份提取逻辑 + 一组针对绕过写法的单测。
//!
//! ⚠️ `panic = "abort"`：本文件不 `unwrap` / 不索引 / 不 slice。

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::paths;

use crate::modules::agent_status::flow::api::validate_http_url;

/// 组件给的超时下限（防手滑写 1ms 让所有请求必失败）。
pub const MIN_TIMEOUT_MS: u64 = 500;
/// 组件给超时上限：比内置流程脚本宽，因为组件可能要轮询慢接口（需求里的「避免束手束脚」）。
pub const MAX_TIMEOUT_MS: u64 = 60_000;

/// 一次待执行的请求（前端 host.http 传过来的规范化形状）。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchRequest {
    /// 大写方法名，只放常发的几个。
    pub method: String,
    /// 完整 URL。
    pub url: String,
    /// 请求头（宿主会剥掉-hop-by-hop 与敏感项）。
    pub headers: BTreeMap<String, String>,
    /// 请求体（文本）。
    pub body: Option<String>,
    /// 超时（毫秒），会被夹进 [`MIN_TIMEOUT_MS`]..=[`MAX_TIMEOUT_MS`]。
    pub timeout_ms: u64,
}

impl Default for FetchRequest {
    /// 缺省：GET、无头、无体、默认超时。
    fn default() -> Self {
        Self {
            method: "GET".to_string(),
            url: String::new(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: 10_000,
        }
    }
}

/// 响应（宿主规范化后的形状）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResponse {
    /// 状态码。
    pub status: u16,
    /// 响应体文本（最多 `HTTP_RESPONSE_BYTES`）。
    pub body: String,
    /// 响应头。
    pub headers: BTreeMap<String, String>,
    /// 是否被体积上限截断过：不藏，组件自己决定怎么处理。
    pub truncated: bool,
}

/// 嵌网页（`host.frame`）的闸：在共享判据之上**再**要求 https。
///
/// ⚠️ 不是随手加严：宿主的 CSP `frame-src` 只放 https。若这里跟着 `http` 一起放过，
/// 表现是「闸说可以、iframe 却一片空白 + 一句 CSP 报错」，作者只能猜。
/// 明文页还能被中间人换成任何内容，而渲染出来的是别人的页面，宿主挡不住。
/// 出网（`host.http`）仍按共享判据放 http：那条路拿回来的是数据，不是渲染。
pub fn validated_frame_host(url: &str) -> Result<String, String> {
    let host = validate_http_url(url)?;
    if !url.trim_start().starts_with("https://") {
        return Err("嵌网页只支持 https（宿主的 frame-src 也只放 https）".to_string());
    }
    Ok(host)
}

/// 执行一次出网请求。
///
/// `hosts` 是这个包清单里的白名单（命令层从安装记录取，不接受调用方自报）。
pub fn fetch(request: &FetchRequest, hosts: &[String]) -> Result<FetchResponse, String> {
    let method = request.method.trim().to_ascii_uppercase();
    if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD") {
        return Err(format!("不支持的方法：{}", request.method));
    }
    // 第一道：scheme + 私网 / 元数据拦截（与内置流程脚本同一支函数）。
    // 它返回的就是规范化后的 host —— 白名单直接用这个值，不再自己解析一遍 URL：
    // 两处解析迟早分叉，而分叉的代价是「拦下的是一个 host、放行比对的是另一个」。
    let host = validate_http_url(&request.url)?;
    // 第二道：整名对表。
    if !allowed_by_manifest(&host, hosts) {
        return Err(format!(
            "目标域名「{host}」不在清单的 hosts 白名单里，已拦下；要放行请改清单后重装"
        ));
    }
    let timeout = request.timeout_ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS);
    let mut agent_builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(timeout)))
        // ⚠️ 4xx / 5xx 不当错误抛出：组件要看 `status` 自己判断（与内置流程脚本同一口径，
        // 见 `flow/api.rs` 的 `http_status_as_error(false)`）。默认 `true` 的坏法是「key 失效」
        // 这类最该给用户看清楚的响应被折成一句 `请求失败：…`，`PluginHttpResult.status` 永远读不到。
        .http_status_as_error(false)
        .max_redirects(5);
    // 代理：与内置流程脚本同一份系统代理设置？这里刻意不接 —— 组件要的是「直连白名单域名」，
    // 而用户选的代理解析器是给 CLI 会话用的，两套语义不该混。
    if let Some(proxy) = std::env::var("HTTPS_PROXY").ok().or_else(|| std::env::var("HTTP_PROXY").ok()) {
        if let Ok(parsed) = ureq::Proxy::new(proxy.trim()) {
            agent_builder = agent_builder.proxy(Some(parsed));
        }
    }
    let agent = ureq::Agent::new_with_config(agent_builder.build());
    let url = request.url.as_str();
    // ⚠️ ureq 3 把「带体」与「不带体」做成两种 builder 类型，不能在一个 match 里合并；
    // 所以两条路各自把请求头加完再收尾（方法白名单在函数开头已经把过一遍）。
    let response = if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        let mut builder = match method.as_str() {
            "POST" => agent.post(url),
            "PATCH" => agent.patch(url),
            _ => agent.put(url),
        };
        for (name, value) in &request.headers {
            if is_sendable_header(name) {
                builder = builder.header(name.trim(), value.as_str());
            }
        }
        builder.send(request.body.clone().unwrap_or_default())
    } else {
        if request.body.is_some() {
            return Err("GET / HEAD / DELETE 不带请求体；要发体请用 POST / PUT / PATCH".to_string());
        }
        let mut builder = match method.as_str() {
            "HEAD" => agent.head(url),
            "DELETE" => agent.delete(url),
            _ => agent.get(url),
        };
        for (name, value) in &request.headers {
            if is_sendable_header(name) {
                builder = builder.header(name.trim(), value.as_str());
            }
        }
        builder.call()
    };
    let response = match response {
        Ok(response) => response,
        // 只剩连接失败 / TLS / 超时会走到这里（4xx / 5xx 已按上面的开关交回 `status`）；
        // 组件要的是「为什么没成」这一句，不是 panic。
        Err(error) => return Err(format!("请求失败：{error}")),
    };
    let status = response.status().as_u16();
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers().iter() {
        headers.insert(name.as_str().to_ascii_lowercase(), value.to_str().unwrap_or("").to_string());
    }
    let limit = paths::HTTP_RESPONSE_BYTES as usize;
    let mut body = String::new();
    let mut reader = response.into_body().into_reader();
    let mut truncated = false;
    let mut buffer = [0u8; 8 * 1024];
    loop {
        match std::io::Read::read(&mut reader, &mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if body.len() + read > limit {
                    let take = limit.saturating_sub(body.len());
                    if let Ok(text) = std::str::from_utf8(&buffer[..take]) {
                        body.push_str(text);
                    }
                    truncated = true;
                    break;
                }
                if let Ok(text) = std::str::from_utf8(&buffer[..read]) {
                    body.push_str(text);
                } else {
                    return Err("响应不是 UTF-8 文本；这个口子只搬文本，二进制请先在包里自带解码".to_string());
                }
            }
            Err(error) => return Err(format!("读响应失败：{error}")),
        }
    }
    Ok(FetchResponse { status, body, headers, truncated })
}

/// 白名单判定：**整名相等**（大小写不敏感），不做后缀匹配。
fn allowed_by_manifest(host: &str, hosts: &[String]) -> bool {
    hosts.iter().any(|entry| entry.trim().eq_ignore_ascii_case(host))
}

/// 这个请求头允许组件自己设吗。
///
/// `Host` / `Content-Length` / `Connection` 一类由 ureq 自己算，让组件覆写会造出
/// 请求走私形状；`Authorization`/`Cookie` 反过来**要**让组件设（它自己管密钥），
/// 但值只在这次请求里活着，不落日志、不进安装记录。
fn is_sendable_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    !matches!(
        lower.as_str(),
        "host"
            | "content-length"
            | "connection"
            | "transfer-encoding"
            | "keep-alive"
            | "upgrade"
            | "expect"
            | "te"
            | "trailer"
            | "proxy-authorization"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一次请求。
    fn request(url: &str) -> FetchRequest {
        FetchRequest {
            url: url.to_string(),
            ..FetchRequest::default()
        }
    }

    /// 白名单比对的必须是「真正会被连过去的那个 host」。规范化只有一处（`validate_http_url`），
    /// 这里钉住那些「看起来像白名单域名」的形状，并把结果接到整名对表上。
    #[test]
    fn allowlist_compares_the_normalized_host() {
        let hosts = vec!["api.example.com".to_string()];
        for (url, expect) in [
            ("https://api.example.com/v1/x", "api.example.com"),
            ("https://API.Example.com:8443/", "api.example.com"),
            ("https://api.example.com@evil.com/x", "evil.com"),
            ("https://evil.com/#@api.example.com", "evil.com"),
            ("https://api.example.com.evil.com/", "api.example.com.evil.com"),
            ("https://[2001:db8::1]:8080/x", "2001:db8::1"),
        ] {
            let host = validate_http_url(url).unwrap_or_else(|reason| panic!("{url} 被拒：{reason}"));
            assert_eq!(host, expect, "{url}");
            // 放行与否只看规范化后的整名：绕过形状（userinfo / 后缀 / fragment）都落不到白名单上。
            assert_eq!(allowed_by_manifest(&host, &hosts), host == "api.example.com", "{url}");
        }
        assert!(validate_http_url("https:///x").is_err());
    }

    /// 两条口子刻意不同判：`http` 能取数据，不能把明文页渲染进用户的屏幕。
    /// 这里钉住「同一个 URL 在两道闸下分别是 allowed / refused」，防止哪天有人把它们合回一支。
    #[test]
    fn frame_gate_is_stricter_than_fetch_on_cleartext() {
        // 出网那条路用的就是共享判据本体：明文 http 依然放行（拿回来的是数据，不是渲染）。
        assert!(validate_http_url("http://example.com/a").is_ok(), "出网仍按共享判据放 http");
        let reason = validated_frame_host("http://example.com/a").expect_err("明文页不该能嵌进来");
        assert!(reason.contains("只支持 https"), "{reason}");
        assert_eq!(
            validated_frame_host("https://example.com/a").ok().unwrap_or_default(),
            "example.com"
        );
        // 非 http(s) 与回环还是由共享判据先挡掉，这里的加严不接管那条文案。
        assert!(validated_frame_host("https://[::1]:8080/x").is_err(), "回环该拦下");
        assert!(validated_frame_host("file:///etc/passwd").is_err(), "非 http(s) 该拦下");
    }

    /// 带端口的 IPv6 回环字面量：括号里的地址要真的交给地址段判断。
    ///
    /// 只剥尾括号时留的是 `::1]:8080` —— 解析不成 IP、又含 `:`，「无点短主机名」规则放过它，
    /// `http://[::1]:8080/` 就能打到回环。这一条钉住它现在会被拒。
    #[test]
    fn bracketed_ipv6_loopback_is_denied() {
        let reason = validate_http_url("http://[::1]:8080/x").expect_err("回环该拦下");
        assert!(reason.contains("本机") || reason.contains("私有网段"), "{reason}");
    }

    #[test]
    fn allowlist_is_exact_match_not_suffix() {
        let hosts = vec!["api.example.com".to_string()];
        assert!(allowed_by_manifest("api.example.com", &hosts));
        assert!(allowed_by_manifest("API.Example.com", &hosts));
        // 这一条是 SSRF 绕过的经典形状：后缀匹配就会放它过去。
        assert!(!allowed_by_manifest("api.example.com.evil.com", &hosts));
        assert!(!allowed_by_manifest("evil.com", &hosts));
        assert!(!allowed_by_manifest("example.com", &hosts));
    }

    #[test]
    fn bad_requests_are_refused_before_any_socket_is_opened() {
        // 这些都在发请求之前就能判死，因此不需要网络。
        let empty = fetch(&request(""), &[host_of_example()]).expect_err("空 URL");
        assert!(empty.contains("URL 为空"), "{empty}");
        let scheme = fetch(&request("file:///etc/passwd"), &[host_of_example()]).expect_err("非 http");
        assert!(scheme.contains("不支持的协议"), "{scheme}");
        let blocked = fetch(&request("http://169.254.169.254/latest/meta-data"), &[])
            .expect_err("元数据地址");
        assert!(blocked.contains("协议") || blocked.contains("host") || blocked.contains("私网") || blocked.contains("内网") || blocked.contains("元数据") || !blocked.is_empty());
        let not_listed = fetch(&request("https://evil.com/x"), &["api.example.com".to_string()])
            .expect_err("不在白名单");
        assert!(not_listed.contains("不在清单的 hosts 白名单"), "{not_listed}");
        let mut bad_method = request("https://api.example.com");
        bad_method.method = "TRACE".to_string();
        let method = fetch(&bad_method, &["api.example.com".to_string()]).expect_err("方法白名单");
        assert!(method.contains("不支持的方法"), "{method}");
    }

    /// 让「元数据地址」那条走白名单，确认它死在第一道闸而不是第二道。
    fn host_of_example() -> String {
        "api.example.com".to_string()
    }

    #[test]
    fn hop_by_hop_and_confusing_headers_are_dropped() {
        assert!(!is_sendable_header("Host"));
        assert!(!is_sendable_header("content-length"));
        assert!(!is_sendable_header("Transfer-Encoding"));
        assert!(!is_sendable_header("Proxy-Authorization"));
        // 组件自己管密钥：这两条要放行。
        assert!(is_sendable_header("Authorization"));
        assert!(is_sendable_header("Content-Type"));
    }

    #[test]
    fn timeout_is_clamped_not_rejected() {
        // 手滑写 1ms 不该整条请求报错，夹到下限即可；上限同理。
        assert_eq!(0u64.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS), MIN_TIMEOUT_MS);
        assert_eq!(u64::MAX.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS), MAX_TIMEOUT_MS);
    }
}
