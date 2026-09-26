//! 推送服务（provider）契约：一种投递服务实现一次 [`PushProvider`]。
//!
//! ## 分层
//! - **本模块只认「发一条」**：拿到已解析好的服务器地址、主题串和一条通用文案，产出 URL / 请求体
//!   并发出去。它**不读配置、不起线程、不判灯态** —— 那些全在
//!   [`crate::modules::agent_status::push`](super) 的调度层，于是 provider 可以纯单测
//!   （喂 [`SendRequest`] 断言 URL 与请求体）。
//! - **一个服务只有一个可配字段：服务器地址**（[`ServerSpec`] 只带文案与默认值）。收件方固定是
//!   「名字 + 主题」两列，Bark 的设备密钥就是一个主题串，因此不再有通用的字段规格表 ——
//!   少一层抽象，前端也就少一套按 spec 渲染的表单。
//!   ⚠️ 代价：需要额外凭据的服务（如开了访问控制的自建 ntfy）不在支持范围内。
//!
//! ## panic 红线
//! release 构建是 `panic = "abort"`（`Cargo.toml`），这里一次 panic 会带走整个 App。
//! 因此不 `unwrap` / 不 `expect` / 不裸索引 / 不按字节 slice；失败一律 `Err(String)`。

pub mod bark;
pub mod ntfy;

use serde::Serialize;

use super::message::PushMessage;
use crate::modules::agent_status::flow::HttpFetch;

/// 服务器地址这一栏的文案与默认值：由 provider 自带，面板因此不认识任何具体服务。
#[derive(Debug, Clone, Serialize)]
pub struct ServerSpec {
    /// 表单标签。
    pub label: &'static str,
    /// 官方默认地址；用户留空时回落它。
    pub default: &'static str,
    /// 标签下方的一行说明。
    pub help: &'static str,
}

/// 一次投递的输入：调度层已把地址、主题与文案备齐。
///
/// 主题**不是密钥**（用户明确要求它以明文列在表里），所以直接借给它，不再有「单独去凭据管理器取」那一步。
pub struct SendRequest<'a> {
    /// 服务级地址原样值；空串表示用 [`ServerSpec::default`]。
    pub server: &'a str,
    /// 主题串（ntfy 的 topic / Bark 的设备密钥）。
    pub topic: &'a str,
    /// 通用文案（标题 + 正文）。
    pub message: &'a PushMessage,
}

/// 一种推送服务。
pub trait PushProvider: Send + Sync {
    /// 稳定 id：存进配置的 `channels[].provider`，也是前端注册表的键。
    fn id(&self) -> &'static str;

    /// 展示名。
    fn label(&self) -> &'static str;

    /// 服务器地址这一栏怎么呈现（标签 / 官方默认 / 一行说明）。
    fn server(&self) -> ServerSpec;

    /// 真正发一条，成功返回 HTTP 状态码。失败返回**可行动的**原因（面板直接显示给用户）。
    ///
    /// 耗时由调度层量（provider 不碰时钟），所以这里只回状态码。
    /// ⚠️ 不得 panic；不得自己起线程；超时与响应体上限由 [`HttpFetch`] 那一侧兜住。
    fn send(&self, request: &SendRequest<'_>, http: &dyn HttpFetch) -> Result<u16, String>;
}

/// 拼 URL 时把路径段做百分号编码（主题名可能含中文、`/`，设备密钥可能被粘进空格）。
///
/// 只保留 unreserved 字符（字母数字与 `-_.~`），其余按 UTF-8 逐字节转 `%XX`。
pub fn encode_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// 去掉地址尾部的 `/`，避免拼出 `//path` 这种双斜杠。
pub fn trim_trailing_slash(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

/// 规范化并校验服务器地址：空值回落默认、必须带 http(s) scheme、去掉尾部斜杠。
///
/// ⚠️ 与流程脚本的 `validate_http_url` **刻意不同**：那边拦的是用户自写脚本拼出来的任意地址，
/// 所以挡掉 loopback / 私网 / 云元数据；这里的地址是用户在设置页**自己显式填**的固定字段，
/// 自建 ntfy / Bark 服务器落在局域网里是常见用法，挡掉就等于砍功能。
/// 因此这里只校 scheme 与形状，地址段的取舍写在 `push/mod.rs` 的模块文档里。
pub fn normalize_server(raw: &str, default: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let candidate = if trimmed.is_empty() { default } else { trimmed };
    if candidate.len() > 4096 {
        return Err("服务器地址过长".to_string());
    }
    let lower = candidate.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("服务器地址必须是 http(s):// 开头".to_string());
    }
    // 含空白或控制字符的地址一定是被填坏了（也挡掉换头注入的雏形）。
    if candidate.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("服务器地址含非法字符".to_string());
    }
    Ok(trim_trailing_slash(candidate))
}
