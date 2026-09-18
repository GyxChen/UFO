//! 沙盒脚本的宿主能力后端（纯 Rust，不依赖 rquickjs 类型）。
//!
//! ## 为什么单独一层
//! release 构建开了 `panic = "abort"`（`Cargo.toml`），宿主函数里一次 panic 会带走整个应用，
//! 而 `catch_unwind` 也兜不住 abort。所以这里刻意写成 **panic-free**：
//! 不 `unwrap` / 不 `expect` / 不用索引与 slice，读取一律走 `.get()` 与 `match`。
//! 这一层的函数只返回 `Result<_, String>` 或 `Option<_>`，把失败变成值、不变成 panic；
//! QuickJS 侧（`runner.rs`）负责把 `Err` 转成抛给脚本的 JS 异常。
//!
//! 分层的另一个收益是**可测**：`HttpFetch` / `SecretSource` 是注入点，单测用假实现即可覆盖
//! 超时、私网拦截、密钥缺失等分支，不必真打网络、不必真写系统凭据。
//!
//! ## 安全边界
//! - HTTP 由 **Rust 代发**：渲染进程受 CSP `connect-src` 限制无法直连外网，走这里等于唯一出口，
//!   顺带拦住 loopback / 私网地址，防止用户脚本被拿去扫内网。
//! - 密钥只按**名字**从系统凭据管理器取；值不落盘、不进事件、不进 `console` 日志。

use std::{net::IpAddr, time::Duration};

use crate::modules::agent_status::status::APP_IDENTIFIER;

/// 单次 HTTP 响应体的字节上限：读满即截断，防住脚本拉回巨型响应撑爆内存。
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// 脚本给的超时下限（防手滑写 1ms 让所有请求必失败）。
pub const MIN_HTTP_TIMEOUT_MS: u64 = 500;
/// 脚本给的超时上限（单次请求最长可接受时间）。
pub const MAX_HTTP_TIMEOUT_MS: u64 = 15_000;

/// 一次待执行的 HTTP 请求（宿主侧规范化后的形态）。
pub struct HttpRequest {
    /// 大写 HTTP 方法（GET/POST/…）。
    pub method: String,
    /// 完整 URL（已通过 scheme / 私网校验）。
    pub url: String,
    /// 请求头键值对。
    pub headers: Vec<(String, String)>,
    /// 请求体；GET/HEAD 通常为空。
    pub body: Option<String>,
    /// 本次请求超时（已 clamp 到 [`MIN_HTTP_TIMEOUT_MS`]..=[`MAX_HTTP_TIMEOUT_MS`]）。
    pub timeout_ms: u64,
}

/// HTTP 响应（宿主侧规范化，字段都是可安全塞进 JS 的标量）。
pub struct HttpResponse {
    /// 状态码。
    pub status: u16,
    /// 响应体（文本，最多 [`MAX_RESPONSE_BYTES`]，超出截断）。
    pub body: String,
    /// 响应头键值对。
    pub headers: Vec<(String, String)>,
}

/// HTTP 执行后端：做成 trait 以便单测注入假实现。
pub trait HttpFetch: Send + Sync {
    /// 执行一次请求；失败返回人类可读原因（会被抛回脚本）。
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

/// 生产实现：基于 `ureq`（阻塞式，走默认 rustls）。
pub struct UreqFetch;

impl HttpFetch for UreqFetch {
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        // 全局超时覆盖连接 + 收发整体；ureq 的 timeout_global 是最贴合「单次请求」语义的档。
        let config = ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_millis(request.timeout_ms)))
            // 4xx/5xx 不当错误抛出：脚本要看 `res.status` 自行判断（余额接口常拿 401 表意「key 失效」）。
            .http_status_as_error(false)
            .max_redirects(3)
            .https_only(false)
            .build();
        let agent = ureq::Agent::new_with_config(config);

        // ureq 把「带 body」与「不带 body」的请求构造器分成两个不同类型
        // （RequestBuilder<WithBody> vs <WithoutBody>），无法在同一个 match 里统一成 `mut builder`。
        // 因此按方法分成两条链：body 类方法走 WithBody，其余走 WithoutBody。非标准方法归到 GET。
        let response = match request.method.as_str() {
            "POST" | "PUT" | "PATCH" => {
                let builder = match request.method.as_str() {
                    "POST" => agent.post(&request.url),
                    "PUT" => agent.put(&request.url),
                    _ => agent.patch(&request.url),
                };
                let builder = apply_headers(builder, &request.headers);
                match request.body.clone() {
                    Some(body) => builder.send(body),
                    // 带体方法却没传 body：用空体发送（POST {} 之类场景）。
                    None => builder.send(()),
                }
            }
            "DELETE" => {
                let builder = apply_headers(agent.delete(&request.url), &request.headers);
                builder.call()
            }
            "HEAD" => {
                let builder = apply_headers(agent.head(&request.url), &request.headers);
                builder.call()
            }
            // GET 及其它非标准方法统一按 GET（避免怪方法把构造搞崩）。
            _ => {
                let builder = apply_headers(agent.get(&request.url), &request.headers);
                builder.call()
            }
        };
        let response = response.map_err(|error| sanitize_http_error(&error))?;

        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    value.to_str().unwrap_or_default().to_string(),
                )
            })
            .collect();

        // 限额读取：ureq 的 .reader() 读到 EOF；我们手动读进有上限的缓冲，读满即停。
        let mut body = String::new();
        let mut cursor = response.into_body().into_reader();
        let _ = read_limited(&mut cursor, MAX_RESPONSE_BYTES, &mut body);

        Ok(HttpResponse {
            status,
            body,
            headers,
        })
    }
}

/// 给请求构造器逐条加头。`header` 定义在 `impl<Any> RequestBuilder<Any>` 上，两种 body 形态通用。
fn apply_headers<Any>(
    mut builder: ureq::RequestBuilder<Any>,
    headers: &[(String, String)],
) -> ureq::RequestBuilder<Any> {
    for (key, value) in headers {
        builder = builder.header(key.clone(), value.clone());
    }
    builder
}

/// 从 reader 读入最多 `limit` 字节到 `out`；返回是否触顶。UTF-8 非法字节按 lossy 处理。
fn read_limited<R: std::io::Read>(reader: &mut R, limit: usize, out: &mut String) -> bool {
    let mut buffer = [0_u8; 8 * 1024];
    let mut total = 0_usize;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return false,
            Ok(read) => {
                let room = limit.saturating_sub(total);
                let take = read.min(room);
                out.push_str(&String::from_utf8_lossy(&buffer[..take]));
                total += take;
                if total >= limit {
                    return true;
                }
            }
            Err(_) => return total >= limit,
        }
    }
}

/// 把 ureq 错误转成简短、不含堆栈的文案（脚本侧看得到，不该泄漏内部类型）。
fn sanitize_http_error(error: &ureq::Error) -> String {
    match error {
        ureq::Error::Timeout(_) => "请求超时".to_string(),
        ureq::Error::HostNotFound => "无法解析主机名".to_string(),
        ureq::Error::StatusCode(status) => format!("HTTP 状态错误：{status}"),
        ureq::Error::BadUri(_) => "URL 非法".to_string(),
        ureq::Error::RedirectFailed => "重定向失败".to_string(),
        // 其余（协议 / IO / TLS）回落到 Display 摘要，已不含堆栈。
        other => format!("请求失败：{other}"),
    }
}

/// 校验脚本请求的 URL：只允许 http/https，且目标 host 不得是 loopback / 私网 / 链路本地。
///
/// 返回规范化后的小写 host（供错误信息与去重用）。这里只做**字面**判定 —— 域名解析到内网 IP
/// 的情况无法在不发起连接的前提下拦截，属于用户自写脚本的固有权力（文档已注明），
/// 但本机服务、私有网段的直连端口扫描被这一层挡住。
pub fn validate_http_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL 为空".to_string());
    }
    if trimmed.len() > 4096 {
        return Err("URL 过长".to_string());
    }
    // 只接受 http/https scheme（脚本不能 file:// gopher:// 之类）。
    let after_scheme = match trimmed.split_once("://") {
        Some(("http", rest)) | Some(("https", rest)) => rest,
        Some((other, _)) => return Err(format!("不支持的协议：{other}")),
        None => return Err("URL 缺少 http(s):// 前缀".to_string()),
    };
    // 去掉 userinfo / path / query / fragment，取 host[:port]。
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };
    // IPv6 形如 [::1]:8080，先剥括号再判端口。
    let (host_only, had_bracket) = match host.strip_prefix('[') {
        Some(rest) => (rest.strip_suffix(']').unwrap_or(rest), true),
        None => (host.split(':').next().unwrap_or(host), false),
    };
    let host_lower = host_only.to_ascii_lowercase();
    if host_lower.is_empty() {
        return Err("URL 没有 host".to_string());
    }
    if is_denied_host(&host_lower, had_bracket) {
        return Err("出于安全考虑，脚本不能访问本机或私有网段地址".to_string());
    }
    Ok(host_lower)
}

/// 判断 host 是否落在被禁的范围：回环、私有、链路本地、CGNAT、云元数据等。
fn is_denied_host(host: &str, _ipv6_bracketed: bool) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    // 能按 IP 字面量解析的，直接查地址段。
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_multicast()
                    // 100.64.0.0/10 CGNAT（不少 Tailscale / 云内网用它）
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
            }
            IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unspecified()
                    || v6.is_multicast()
                    // fc00::/7 唯一本地地址
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    // fe80::/10 链路本地
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
    }
    // 云元数据 / 短主机名（无点，非 localhost）也拒 —— 减少 SSRF 面。
    if host == "169.254.169.254" || !host.contains('.') && !host.contains(':') {
        return true;
    }
    false
}

/// 密钥来源：脚本按 (service, name) 取一个字符串。做成 trait 便于单测注入。
pub trait SecretSource: Send + Sync {
    /// 取密钥值；未配置返回 None。
    fn get(&self, service: &str, name: &str) -> Option<String>;
    /// 写入密钥值。
    fn set(&self, service: &str, name: &str, value: &str) -> Result<(), String>;
    /// 删除密钥值。
    fn delete(&self, service: &str, name: &str) -> Result<(), String>;
}

/// 生产实现：`keyring`（Windows = 凭据管理器 / macOS = Keychain / Linux = Secret Service）。
pub struct KeyringSecrets;

impl KeyringSecrets {
    /// 由 endpoint id 派生凭据 service 名：与 App 标识绑定，删除档案时可整片清理。
    pub fn service_for(endpoint_id: &str) -> String {
        // endpoint_id 形如 `api.example.com:443`，替换掉分隔符冲突字符。
        let sanitized: String = endpoint_id
            .chars()
            .map(|c| if c == '/' || c == '\\' { '_' } else { c })
            .collect();
        format!("{}/flow/{}", APP_IDENTIFIER, sanitized)
    }
}

impl SecretSource for KeyringSecrets {
    fn get(&self, service: &str, name: &str) -> Option<String> {
        let entry = keyring::Entry::new(service, name).ok()?;
        entry.get_password().ok()
    }

    fn set(&self, service: &str, name: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(service, name).map_err(|error| error.to_string())?;
        entry.set_password(value).map_err(|error| error.to_string())
    }

    fn delete(&self, service: &str, name: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(service, name).map_err(|error| error.to_string())?;
        // 「不存在」当成功处理：删除应幂等，便于 UI 反复点。
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// 进程内脚本 KV 缓存（给脚本做增量与去抖，如「5 分钟内余额没变就不重复查」）。
///
/// 值以 JSON 字符串存（cache 边界在 JS 侧 stringify/parse），带绝对过期时刻；
/// 读取时惰性丢弃过期项。按 endpoint 分组，避免跨 endpoint 泄漏。
#[derive(Default)]
pub struct CacheStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, std::collections::HashMap<String, CacheEntry>>>,
}

struct CacheEntry {
    value: String,
    expires_at_ms: i64,
}

impl CacheStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取值；缺失或过期返回 None（并顺手删过期项）。
    pub fn get(&self, scope: &str, key: &str, now_ms: i64) -> Option<String> {
        let mut store = self.inner.lock().ok()?;
        let bucket = store.get_mut(scope)?;
        match bucket.get(key) {
            Some(entry) if entry.expires_at_ms > now_ms => Some(entry.value.clone()),
            Some(_) => {
                bucket.remove(key);
                None
            }
            None => None,
        }
    }

    /// 写值；`ttl_ms` <= 0 时按默认 10 分钟过期。
    pub fn set(&self, scope: &str, key: &str, value: String, ttl_ms: i64, now_ms: i64) {
        let ttl = if ttl_ms <= 0 { 600_000 } else { ttl_ms };
        if let Ok(mut store) = self.inner.lock() {
            store.entry(scope.to_string()).or_default().insert(
                key.to_string(),
                CacheEntry {
                    value,
                    expires_at_ms: now_ms.saturating_add(ttl),
                },
            );
        }
    }

    /// 清空某个 endpoint 的缓存（删除档案 / 改脚本后可选调用）。
    pub fn clear_scope(&self, scope: &str) {
        if let Ok(mut store) = self.inner.lock() {
            store.remove(scope);
        }
    }
}
