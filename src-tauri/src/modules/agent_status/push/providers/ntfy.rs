//! ntfy：公开主题的推送服务。
//!
//! 协议：`POST {server}/{topic}`，正文放 body，标题走 `Title` 请求头。
//! 自建服务器（`https://ntfy.example.com`，或局域网里的 `http://192.168.x.x:8080`）由用户在
//! 服务级地址栏里显式填 —— 与流程脚本不同，这里的地址是**用户自己配的**、不是任意代码拼出来的，
//! 所以允许私网（策略与理由见 `push/mod.rs` 的模块文档）。
//!
//! ⚠️ **主题是公开的**：知道主题串的人都能订阅到内容。因此提问原文这类内容默认不进正文，
//! 要用户在「提醒内容」里显式勾选（见 `reference/doc/guide/agent-status-push-guide.md` §3）。
//! ⚠️ 需要访问令牌的自建服务器**不再支持**：本版本按用户要求不存任何密钥，也不带鉴权请求头。

use super::{encode_segment, normalize_server, PushProvider, SendRequest, ServerSpec};
use crate::modules::agent_status::flow::HttpRequest;

/// 单次推送的超时（毫秒）：够慢网走完 TLS，又明显短到不会让人以为卡死。
const PUSH_TIMEOUT_MS: u64 = 10_000;

/// 官方公共服务器：用户把地址栏留空就用它。
const DEFAULT_SERVER: &str = "https://ntfy.sh";

/// ntfy 适配器单例；由 `push::build_providers()` 注册。
pub struct NtfyProvider;

impl PushProvider for NtfyProvider {
    fn id(&self) -> &'static str {
        "ntfy"
    }

    fn label(&self) -> &'static str {
        "ntfy"
    }

    fn server(&self) -> ServerSpec {
        ServerSpec {
            label: "服务器",
            default: DEFAULT_SERVER,
            help: "官方公共服务器留空即可；自建填自己的地址（需要鉴权的服务器不支持）",
        }
    }

    /// 向 `{server}/{topic}` POST 正文；标题走请求头，正文就是消息的 `body`。
    fn send(
        &self,
        request: &SendRequest<'_>,
        http: &dyn crate::modules::agent_status::flow::HttpFetch,
    ) -> Result<u16, String> {
        let topic = request.topic.trim();
        if topic.is_empty() {
            return Err("ntfy 主题为空，先在右侧列表填一行主题".to_string());
        }
        let server = normalize_server(request.server, DEFAULT_SERVER)?;
        // 主题进路径必须整体编码：含 `/` 的主题会把请求挪到别的主题上去。
        let url = format!("{}/{}", server, encode_segment(topic));

        let response = http.fetch(&HttpRequest {
            method: "POST".to_string(),
            url,
            headers: vec![
                ("Title".to_string(), request.message.title.clone()),
                (
                    "Content-Type".to_string(),
                    "text/plain; charset=utf-8".to_string(),
                ),
            ],
            body: Some(request.message.body.clone()),
            timeout_ms: PUSH_TIMEOUT_MS,
        })?;
        if !(200..300).contains(&response.status) {
            return Err(describe_failure(response.status, &response.body));
        }
        Ok(response.status)
    }
}

/// 把非 2xx 翻译成「能照着修」的一句话。
fn describe_failure(status: u16, body: &str) -> String {
    let hint = match status {
        400 => "主题名不合法",
        // 这两种最常见的原因是服务器开了访问控制 —— 而本版本不再带令牌，只能换服务器或关掉鉴权。
        401 | 403 => "服务器拒绝发布（开了访问控制，或主题不允许匿名发布）",
        404 => "服务器地址或主题不存在",
        429 => "被限流了（发送太快）",
        _ => "服务端拒绝",
    };
    // 响应体只取第一行、截到 120 字：够定位，又不把服务端内部信息整段搬进面板。
    let detail: String = body
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect();
    if detail.is_empty() {
        format!("ntfy 返回 {status}：{hint}")
    } else {
        format!("ntfy 返回 {status}：{hint}（{detail}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agent_status::push::message::PushMessage;
    use crate::modules::agent_status::push::testing::FakeFetch;

    /// 组一份请求：地址与主题都是普通的借用水值。
    fn request_with<'a>(
        server: &'a str,
        topic: &'a str,
        message: &'a PushMessage,
    ) -> SendRequest<'a> {
        SendRequest { server, topic, message }
    }

    fn message() -> PushMessage {
        PushMessage {
            title: "UFO · 一轮跑完".to_string(),
            body: "claude:8f21c3a4 等你输入".to_string(),
        }
    }

    #[test]
    fn posts_topic_in_path_with_the_title_header() {
        let message = message();
        let fetch = FakeFetch::ok();
        NtfyProvider
            .send(&request_with("https://ntfy.sh", "ufo-gyx-home", &message), &fetch)
            .expect("发送成功");
        let last = fetch.last();
        assert_eq!(last.method, "POST");
        assert_eq!(last.url, "https://ntfy.sh/ufo-gyx-home");
        assert_eq!(last.body.as_deref(), Some("claude:8f21c3a4 等你输入"));
        assert!(last
            .headers
            .iter()
            .any(|(key, value)| key == "Title" && value == "UFO · 一轮跑完"));
        // 只剩标题与类型两条头：优先级与鉴权都随功能一起删掉了。
        assert!(!last.headers.iter().any(|(key, _)| key == "Priority"));
        assert!(!last.headers.iter().any(|(key, _)| key == "Authorization"));
    }

    #[test]
    fn empty_server_falls_back_to_the_official_one() {
        let message = message();
        let fetch = FakeFetch::ok();
        NtfyProvider
            .send(&request_with("", "ufo", &message), &fetch)
            .expect("留空等于用官方服务器");
        assert_eq!(fetch.last().url, "https://ntfy.sh/ufo");
    }

    #[test]
    fn topic_is_percent_encoded_so_paths_cannot_escape() {
        let message = message();
        let fetch = FakeFetch::ok();
        NtfyProvider
            .send(&request_with("https://ntfy.sh", "a/../b", &message), &fetch)
            .expect("发送成功");
        // `..` 段被编码成一个普通路径段，不会把请求挪到别的主题上。
        assert_eq!(fetch.last().url, "https://ntfy.sh/a%2F..%2Fb");
    }

    #[test]
    fn self_hosted_lan_server_is_allowed_and_slashes_normalized() {
        let message = message();
        let fetch = FakeFetch::ok();
        NtfyProvider
            .send(&request_with("http://192.168.1.20:8080///", "ufo", &message), &fetch)
            .expect("自建允许走局域网");
        assert_eq!(fetch.last().url, "http://192.168.1.20:8080/ufo");
    }

    #[test]
    fn server_without_scheme_is_rejected() {
        let message = message();
        let fetch = FakeFetch::ok();
        let error = NtfyProvider
            .send(&request_with("ntfy.example.com", "ufo", &message), &fetch)
            .expect_err("缺 scheme 必须失败");
        assert!(error.contains("http"), "{error}");
        assert!(fetch.is_empty(), "校验失败时不该发出请求");
    }

    #[test]
    fn empty_topic_fails_without_hitting_network() {
        let message = message();
        let fetch = FakeFetch::ok();
        assert!(NtfyProvider
            .send(&request_with("https://ntfy.sh", "   ", &message), &fetch)
            .is_err());
        assert!(fetch.is_empty());
    }

    #[test]
    fn non_2xx_becomes_an_actionable_message() {
        let message = message();
        let fetch = FakeFetch::failing(401, "unauthorized");
        let error = NtfyProvider
            .send(&request_with("https://ntfy.sh", "ufo", &message), &fetch)
            .expect_err("401 是失败");
        // 文案必须指向真正的出路（这套服务器要鉴权，而我们已经不带令牌了）。
        assert!(error.contains("访问控制"), "{error}");
        assert!(error.contains("401"), "{error}");
    }

    #[test]
    fn server_spec_declares_an_https_default_and_help() {
        let spec = NtfyProvider.server();
        assert_eq!(spec.label, "服务器");
        assert_eq!(spec.default, "https://ntfy.sh");
        assert!(!spec.help.is_empty(), "地址栏要有一句说明");
    }
}
